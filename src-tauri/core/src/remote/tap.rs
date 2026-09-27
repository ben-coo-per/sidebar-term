//! Output taps: what the Host keeps of each Session's output for its clients. A Session has a
//! tap from its first byte of output: a ring of its recent output (replayed on attach, so a
//! client does not start from a blank screen), the pty's size (a phone renders at the Host's
//! size and never resizes), the clients attached, each fed the live output as it arrives, and
//! the [`Marks`] a [`Scanner`] reads in the output as it passes: the OSC 0 / 2 title, BELs and
//! when output last arrived, from which `status.rs` derives Agent status without a Terminal.
//! `session.rs` feeds the taps; `remote/server.rs` attaches to them; the monitor reads the
//! marks. No Tauri types.
//!
//! A subscriber that falls [`SUBSCRIBER_QUEUE`] frames behind is dropped rather than stalling the
//! reader thread: its connection closes, and the phone reconnects and replays the ring.

use crate::model::SessionId;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, error::TrySendError};

/// Bytes of recent output kept per Session; what an attaching phone replays.
pub const SCROLLBACK_MAX: usize = 256 * 1024;
/// After trimming, the ring keeps this much, so trimming is not a per-byte cost.
const SCROLLBACK_KEEP: usize = SCROLLBACK_MAX * 3 / 4;
/// How far past the cut to look for a line break, so a replay starts on a line.
const LINE_SEARCH: usize = 4 * 1024;
/// Frames a subscriber may fall behind before it is dropped.
const SUBSCRIBER_QUEUE: usize = 512;
/// An OSC longer than this is not a title; it is skipped to its terminator.
const OSC_MAX: usize = 4 * 1024;
/// `Marks::last_output_at` moves at most this often, as the webview's `activity` events were
/// throttled: a redraw right after a BEL does not count as fresh output (`status.rs`).
const OUTPUT_THROTTLE: Duration = Duration::from_millis(250);

/// What the Host read in a Session's output so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    /// The latest OSC 0 / 2 title (`""` once cleared); `None` before the first.
    pub title: Option<String>,
    /// BELs outside escape sequences.
    pub bells: u32,
    pub last_bell_at: Option<Instant>,
    /// When output last arrived, throttled to [`OUTPUT_THROTTLE`].
    pub last_output_at: Option<Instant>,
}

/// Reads OSC 0 / 2 titles (`ESC ] 0 ; title BEL`, or `ESC \` as the terminator) and bare BELs
/// out of a byte stream, across chunk boundaries: what xterm.js would report as `onTitleChange`
/// and `onBell`. Everything else passes unparsed.
#[derive(Debug, Default)]
pub struct Scanner {
    state: ScanState,
    /// The OSC being read, `Ps;Pt`.
    osc: Vec<u8>,
    /// The OSC being read passed [`OSC_MAX`]: dropped at its terminator.
    overlong: bool,
    marks: Marks,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum ScanState {
    #[default]
    Text,
    /// After ESC.
    Esc,
    /// Inside `ESC ]`.
    Osc,
    /// Inside an OSC, after ESC: `\` ends it.
    OscEsc,
}

impl Scanner {
    pub fn marks(&self) -> &Marks {
        &self.marks
    }

    pub fn feed(&mut self, bytes: &[u8], now: Instant) {
        if bytes.is_empty() {
            return;
        }
        if self
            .marks
            .last_output_at
            .is_none_or(|t| now.duration_since(t) >= OUTPUT_THROTTLE)
        {
            self.marks.last_output_at = Some(now);
        }
        for &b in bytes {
            self.step(b, now);
        }
    }

    fn step(&mut self, b: u8, now: Instant) {
        self.state = match (self.state, b) {
            (ScanState::Text, 0x1b) => ScanState::Esc,
            (ScanState::Text, 0x07) => {
                self.marks.bells = self.marks.bells.wrapping_add(1);
                self.marks.last_bell_at = Some(now);
                ScanState::Text
            }
            (ScanState::Text, _) => ScanState::Text,
            (ScanState::Esc, b']') => {
                self.osc.clear();
                self.overlong = false;
                ScanState::Osc
            }
            (ScanState::Esc, 0x1b) => ScanState::Esc,
            (ScanState::Esc, _) => ScanState::Text,
            (ScanState::Osc, 0x07) => {
                self.finish_osc();
                ScanState::Text
            }
            (ScanState::Osc, 0x1b) => ScanState::OscEsc,
            (ScanState::Osc, _) => {
                if self.osc.len() < OSC_MAX {
                    self.osc.push(b);
                } else {
                    self.overlong = true;
                }
                ScanState::Osc
            }
            (ScanState::OscEsc, b'\\') => {
                self.finish_osc();
                ScanState::Text
            }
            // ESC inside an OSC that is not ST: the sequence is malformed; treat the ESC as
            // the start of whatever follows.
            (ScanState::OscEsc, 0x1b) => ScanState::Esc,
            (ScanState::OscEsc, b']') => {
                self.osc.clear();
                self.overlong = false;
                ScanState::Osc
            }
            (ScanState::OscEsc, _) => ScanState::Text,
        };
    }

    fn finish_osc(&mut self) {
        if self.overlong {
            return;
        }
        let Some(semi) = self.osc.iter().position(|&b| b == b';') else {
            return;
        };
        let (ps, pt) = (&self.osc[..semi], &self.osc[semi + 1..]);
        if ps == b"0" || ps == b"2" {
            self.marks.title = Some(String::from_utf8_lossy(pt).into_owned());
        }
    }
}

/// What an attached subscriber receives, tagged with the Session it came from.
#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    Output(Vec<u8>),
    Resized { cols: u16, rows: u16 },
    /// The Session ended; nothing more follows.
    Exit,
}

pub type FrameSender = mpsc::Sender<(SessionId, Frame)>;

/// One tap per live Session, created on its first output or `open`.
#[derive(Default)]
pub struct Taps {
    taps: Mutex<HashMap<SessionId, Tap>>,
    next_sub: AtomicU64,
}

#[derive(Default)]
struct Tap {
    scrollback: Vec<u8>,
    cols: u16,
    rows: u16,
    subs: Vec<Subscriber>,
    scanner: Scanner,
}

struct Subscriber {
    id: u64,
    tx: FrameSender,
}

/// What an attach returns: the ring so far and the pty size, plus the subscription's id.
pub struct Attached {
    pub sub: u64,
    pub scrollback: Vec<u8>,
    pub cols: u16,
    pub rows: u16,
}

impl Taps {
    /// A channel sized for one subscriber; pass its sender to `attach`.
    pub fn channel() -> (FrameSender, mpsc::Receiver<(SessionId, Frame)>) {
        mpsc::channel(SUBSCRIBER_QUEUE)
    }

    /// Record the pty size of a new Session (output may have arrived already).
    pub fn open(&self, id: SessionId, cols: u16, rows: u16) {
        let mut taps = lock(&self.taps);
        let tap = taps.entry(id).or_default();
        tap.cols = cols;
        tap.rows = rows;
    }

    /// Append output to the ring and hand it to every subscriber.
    pub fn push(&self, id: SessionId, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut taps = lock(&self.taps);
        let tap = taps.entry(id).or_default();
        tap.scanner.feed(bytes, Instant::now());
        tap.scrollback.extend_from_slice(bytes);
        trim(&mut tap.scrollback);
        fan_out(tap, id, || Frame::Output(bytes.to_vec()));
    }

    /// What the Host read in a live Session's output: its title, BELs, when output last came.
    pub fn marks(&self, id: SessionId) -> Option<Marks> {
        lock(&self.taps).get(&id).map(|t| t.scanner.marks().clone())
    }

    /// How many subscribers a live Session has (0 for none, or no such Session).
    pub fn subscribers(&self, id: SessionId) -> usize {
        lock(&self.taps).get(&id).map_or(0, |t| t.subs.len())
    }

    pub fn resized(&self, id: SessionId, cols: u16, rows: u16) {
        let mut taps = lock(&self.taps);
        let Some(tap) = taps.get_mut(&id) else { return };
        if tap.cols == cols && tap.rows == rows {
            return;
        }
        tap.cols = cols;
        tap.rows = rows;
        fan_out(tap, id, || Frame::Resized { cols, rows });
    }

    /// The Session ended: tell every subscriber and drop the tap.
    pub fn close(&self, id: SessionId) {
        let Some(tap) = lock(&self.taps).remove(&id) else { return };
        for s in tap.subs {
            let _ = s.tx.try_send((id, Frame::Exit));
        }
    }

    /// Subscribe `tx` to a live Session's output. The ring and the size are read under the same
    /// lock that registers the subscriber, so no output falls between replay and live frames.
    pub fn attach(&self, id: SessionId, tx: FrameSender) -> Option<Attached> {
        let mut taps = lock(&self.taps);
        let tap = taps.get_mut(&id)?;
        let sub = self.next_sub.fetch_add(1, Ordering::SeqCst) + 1;
        tap.subs.push(Subscriber { id: sub, tx });
        Some(Attached {
            sub,
            scrollback: tap.scrollback.clone(),
            cols: tap.cols,
            rows: tap.rows,
        })
    }

    pub fn detach(&self, id: SessionId, sub: u64) {
        if let Some(tap) = lock(&self.taps).get_mut(&id) {
            tap.subs.retain(|s| s.id != sub);
        }
    }

    /// Whether a live Session has a tap (it is attachable).
    pub fn has(&self, id: SessionId) -> bool {
        lock(&self.taps).contains_key(&id)
    }

    /// Whether subscription `sub` is still fed: false once the Session ended or the subscriber
    /// fell behind and was dropped.
    pub fn attached(&self, id: SessionId, sub: u64) -> bool {
        lock(&self.taps)
            .get(&id)
            .is_some_and(|t| t.subs.iter().any(|s| s.id == sub))
    }
}

/// Send one frame to every subscriber, dropping those that are gone or too far behind.
fn fan_out(tap: &mut Tap, id: SessionId, frame: impl Fn() -> Frame) {
    tap.subs.retain(|s| match s.tx.try_send((id, frame())) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => false,
    });
}

/// Keep the ring under `SCROLLBACK_MAX`, cutting to `SCROLLBACK_KEEP` on a line break when one
/// is near, so a replay does not begin inside an escape sequence.
fn trim(ring: &mut Vec<u8>) {
    if ring.len() <= SCROLLBACK_MAX {
        return;
    }
    let mut cut = ring.len() - SCROLLBACK_KEEP;
    let search_end = (cut + LINE_SEARCH).min(ring.len());
    if let Some(nl) = ring[cut..search_end].iter().position(|&b| b == b'\n') {
        cut += nl + 1;
    }
    ring.drain(..cut);
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(rx: &mut mpsc::Receiver<(SessionId, Frame)>) -> Vec<(SessionId, Frame)> {
        let mut out = Vec::new();
        while let Ok(f) = rx.try_recv() {
            out.push(f);
        }
        out
    }

    #[test]
    fn attach_replays_the_ring_then_streams_live_output() {
        let taps = Taps::default();
        taps.push(7, b"before ");
        taps.open(7, 120, 40);
        let (tx, mut rx) = Taps::channel();
        let a = taps.attach(7, tx).expect("live session");
        assert_eq!(a.scrollback, b"before ");
        assert_eq!((a.cols, a.rows), (120, 40));

        taps.push(7, b"after");
        taps.resized(7, 100, 30);
        taps.resized(7, 100, 30); // no change: no frame
        assert_eq!(
            drain(&mut rx),
            [
                (7, Frame::Output(b"after".to_vec())),
                (7, Frame::Resized { cols: 100, rows: 30 })
            ]
        );

        taps.detach(7, a.sub);
        taps.push(7, b"unseen");
        assert!(drain(&mut rx).is_empty());
        assert!(taps.has(7));
    }

    #[test]
    fn close_tells_subscribers_and_forgets_the_session() {
        let taps = Taps::default();
        taps.open(1, 80, 24);
        let (tx, mut rx) = Taps::channel();
        taps.attach(1, tx).unwrap();
        taps.close(1);
        assert_eq!(drain(&mut rx), [(1, Frame::Exit)]);
        assert!(!taps.has(1));
        let (tx, _rx) = Taps::channel();
        assert!(taps.attach(1, tx).is_none());
    }

    #[test]
    fn a_subscriber_that_falls_behind_is_dropped() {
        let taps = Taps::default();
        taps.open(1, 80, 24);
        let (tx, mut rx) = Taps::channel();
        taps.attach(1, tx).unwrap();
        for _ in 0..SUBSCRIBER_QUEUE + 5 {
            taps.push(1, b"x");
        }
        assert_eq!(drain(&mut rx).len(), SUBSCRIBER_QUEUE);
        // Gone: nothing more arrives, and the sender side is dropped, which ends the receiver.
        taps.push(1, b"y");
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn the_ring_is_bounded_and_cut_on_a_line() {
        let mut ring = Vec::new();
        let line = b"0123456789abcdef\n"; // 17 bytes
        while ring.len() <= SCROLLBACK_MAX {
            ring.extend_from_slice(line);
        }
        trim(&mut ring);
        assert!(ring.len() <= SCROLLBACK_KEEP);
        assert!(ring.len() > SCROLLBACK_KEEP - LINE_SEARCH);
        assert_eq!(&ring[..17], line, "replay starts on a line");
        assert_eq!(ring.len() % 17, 0);

        // No line break anywhere near: plain cut.
        let mut solid = vec![b'z'; SCROLLBACK_MAX + 1];
        trim(&mut solid);
        assert_eq!(solid.len(), SCROLLBACK_KEEP);
    }

    #[test]
    fn output_before_open_is_kept() {
        let taps = Taps::default();
        taps.push(3, b"early");
        taps.open(3, 80, 24);
        let (tx, _rx) = Taps::channel();
        assert_eq!(taps.attach(3, tx).unwrap().scrollback, b"early");
    }

    // --- The scanner -------------------------------------------------------------------------

    fn scan(chunks: &[&[u8]]) -> Marks {
        let mut s = Scanner::default();
        let base = Instant::now();
        for (i, c) in chunks.iter().enumerate() {
            s.feed(c, base + Duration::from_secs(i as u64));
        }
        s.marks().clone()
    }

    #[test]
    fn titles_are_read_with_either_terminator_and_other_oscs_are_ignored() {
        assert_eq!(scan(&[b"\x1b]0;first\x07"]).title.as_deref(), Some("first"));
        assert_eq!(scan(&[b"\x1b]2;second\x1b\\"]).title.as_deref(), Some("second"));
        assert_eq!(scan(&[b"\x1b]0;a\x07\x1b]2;b\x07"]).title.as_deref(), Some("b"), "the latest wins");
        assert_eq!(scan(&[b"\x1b]0;\x07"]).title.as_deref(), Some(""), "cleared");
        assert_eq!(scan(&[b"\x1b]1;icon\x07\x1b]8;;http://x\x1b\\"]).title, None);
        assert_eq!(scan(&[b"\x1b]0;\xe2\x97\x90 Fix\x07"]).title.as_deref(), Some("◐ Fix"), "UTF-8");
        assert_eq!(scan(&[b"plain \x1b[31mred\x1b[0m"]).title, None, "CSI is not OSC");
    }

    #[test]
    fn sequences_split_across_reads_are_joined() {
        let m = scan(&[b"\x1b", b"]0;ti", b"tle", b"\x07 rest"]);
        assert_eq!(m.title.as_deref(), Some("title"));
        assert_eq!(m.bells, 0, "the OSC's terminator is not a bell");
        let m = scan(&[b"\x1b]2;st-ended\x1b", b"\\"]);
        assert_eq!(m.title.as_deref(), Some("st-ended"));
        let m = scan(&[b"\x1b]0;half", b"\x1b]0;whole\x07"]);
        assert_eq!(m.title.as_deref(), Some("whole"), "an OSC cut short by another");
    }

    #[test]
    fn bells_count_outside_escape_sequences_and_output_is_timed() {
        let m = scan(&[b"a\x07b", b"\x07\x07"]);
        assert_eq!(m.bells, 3);
        let base = m.last_bell_at.unwrap();
        assert!(m.last_output_at.unwrap() <= base);
        let m = scan(&[b"\x1b]0;t\x07"]);
        assert_eq!((m.bells, m.last_bell_at), (0, None));
        let m = scan(&[b"\x1b[31m\x07"]);
        assert_eq!(m.bells, 1, "a BEL after a CSI is a bell");
        assert_eq!(scan(&[]).last_output_at, None);
        assert_eq!(Scanner::default().marks(), &Marks::default());
    }

    #[test]
    fn the_last_output_time_is_throttled() {
        let mut s = Scanner::default();
        let t0 = Instant::now();
        s.feed(b"a", t0);
        s.feed(b"b", t0 + Duration::from_millis(100));
        assert_eq!(s.marks().last_output_at, Some(t0), "within the throttle: unchanged");
        let t1 = t0 + OUTPUT_THROTTLE;
        s.feed(b"c", t1);
        assert_eq!(s.marks().last_output_at, Some(t1));
    }

    #[test]
    fn an_overlong_osc_is_skipped() {
        let mut long = b"\x1b]0;".to_vec();
        long.extend(std::iter::repeat_n(b'x', OSC_MAX + 10));
        long.extend_from_slice(b"\x07\x1b]0;after\x07");
        assert_eq!(scan(&[&long]).title.as_deref(), Some("after"));
    }

    #[test]
    fn taps_expose_the_marks_and_the_subscriber_count() {
        let taps = Taps::default();
        assert_eq!(taps.marks(1), None);
        taps.push(1, b"\x1b]0;hi\x07\x07");
        let m = taps.marks(1).unwrap();
        assert_eq!((m.title.as_deref(), m.bells), (Some("hi"), 1));
        assert_eq!(taps.subscribers(1), 0);
        let (tx, _rx) = Taps::channel();
        let a = taps.attach(1, tx).unwrap();
        assert_eq!(taps.subscribers(1), 1);
        taps.detach(1, a.sub);
        assert_eq!(taps.subscribers(1), 0);
        assert_eq!(taps.subscribers(9), 0);
    }
}
