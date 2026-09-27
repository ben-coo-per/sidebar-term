//! Output taps: what Remote needs from each Session so a phone can attach to it. A Session has a
//! tap from its first byte of output: a ring of its recent output (replayed on attach, so the
//! phone does not start from a blank screen), the pty's size (the phone renders at the Mac's
//! size and never resizes), and the phones attached, each fed the live output as it arrives.
//! `session.rs` feeds the taps; `remote/server.rs` attaches to them. No Tauri types.
//!
//! A subscriber that falls [`SUBSCRIBER_QUEUE`] frames behind is dropped rather than stalling the
//! reader thread: its connection closes, and the phone reconnects and replays the ring.

use crate::model::SessionId;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use tokio::sync::mpsc::{self, error::TrySendError};

/// Bytes of recent output kept per Session; what an attaching phone replays.
pub const SCROLLBACK_MAX: usize = 256 * 1024;
/// After trimming, the ring keeps this much, so trimming is not a per-byte cost.
const SCROLLBACK_KEEP: usize = SCROLLBACK_MAX * 3 / 4;
/// How far past the cut to look for a line break, so a replay starts on a line.
const LINE_SEARCH: usize = 4 * 1024;
/// Frames a subscriber may fall behind before it is dropped.
const SUBSCRIBER_QUEUE: usize = 512;

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
        tap.scrollback.extend_from_slice(bytes);
        trim(&mut tap.scrollback);
        fan_out(tap, id, || Frame::Output(bytes.to_vec()));
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
}
