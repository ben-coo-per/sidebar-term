//! Memory Guard: while on, freezes the heaviest Tab when memory gets tight, and thaws frozen Tabs
//! one at a time once it frees up. The Tray's Memory Guard button turns it on.
//!
//! Freezing stops every process of a Session with SIGSTOP; thawing continues them with SIGCONT.
//! A frozen process keeps the memory it holds (macOS may compress or swap it, since nothing touches
//! it), but stops using CPU and stops growing. So Memory Guard makes heavy work take turns rather
//! than thrash: memory falls back when something else finishes, and the frozen Tab then carries on.
//!
//! Policy (`Inner::decide`, on every Activity sample while on; `activity.rs` samples for it):
//! - Memory Used over the limit (a percent of physical memory, set in Settings) for [`OVER_FOR`]:
//!   freeze the Session using the most memory, if it uses at least [`MIN_MEM`]. Never the Session
//!   in view, one already frozen, or one spared (below).
//! - Memory Used under the limit less [`THAW_GAP`] points: thaw the Session frozen first.
//! - After either, wait [`SETTLE`] before the next, so memory shows the effect first.
//! - Going to a frozen Tab thaws it at once and spares it: it is not frozen again until memory
//!   falls under the thaw line. Turning Memory Guard off thaws everything.
//!
//! Order matters with job control. Stopping a shell's foreground job makes the shell take the tty
//! back ("suspended"), so the shell is stopped first, then its descendants, parents before
//! children; thawing goes the other way, the shell last, when its job is already running again.
//!
//! A frozen Session is thawed before it is killed (closing its Tab, a webview reload, quitting):
//! SIGHUP does not reach a stopped process, and processes in their own process groups would stay
//! stopped forever. For a crash, what is frozen is kept in `frozen.json`; the next launch thaws it.
//! Each process is recorded with its start time and only signalled if that still matches, so a
//! recycled pid is never hit.

use crate::activity::{run_ps, rusage, PsRow};
use crate::layout;
use crate::model::{ActivitySnapshot, FrozenSession, GuardSnapshot, ProbeTarget, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Limit when Settings has none.
pub const DEFAULT_LIMIT: u8 = 85;
/// Limits Settings may set, in percent of physical memory.
const LIMITS: std::ops::RangeInclusive<u8> = 50..=95;
/// Thaw once Memory Used is this many points under the limit.
const THAW_GAP: u8 = 10;
/// Memory Used must stay over the limit this long (two Activity samples) before a freeze.
const OVER_FOR: Duration = Duration::from_secs(4);
/// After a freeze or thaw, wait this long before the next.
const SETTLE: Duration = Duration::from_secs(10);
/// A Session using less than this is not worth freezing: freezing it would not help.
const MIN_MEM: u64 = 128 * 1024 * 1024;
/// Passes over the process tree when freezing, for children forked while it was being stopped.
const FREEZE_PASSES: usize = 3;

/// A stopped process: pid and start time (`ri_proc_start_abstime`), to tell a recycled pid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Proc {
    pid: i32,
    start: u64,
}

#[derive(Debug)]
struct Frozen {
    session_id: SessionId,
    mem: u64,
    frozen_at: u64,
    /// Parents first: the order they were stopped in.
    procs: Vec<Proc>,
}

#[derive(Debug, PartialEq)]
enum Step {
    Freeze(SessionId),
    Thaw(SessionId),
    Wait,
}

#[derive(Debug)]
struct Inner {
    on: bool,
    limit_percent: u8,
    /// The Session whose Tab is in view: never frozen.
    visible: Option<SessionId>,
    /// Oldest first.
    frozen: Vec<Frozen>,
    /// Thawed because the user went to the Tab: not frozen again until memory falls under the
    /// thaw line.
    spared: HashSet<SessionId>,
    /// Since when Memory Used has been over the limit, if it is.
    over_since: Option<Instant>,
    /// Last freeze or thaw.
    last_step: Option<Instant>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            on: false,
            limit_percent: DEFAULT_LIMIT,
            visible: None,
            frozen: Vec::new(),
            spared: HashSet::new(),
            over_since: None,
            last_step: None,
        }
    }
}

impl Inner {
    /// What to do about this sample. Only moves the clocks and the spared set; the caller acts.
    fn decide(&mut self, snap: &ActivitySnapshot, now: Instant) -> Step {
        if !self.on || snap.mem_total == 0 {
            return Step::Wait;
        }
        let percent = |p: u8| snap.mem_total / 100 * u64::from(p);
        let limit = percent(self.limit_percent);
        let thaw_line = percent(self.limit_percent.saturating_sub(THAW_GAP));

        if snap.mem_used > limit {
            self.over_since.get_or_insert(now);
        } else {
            self.over_since = None;
        }
        if snap.mem_used < thaw_line {
            self.spared.clear();
        }
        if self
            .last_step
            .is_some_and(|at| now.duration_since(at) < SETTLE)
        {
            return Step::Wait;
        }

        if self
            .over_since
            .is_some_and(|at| now.duration_since(at) >= OVER_FOR)
        {
            let frozen: HashSet<SessionId> = self.frozen.iter().map(|f| f.session_id).collect();
            return snap
                .sessions
                .iter()
                .filter(|s| {
                    s.mem >= MIN_MEM
                        && Some(s.session_id) != self.visible
                        && !frozen.contains(&s.session_id)
                        && !self.spared.contains(&s.session_id)
                })
                .max_by_key(|s| (s.mem, std::cmp::Reverse(s.session_id)))
                .map_or(Step::Wait, |s| Step::Freeze(s.session_id));
        }
        match self.frozen.first() {
            Some(f) if snap.mem_used < thaw_line => Step::Thaw(f.session_id),
            _ => Step::Wait,
        }
    }

    fn snapshot(&self) -> GuardSnapshot {
        GuardSnapshot {
            on: self.on,
            limit_percent: self.limit_percent,
            frozen: self
                .frozen
                .iter()
                .map(|f| FrozenSession {
                    session_id: f.session_id,
                    mem: f.mem,
                    frozen_at: f.frozen_at,
                })
                .collect(),
        }
    }

    /// Thaw one Session and forget it. False if it was not frozen.
    fn thaw(&mut self, id: SessionId) -> bool {
        let Some(i) = self.frozen.iter().position(|f| f.session_id == id) else {
            return false;
        };
        thaw_procs(&self.frozen.remove(i).procs);
        true
    }

    fn thaw_all(&mut self) -> bool {
        let any = !self.frozen.is_empty();
        for f in self.frozen.drain(..) {
            thaw_procs(&f.procs);
        }
        any
    }
}

type OnChange = Box<dyn Fn(GuardSnapshot) + Send + Sync>;

/// Memory Guard (Tauri state). Starts off; the webview turns it on from Settings at startup.
pub struct Guard {
    inner: Mutex<Inner>,
    /// `frozen.json`: what is frozen, for the next launch to thaw after a crash.
    record: Option<PathBuf>,
    on_change: OnChange,
}

impl Guard {
    /// Thaw whatever a crashed run left frozen (per `record`), then start off. `on_change` gets
    /// every change of state.
    pub fn open(
        record: Option<PathBuf>,
        on_change: impl Fn(GuardSnapshot) + Send + Sync + 'static,
    ) -> Self {
        if let Some(path) = &record {
            if let Ok(body) = fs::read_to_string(path) {
                let procs: Vec<Proc> = serde_json::from_str(&body).unwrap_or_default();
                if !procs.is_empty() {
                    eprintln!(
                        "memory guard: thawing {} processes a crashed run left frozen",
                        procs.len()
                    );
                }
                thaw_procs(&procs);
            }
            let _ = fs::remove_file(path);
        }
        Self {
            inner: Mutex::default(),
            record,
            on_change: Box::new(on_change),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn snapshot(&self) -> GuardSnapshot {
        self.lock().snapshot()
    }

    /// Turn on or off and set the limit (clamped to 50..=95%). Off thaws everything.
    pub fn set(&self, on: bool, limit_percent: u8) -> GuardSnapshot {
        let mut inner = self.lock();
        inner.on = on;
        inner.limit_percent = limit_percent.clamp(*LIMITS.start(), *LIMITS.end());
        if !on {
            inner.thaw_all();
            inner.spared.clear();
            inner.over_since = None;
            inner.last_step = None;
        }
        self.changed(inner)
    }

    /// The Session whose Tab is in view. Going to a frozen Tab thaws and spares it.
    pub fn set_visible(&self, id: Option<SessionId>) {
        let mut inner = self.lock();
        inner.visible = id;
        if let Some(id) = id {
            if inner.thaw(id) {
                inner.spared.insert(id);
                inner.last_step = Some(Instant::now());
                self.changed(inner);
            }
        }
    }

    /// Thaw a Session about to be killed, so the hangup reaches every process.
    pub fn release(&self, id: SessionId) {
        let mut inner = self.lock();
        if inner.thaw(id) {
            self.changed(inner);
        }
    }

    /// Thaw every Session: before killing them all (quit, webview reload).
    pub fn release_all(&self) {
        let mut inner = self.lock();
        if inner.thaw_all() {
            self.changed(inner);
        }
    }

    /// One Activity sample: forget Sessions that are gone, then freeze or thaw as `decide` says.
    pub fn observe(&self, snap: &ActivitySnapshot, targets: &[ProbeTarget]) {
        let mut inner = self.lock();
        let live: HashSet<SessionId> = targets.iter().map(|t| t.session_id).collect();
        let gone: Vec<SessionId> = inner
            .frozen
            .iter()
            .map(|f| f.session_id)
            .filter(|id| !live.contains(id))
            .collect();
        let mut changed = !gone.is_empty();
        for id in gone {
            inner.thaw(id); // its shell is gone; continue whatever it left behind
        }
        inner.spared.retain(|id| live.contains(id));

        let now = Instant::now();
        match inner.decide(snap, now) {
            Step::Freeze(id) => {
                let target = targets.iter().find(|t| t.session_id == id);
                let procs = target.map(|t| freeze_tree(t.shell_pid)).unwrap_or_default();
                if !procs.is_empty() {
                    let mem = snap
                        .sessions
                        .iter()
                        .find(|s| s.session_id == id)
                        .map_or(0, |s| s.mem);
                    inner.frozen.push(Frozen {
                        session_id: id,
                        mem,
                        frozen_at: now_ms(),
                        procs,
                    });
                    changed = true;
                }
                inner.over_since = None; // the freeze needs its own OVER_FOR to be judged
                inner.last_step = Some(now);
            }
            Step::Thaw(id) => {
                changed |= inner.thaw(id);
                inner.last_step = Some(now);
            }
            Step::Wait => {}
        }
        if changed {
            self.changed(inner);
        }
    }

    /// Save the record and tell the webview. Emits after unlocking.
    fn changed(&self, inner: MutexGuard<'_, Inner>) -> GuardSnapshot {
        let snapshot = inner.snapshot();
        let procs: Vec<Proc> = inner
            .frozen
            .iter()
            .flat_map(|f| f.procs.iter().copied())
            .collect();
        drop(inner);
        if let Some(path) = &self.record {
            let saved = if procs.is_empty() {
                fs::remove_file(path).or_else(|e| match e.kind() {
                    std::io::ErrorKind::NotFound => Ok(()),
                    _ => Err(e.to_string()),
                })
            } else {
                layout::write(path, &procs)
            };
            if let Err(e) = saved {
                eprintln!("memory guard: could not save {}: {e}", path.display());
            }
        }
        (self.on_change)(snapshot.clone());
        snapshot
    }
}

/// `root` and every process descending from it, parents before children.
fn descendants(rows: &[PsRow], root: i32) -> Vec<i32> {
    let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
    for r in rows.iter().filter(|r| r.pid != r.ppid) {
        children.entry(r.ppid).or_default().push(r.pid);
    }
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([root]);
    while let Some(pid) = queue.pop_front() {
        if seen.insert(pid) {
            order.push(pid);
            queue.extend(children.get(&pid).into_iter().flatten());
        }
    }
    order
}

/// SIGSTOP `root` and its descendants, parents first, re-reading the tree for children forked
/// meanwhile. Returns the stopped processes; empty if `root` could not be stopped.
fn freeze_tree(root: i32) -> Vec<Proc> {
    let mut stopped: Vec<Proc> = Vec::new();
    let mut tried = HashSet::new();
    for _ in 0..FREEZE_PASSES {
        let Some(rows) = run_ps() else { break };
        let mut stopped_any = false;
        for pid in descendants(&rows, root) {
            if !tried.insert(pid) {
                continue;
            }
            // Only this user's processes have a readable start time, and only they can be stopped.
            let Some((_, start)) = rusage(pid) else {
                continue;
            };
            // SAFETY: plain syscall on a pid just read, of this user.
            if unsafe { libc::kill(pid, libc::SIGSTOP) } == 0 {
                stopped.push(Proc { pid, start });
                stopped_any = true;
            }
        }
        if !stopped_any {
            break;
        }
    }
    if stopped.first().map(|p| p.pid) != Some(root) {
        // The shell is gone or was not ours: undo, nothing is frozen.
        thaw_procs(&stopped);
        return Vec::new();
    }
    stopped
}

/// SIGCONT `procs` children first, skipping any whose pid now names another process.
fn thaw_procs(procs: &[Proc]) {
    for p in procs.iter().rev() {
        if rusage(p.pid).is_some_and(|(_, start)| start == p.start) {
            // SAFETY: plain syscall; the start time says it is the process we stopped.
            unsafe { libc::kill(p.pid, libc::SIGCONT) };
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ActivitySession;
    use std::process::{Child, Command};
    use std::sync::{Arc, Mutex as StdMutex};
    use std::thread;

    const GB: u64 = 1 << 30;

    fn snap(used_gb: f64, sessions: &[(SessionId, u64)]) -> ActivitySnapshot {
        ActivitySnapshot {
            cpu_count: 8,
            cpu_total: 0.0,
            mem_used: (used_gb * GB as f64) as u64,
            mem_wired: 0,
            mem_compressed: 0,
            mem_total: 10 * GB,
            sessions: sessions
                .iter()
                .map(|&(session_id, mem)| ActivitySession {
                    session_id,
                    cpu: 0.0,
                    mem,
                    processes: 1,
                })
                .collect(),
            processes: Vec::new(),
        }
    }

    fn on(limit_percent: u8) -> Inner {
        Inner {
            on: true,
            limit_percent,
            ..Inner::default()
        }
    }

    fn frozen(session_id: SessionId) -> Frozen {
        Frozen {
            session_id,
            mem: GB,
            frozen_at: 0,
            procs: Vec::new(),
        }
    }

    #[test]
    fn freezes_the_heaviest_tab_once_over_the_limit_for_a_while() {
        let mut g = on(80);
        g.visible = Some(1);
        let t0 = Instant::now();
        let s = snap(8.5, &[(1, 3 * GB), (2, GB), (3, 2 * GB), (4, MIN_MEM - 1)]);
        assert_eq!(g.decide(&s, t0), Step::Wait, "not over for OVER_FOR yet");
        // 1 is in view, so 3 is the heaviest candidate.
        assert_eq!(g.decide(&s, t0 + OVER_FOR), Step::Freeze(3));

        g.frozen.push(frozen(3));
        g.spared.insert(2);
        assert_eq!(
            g.decide(&s, t0 + OVER_FOR),
            Step::Wait,
            "only tiny, visible, frozen or spared left"
        );
    }

    #[test]
    fn dipping_under_the_limit_restarts_the_clock() {
        let mut g = on(80);
        let t0 = Instant::now();
        let heavy = [(1, 2 * GB)];
        assert_eq!(g.decide(&snap(8.5, &heavy), t0), Step::Wait);
        assert_eq!(g.decide(&snap(7.5, &heavy), t0 + OVER_FOR / 2), Step::Wait);
        assert_eq!(g.decide(&snap(8.5, &heavy), t0 + OVER_FOR), Step::Wait);
        assert_eq!(
            g.decide(&snap(8.5, &heavy), t0 + OVER_FOR * 2),
            Step::Freeze(1)
        );
    }

    #[test]
    fn thaws_the_oldest_once_under_the_thaw_line() {
        let mut g = on(80);
        g.frozen = vec![frozen(5), frozen(6)];
        let t0 = Instant::now();
        assert_eq!(
            g.decide(&snap(7.5, &[]), t0),
            Step::Wait,
            "between the lines: hold"
        );
        assert_eq!(g.decide(&snap(6.9, &[]), t0), Step::Thaw(5));
    }

    #[test]
    fn waits_for_memory_to_settle_after_a_step() {
        let mut g = on(80);
        g.frozen = vec![frozen(5)];
        let t0 = Instant::now();
        g.last_step = Some(t0);
        assert_eq!(g.decide(&snap(5.0, &[]), t0 + SETTLE / 2), Step::Wait);
        assert_eq!(g.decide(&snap(5.0, &[]), t0 + SETTLE), Step::Thaw(5));
    }

    #[test]
    fn spared_tabs_are_forgiven_under_the_thaw_line() {
        let mut g = on(80);
        g.spared.insert(1);
        let t0 = Instant::now();
        g.decide(&snap(7.5, &[(1, 2 * GB)]), t0);
        assert!(g.spared.contains(&1), "still between the lines");
        g.decide(&snap(6.0, &[(1, 2 * GB)]), t0);
        assert!(g.spared.is_empty());
    }

    #[test]
    fn off_never_acts() {
        let mut g = Inner::default();
        g.frozen.push(frozen(1));
        let s = snap(9.9, &[(2, 5 * GB)]);
        assert_eq!(g.decide(&s, Instant::now()), Step::Wait);
        assert_eq!(g.decide(&s, Instant::now() + OVER_FOR * 10), Step::Wait);
    }

    #[test]
    fn descendants_come_parents_first() {
        let row = |pid, ppid| PsRow::test(pid, ppid, "p");
        let rows = [
            row(10, 1),
            row(12, 11),
            row(11, 10),
            row(13, 10),
            row(20, 1),
            row(0, 0),
        ];
        assert_eq!(descendants(&rows, 10), [10, 11, 13, 12]);
        assert_eq!(descendants(&rows, 99), [99]);
    }

    /// `ps` state letter of `pid`: `T` when stopped.
    fn state(pid: u32) -> String {
        let out = Command::new("/bin/ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .chars()
            .next()
            .map(String::from)
            .unwrap_or_default()
    }

    fn children_of(pid: u32) -> Vec<u32> {
        run_ps()
            .unwrap()
            .iter()
            .filter(|r| r.ppid == pid as i32)
            .map(|r| r.pid as u32)
            .collect()
    }

    struct Reap(Child);
    impl Drop for Reap {
        fn drop(&mut self) {
            // SAFETY: continue then kill our own test child's group.
            unsafe {
                libc::kill(self.0.id() as i32, libc::SIGCONT);
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn freezes_and_thaws_a_real_process_tree() {
        // sh -> sleep: the sh waits on its child, like a shell on its foreground job.
        let sh = Reap(
            Command::new("/bin/sh")
                .args(["-c", "/bin/sleep 30 & wait"])
                .spawn()
                .unwrap(),
        );
        let pid = sh.0.id();
        let deadline = Instant::now() + Duration::from_secs(5);
        let sleep = loop {
            if let Some(&c) = children_of(pid).first() {
                break c;
            }
            assert!(Instant::now() < deadline, "sleep never started");
            thread::sleep(Duration::from_millis(20));
        };

        let procs = freeze_tree(pid as i32);
        assert_eq!(
            procs.iter().map(|p| p.pid as u32).collect::<Vec<_>>(),
            [pid, sleep]
        );
        assert_eq!((state(pid), state(sleep)), ("T".into(), "T".into()));

        thaw_procs(&procs);
        assert_ne!(state(pid), "T");
        assert_ne!(state(sleep), "T");

        // A stale record (a recycled pid, per its start time) is left alone.
        freeze_tree(sleep as i32);
        thaw_procs(&[Proc {
            pid: sleep as i32,
            start: 1,
        }]);
        assert_eq!(state(sleep), "T");
        thaw_procs(&[Proc {
            pid: sleep as i32,
            start: rusage(sleep as i32).unwrap().1,
        }]);
        assert_ne!(state(sleep), "T");
        // SAFETY: our test child.
        unsafe { libc::kill(sleep as i32, libc::SIGKILL) };
    }

    #[test]
    fn a_crashed_runs_record_is_thawed_on_open() {
        let dir = crate::detect::testutil::TempDir::new("guard");
        let path = dir.path().join("frozen.json");
        let child = Reap(Command::new("/bin/sleep").arg("30").spawn().unwrap());
        let pid = child.0.id() as i32;
        let procs = freeze_tree(pid);
        assert_eq!(state(pid as u32), "T");
        layout::write(&path, &procs).unwrap();

        let events = Arc::new(StdMutex::new(0));
        let counted = Arc::clone(&events);
        let guard = Guard::open(Some(path.clone()), move |_| *counted.lock().unwrap() += 1);
        assert_ne!(state(pid as u32), "T");
        assert!(!path.exists());
        assert!(!guard.snapshot().on);
        assert_eq!(*events.lock().unwrap(), 0);
    }

    #[test]
    fn turning_off_thaws_and_limits_are_clamped() {
        let guard = Guard::open(None, |_| {});
        assert_eq!(guard.set(true, 99).limit_percent, 95);
        assert_eq!(guard.set(true, 10).limit_percent, 50);

        let child = Reap(Command::new("/bin/sleep").arg("30").spawn().unwrap());
        let pid = child.0.id() as i32;
        guard.lock().frozen.push(Frozen {
            session_id: 7,
            mem: GB,
            frozen_at: 0,
            procs: freeze_tree(pid),
        });
        assert_eq!(state(pid as u32), "T");
        let snap = guard.set(false, 80);
        assert!(snap.frozen.is_empty() && !snap.on);
        assert_ne!(state(pid as u32), "T");
    }
}
