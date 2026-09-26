//! Suites: test suites recognised under a Session, with their progress and ETA, for the Tab row
//! and the Panel's Activity view. See docs/architecture.md "Suites" and
//! docs/research/test-progress.md.
//!
//! A thread ticks every [`TICK`]: for each Session it walks the shell's descendants with
//! libproc (`process::children`, recursive: Claude Code's Bash tool runs commands under a
//! tty-less `zsh -c`, so nothing it runs is in the pty's Foreground process group), classifies
//! each process (`detect::runner`), and turns the outermost driver of each run into a Suite. A
//! Suite starts when its runner is first seen and ends when that pid is gone; it lingers
//! [`LINGER`] showing the result, then goes.
//!
//! Progress comes from the progress files the app's reporters append to the Session's directory
//! (`$SIDEBAR_TERM_PROGRESS_DIR`, set at spawn: [`Suites::inject`]), matched to a Suite by the
//! writer's pid being the runner or one of its descendants (see `progress.rs`). Without a
//! stream, the duration history of earlier runs of the same suite gives an ETA (`history.rs`).
//! A Session frozen by Memory Guard stops the clock, and a run frozen at any point is not
//! recorded in the history.

pub mod env;
mod history;
mod progress;

use crate::detect::runner::{self, Role, Runner, RunnerProcess};
use crate::detect::{git, process};
use crate::model::{
    ProbeTarget, SessionId, SuiteOutcome, SuitePhase, SuiteSnapshot, SuiteSource, EVENT_SUITE,
};
use history::{Estimate, History, Key, Run};
use progress::{merge, FileProgress, Progress, Tail};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

/// Time between ticks: the monitor's cadence, so a run shows within half a second.
const TICK: Duration = Duration::from_millis(500);
/// How long a finished Suite stays on the Tab showing its result.
const LINGER: Duration = Duration::from_secs(5);
/// Upper bound on processes read under one Session per tick.
const MAX_TREE: usize = 4096;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

// ---------------------------------------------------------------------------------------------
// Tauri state: per-Session progress directories and the environment to spawn with
// ---------------------------------------------------------------------------------------------

/// Tauri state: hands each new Session its progress directory and environment, and owns the
/// tracking thread's shared knowledge of which directory is which Session's.
pub struct Suites {
    shared: Arc<Mutex<Shared>>,
}

struct Shared {
    /// This app run's progress root: `<temp>/sidebar-term/progress/<app pid>`.
    root: PathBuf,
    /// The shipped reporters and wrappers, if found.
    reporters: Option<PathBuf>,
    triple: String,
    next_token: u64,
    /// Reserved directories of Sessions being spawned.
    pending: HashMap<u64, PathBuf>,
    /// Each live Session's progress directory.
    dirs: HashMap<SessionId, PathBuf>,
}

/// A progress directory reserved for a Session about to be spawned.
pub struct Reservation {
    token: u64,
    dir: Option<PathBuf>,
}

impl Suites {
    /// Reserve a progress directory for a Session about to spawn; `inject: false` (the "Suite
    /// progress" setting off) reserves nothing, and the Session gets no variables.
    pub fn reserve(&self, inject: bool) -> Reservation {
        if !inject {
            return Reservation {
                token: 0,
                dir: None,
            };
        }
        let mut s = lock(&self.shared);
        s.next_token += 1;
        let token = s.next_token;
        let dir = s.root.join(token.to_string());
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!("suites: cannot create {}: {e}", dir.display());
            return Reservation {
                token: 0,
                dir: None,
            };
        }
        s.pending.insert(token, dir.clone());
        Reservation {
            token,
            dir: Some(dir),
        }
    }

    /// Add the progress and reporter variables (`env::inject`) to a Session's environment.
    pub fn inject(&self, env: &mut std::collections::BTreeMap<OsString, OsString>, r: &Reservation) {
        let Some(dir) = &r.dir else {
            return;
        };
        let s = lock(&self.shared);
        env::inject(env, dir, s.reporters.as_deref(), &s.triple);
    }

    /// The Session spawned: its directory is now tracked under its id.
    pub fn bind(&self, r: &Reservation, session_id: SessionId) {
        let mut s = lock(&self.shared);
        if let Some(dir) = s.pending.remove(&r.token) {
            s.dirs.insert(session_id, dir);
        }
    }

    /// The spawn failed: drop the reservation.
    pub fn unbind(&self, r: &Reservation) {
        let mut s = lock(&self.shared);
        if let Some(dir) = s.pending.remove(&r.token) {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

/// The reporters directory to inject: the shipped one, unless its path has whitespace (the
/// values of `MOCHA_OPTIONS`, `GOFLAGS` and cargo's runner are split on it), in which case a
/// copy under `temp_root`. `None` when the shipped directory is missing.
pub fn reporters_dir(shipped: Option<PathBuf>, temp_root: &Path) -> Option<PathBuf> {
    let shipped = shipped.filter(|p| p.is_dir())?;
    if !shipped.to_string_lossy().contains(char::is_whitespace) {
        return Some(shipped);
    }
    let copy = temp_root.join("reporters");
    let _ = fs::remove_dir_all(&copy);
    fs::create_dir_all(&copy).ok()?;
    for entry in fs::read_dir(&shipped).ok()?.flatten() {
        let from = entry.path();
        if from.is_file() {
            fs::copy(&from, copy.join(entry.file_name())).ok()?;
        }
    }
    Some(copy)
}

/// Start the tracking thread. `history_path` is `history.json` in the app data dir; `reporters`
/// the directory of shipped reporters; `targets` lists live Sessions (as for the monitor);
/// `frozen` lists the Sessions Memory Guard has frozen. Emits `suite` on change each tick.
pub fn spawn<F, G>(
    app: AppHandle,
    reporters: Option<PathBuf>,
    history_path: Option<PathBuf>,
    targets: F,
    frozen: G,
) -> Suites
where
    F: Fn() -> Vec<ProbeTarget> + Send + 'static,
    G: Fn() -> Vec<SessionId> + Send + 'static,
{
    let root = progress_root();
    let shared = Arc::new(Mutex::new(Shared {
        root,
        reporters,
        triple: env::host_triple(),
        next_token: 0,
        pending: HashMap::new(),
        dirs: HashMap::new(),
    }));
    let for_thread = Arc::clone(&shared);
    // `SIDEBAR_TERM_SUITE_LOG=1` prints every emission to stderr, to watch detection live.
    let log = std::env::var_os("SIDEBAR_TERM_SUITE_LOG").is_some_and(|v| !v.is_empty());
    let started = thread::Builder::new()
        .name("suites".into())
        .spawn(move || {
            let mut tracker = Tracker::new(History::open(history_path), LINGER);
            let mut last: Vec<SuiteSnapshot> = Vec::new();
            loop {
                thread::sleep(TICK);
                let Ok(live) = catch_unwind(AssertUnwindSafe(&targets)) else {
                    eprintln!("suites: listing sessions panicked; skipping this tick");
                    continue;
                };
                let frozen: HashSet<SessionId> =
                    catch_unwind(AssertUnwindSafe(&frozen)).unwrap_or_default().into_iter().collect();
                let dirs = {
                    let mut s = lock(&for_thread);
                    let gone: Vec<SessionId> = s
                        .dirs
                        .keys()
                        .filter(|id| !live.iter().any(|t| t.session_id == **id))
                        .copied()
                        .collect();
                    for id in gone {
                        if let Some(dir) = s.dirs.remove(&id) {
                            let _ = fs::remove_dir_all(dir);
                        }
                    }
                    s.dirs.clone()
                };
                let ticked = catch_unwind(AssertUnwindSafe(|| {
                    tracker.tick(&live, &frozen, &dirs, Instant::now(), epoch_ms())
                }));
                match ticked {
                    Ok(snapshots) => {
                        if snapshots != last {
                            if log {
                                eprintln!("suites: {}", serde_json::to_string(&snapshots).unwrap_or_default());
                            }
                            if let Err(e) = app.emit(EVENT_SUITE, &snapshots) {
                                eprintln!("suites: emit failed: {e}");
                            }
                            last = snapshots;
                        }
                    }
                    Err(_) => eprintln!("suites: tick panicked; skipping it"),
                }
            }
        });
    if let Err(e) = started {
        eprintln!("suites: could not start thread: {e}");
    }
    Suites { shared }
}

/// `<temp>/sidebar-term/progress/<app pid>`, created; the directories of app runs that are gone
/// (a crash left them) are removed.
fn progress_root() -> PathBuf {
    let base = std::env::temp_dir().join("sidebar-term").join("progress");
    let me = std::process::id();
    if let Ok(entries) = fs::read_dir(&base) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            // SAFETY: signal 0 checks for existence only.
            if pid != me && unsafe { libc::kill(pid as i32, 0) } != 0 {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
    let root = base.join(me.to_string());
    if let Err(e) = fs::create_dir_all(&root) {
        eprintln!("suites: cannot create {}: {e}", root.display());
    }
    root
}

// ---------------------------------------------------------------------------------------------
// The process tree
// ---------------------------------------------------------------------------------------------

/// One process under a Session's shell, as far as Suites care.
#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub pid: i32,
    pub ppid: i32,
    pub class: Option<RunnerProcess>,
    /// Read only for classified drivers (the history signature needs it); empty otherwise.
    pub argv: Vec<String>,
}

#[derive(Clone)]
struct Cached {
    comm: String,
    class: Option<RunnerProcess>,
    argv: Vec<String>,
}

/// Every live descendant of `shell_pid`, classified. `cache` spares the executable path and argv
/// reads for a pid seen before with the same `comm`.
fn walk(shell_pid: i32, cache: &mut HashMap<i32, Cached>) -> Vec<Node> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![shell_pid];
    while let Some(parent) = stack.pop() {
        for pid in process::children(parent) {
            if pid == shell_pid || !seen.insert(pid) || out.len() >= MAX_TREE {
                continue;
            }
            let Some(info) = process::short_info(pid) else {
                continue;
            };
            let cached = match cache.get(&pid) {
                Some(c) if c.comm == info.comm => c.clone(),
                _ => {
                    let path = process::exe_path(pid).unwrap_or_default();
                    let argv = if runner::needs_argv(&info.comm) {
                        process::argv(pid).unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    let class = runner::classify_runner(&info.comm, &path, &argv);
                    let c = Cached {
                        comm: info.comm.clone(),
                        class,
                        argv: if class.is_some_and(|c| c.role == Role::Driver) {
                            argv
                        } else {
                            Vec::new()
                        },
                    };
                    cache.insert(pid, c.clone());
                    c
                }
            };
            out.push(Node {
                pid,
                ppid: parent,
                class: cached.class,
                argv: cached.argv,
            });
            stack.push(pid);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Tracking
// ---------------------------------------------------------------------------------------------

#[derive(Debug)]
struct Suite {
    id: u64,
    session_id: SessionId,
    /// The outermost driver's pid (or the test binary's, run by hand).
    root_pid: i32,
    runner: Runner,
    key: Key,
    estimate: Option<Estimate>,
    started_at: u64,
    /// Time run, not counting time frozen.
    elapsed: Duration,
    last_tick: Instant,
    frozen_during: bool,
    phase: SuitePhase,
    /// `elapsed` when testing began, for the stream's rate.
    testing_since: Option<Duration>,
    ended_at: Option<Instant>,
    /// The numbers at the end, once the files are gone.
    final_progress: Option<Progress>,
}

#[derive(Debug, Default)]
struct FileState {
    tail: Tail,
    progress: FileProgress,
    suite: Option<u64>,
}

/// Turns each tick's process trees and progress files into snapshots.
pub(crate) struct Tracker {
    history: History,
    linger: Duration,
    next_id: u64,
    suites: Vec<Suite>,
    files: HashMap<PathBuf, FileState>,
    cache: HashMap<i32, Cached>,
}

impl Tracker {
    fn new(history: History, linger: Duration) -> Self {
        Self {
            history,
            linger,
            next_id: 0,
            suites: Vec::new(),
            files: HashMap::new(),
            cache: HashMap::new(),
        }
    }

    /// One tick over every live Session. `dirs` maps a Session to its progress directory (a
    /// Session spawned without injection has none).
    fn tick(
        &mut self,
        targets: &[ProbeTarget],
        frozen: &HashSet<SessionId>,
        dirs: &HashMap<SessionId, PathBuf>,
        now: Instant,
        epoch: u64,
    ) -> Vec<SuiteSnapshot> {
        // Sessions that are gone take their Suites with them at once.
        self.suites
            .retain(|s| targets.iter().any(|t| t.session_id == s.session_id));
        let mut seen = HashSet::new();
        for t in targets {
            let nodes = walk(t.shell_pid, &mut self.cache);
            seen.extend(nodes.iter().map(|n| n.pid));
            self.observe(
                t.session_id,
                t.shell_pid,
                &nodes,
                dirs.get(&t.session_id).map(PathBuf::as_path),
                frozen.contains(&t.session_id),
                now,
                epoch,
            );
        }
        self.cache.retain(|pid, _| seen.contains(pid));
        self.finish(now)
    }

    /// One Session's tick: `nodes` is its process tree (`walk`), `dir` its progress directory.
    #[allow(clippy::too_many_arguments)]
    fn observe(
        &mut self,
        session_id: SessionId,
        shell_pid: i32,
        nodes: &[Node],
        dir: Option<&Path>,
        frozen: bool,
        now: Instant,
        epoch: u64,
    ) {
        let by_pid: HashMap<i32, &Node> = nodes.iter().map(|n| (n.pid, n)).collect();
        let is_driver = |pid: i32| by_pid.get(&pid).is_some_and(|n| n.class.is_some_and(|c| c.role == Role::Driver));
        // The nearest driver above `pid`, if any.
        let driver_above = |pid: i32| -> Option<i32> {
            let mut p = by_pid.get(&pid)?.ppid;
            while p != shell_pid {
                if is_driver(p) {
                    return Some(p);
                }
                p = by_pid.get(&p)?.ppid;
            }
            None
        };
        // The Suite roots this tick: drivers with no driver above them, and test binaries run
        // by hand (no driver above them at all).
        let mut roots: Vec<(i32, Runner, Role, &[String])> = Vec::new();
        let mut binaries_under: HashSet<i32> = HashSet::new();
        for n in nodes {
            let Some(class) = n.class else {
                continue;
            };
            match (class.role, driver_above(n.pid)) {
                (Role::Driver, None) => roots.push((n.pid, class.runner, Role::Driver, &n.argv)),
                (Role::Driver, Some(_)) => {}
                (Role::TestBinary, Some(driver)) => {
                    // Credit the outermost driver, which is the Suite.
                    let mut top = driver;
                    while let Some(above) = driver_above(top) {
                        top = above;
                    }
                    binaries_under.insert(top);
                }
                (Role::TestBinary, None) => {
                    roots.push((n.pid, class.runner, Role::TestBinary, &n.argv))
                }
            }
        }
        let root_pids: HashSet<i32> = roots.iter().map(|r| r.0).collect();
        // The Suite a process belongs to: the highest root among its ancestors and itself.
        let root_of = |pid: i32| -> Option<i32> {
            let mut found = None;
            let mut p = Some(pid);
            while let Some(cur) = p {
                if cur == shell_pid {
                    break;
                }
                if root_pids.contains(&cur) {
                    found = Some(cur);
                }
                p = by_pid.get(&cur).map(|n| n.ppid);
            }
            found
        };

        // Clocks: running Suites of this Session advance unless the Session is frozen.
        for s in self.suites.iter_mut().filter(|s| s.session_id == session_id && s.ended_at.is_none()) {
            if frozen {
                s.frozen_during = true;
            } else {
                s.elapsed += now.saturating_duration_since(s.last_tick);
            }
            s.last_tick = now;
        }

        // Roots: known Suites carry on (a build may have reached its tests); new ones start.
        for (pid, runner, role, argv) in &roots {
            let has_binary = binaries_under.contains(pid);
            if let Some(s) = self
                .suites
                .iter_mut()
                .find(|s| s.session_id == session_id && s.root_pid == *pid && s.ended_at.is_none())
            {
                if s.phase == SuitePhase::Building && has_binary {
                    s.phase = SuitePhase::Testing;
                    s.testing_since = Some(s.elapsed);
                }
                continue;
            }
            let cwd = process::cwd(*pid);
            let repo = cwd
                .as_deref()
                .and_then(|c| git::resolve(Path::new(c)))
                .map(|g| g.common_dir)
                .or(cwd)
                .unwrap_or_default();
            let key = Key {
                repo,
                runner: runner.label().to_owned(),
                signature: runner::signature(*runner, argv),
            };
            let building = *role == Role::Driver && runner.builds_first() && !has_binary;
            self.next_id += 1;
            self.suites.push(Suite {
                id: self.next_id,
                session_id,
                root_pid: *pid,
                runner: *runner,
                estimate: self.history.estimate(&key),
                key,
                started_at: epoch,
                elapsed: Duration::ZERO,
                last_tick: now,
                frozen_during: frozen,
                phase: if building { SuitePhase::Building } else { SuitePhase::Testing },
                testing_since: if building { None } else { Some(Duration::ZERO) },
                ended_at: None,
                final_progress: None,
            });
        }

        // Suites whose root is gone have ended.
        for s in self.suites.iter_mut().filter(|s| s.session_id == session_id && s.ended_at.is_none()) {
            if !root_pids.contains(&s.root_pid) {
                s.ended_at = Some(now);
            }
        }

        // Progress files: read what is new, and match new files to their Suite.
        let Some(dir) = dir else {
            return;
        };
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let state = self.files.entry(path.clone()).or_default();
            let lines = state.tail.read_new(&path);
            state
                .progress
                .apply_lines(lines.iter().map(String::as_str));
            if state.suite.is_none() {
                let writer: Option<i32> = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse().ok());
                let by_tree = writer.and_then(root_of).and_then(|root| {
                    self.suites
                        .iter()
                        .find(|s| s.session_id == session_id && s.root_pid == root && s.ended_at.is_none())
                        .map(|s| s.id)
                });
                // A writer already gone (a wrapper that only listed the tests) goes to the
                // Session's newest Suite, one that ended this very tick included.
                let fallback = || {
                    self.suites
                        .iter()
                        .filter(|s| s.session_id == session_id && s.ended_at.is_none_or(|at| at == now))
                        .max_by_key(|s| s.id)
                        .map(|s| s.id)
                };
                state.suite = by_tree.or_else(fallback);
            }
        }
    }

    /// After every Session's tick: close out Suites that ended, drop the ones that lingered
    /// long enough, and build the snapshots.
    fn finish(&mut self, now: Instant) -> Vec<SuiteSnapshot> {
        let live: HashSet<u64> = self.suites.iter().map(|s| s.id).collect();
        self.files.retain(|_, f| f.suite.is_none_or(|id| live.contains(&id)));

        for s in self.suites.iter_mut() {
            if s.ended_at.is_none() || s.final_progress.is_some() {
                continue;
            }
            let progress = merge(self.files.values().filter(|f| f.suite == Some(s.id)).map(|f| &f.progress));
            if !s.frozen_during {
                self.history.record(
                    &s.key,
                    Run {
                        started_at: s.started_at,
                        duration_ms: s.elapsed.as_millis() as u64,
                        outcome: progress.outcome_at_exit().map(Into::into),
                    },
                );
            }
            let mine: Vec<PathBuf> = self
                .files
                .iter()
                .filter(|(_, f)| f.suite == Some(s.id))
                .map(|(p, _)| p.clone())
                .collect();
            for p in mine {
                let _ = fs::remove_file(&p);
                self.files.remove(&p);
            }
            s.final_progress = Some(progress);
        }
        let linger = self.linger;
        self.suites
            .retain(|s| s.ended_at.is_none_or(|at| now.saturating_duration_since(at) < linger));

        let mut out: Vec<SuiteSnapshot> = self
            .suites
            .iter()
            .map(|s| {
                let progress = s.final_progress.clone().unwrap_or_else(|| {
                    merge(self.files.values().filter(|f| f.suite == Some(s.id)).map(|f| &f.progress))
                });
                snapshot(s, &progress)
            })
            .collect();
        out.sort_by_key(|s| (s.session_id, s.started_at, s.id));
        out
    }
}

fn snapshot(s: &Suite, p: &Progress) -> SuiteSnapshot {
    let ended = s.ended_at.is_some();
    let elapsed_ms = s.elapsed.as_millis() as u64;
    let source = if p.streaming {
        SuiteSource::Stream
    } else if s.estimate.is_some() {
        SuiteSource::History
    } else {
        SuiteSource::None
    };
    let eta_ms = if ended {
        None
    } else if let (true, Some(total), Some(since)) = (p.done > 0, p.total, s.testing_since) {
        let testing = s.elapsed.saturating_sub(since).as_millis() as u64;
        Some(testing * u64::from(total.saturating_sub(p.done)) / u64::from(p.done))
    } else {
        s.estimate.map(|e| e.median_ms.saturating_sub(elapsed_ms))
    };
    let outcome = if ended {
        p.outcome_at_exit()
    } else if p.failed > 0 || p.any_failed {
        Some(SuiteOutcome::Failed)
    } else {
        None
    };
    SuiteSnapshot {
        id: s.id,
        session_id: s.session_id,
        runner: s.runner.label().to_owned(),
        phase: if ended { SuitePhase::Done } else { s.phase },
        started_at: s.started_at,
        elapsed_ms,
        done: p.done,
        total: p.total,
        failed: p.failed,
        outcome,
        eta_ms,
        source,
        typical_ms: s.estimate.map(|e| e.median_ms),
        longest_ms: s.estimate.map(|e| e.longest_ms),
        runs: s.estimate.map_or(0, |e| e.runs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::testutil::{fake_binary, spawn_in_own_group, wait_until, TempDir, SLEEPER_ARGS};
    use std::io::Write;

    fn node(pid: i32, ppid: i32, class: Option<(Runner, Role)>, argv: &[&str]) -> Node {
        Node {
            pid,
            ppid,
            class: class.map(|(runner, role)| RunnerProcess { runner, role }),
            argv: argv.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn tracker() -> Tracker {
        Tracker::new(History::open(None), Duration::from_secs(5))
    }

    fn target(session_id: SessionId, shell_pid: i32) -> ProbeTarget {
        ProbeTarget {
            session_id,
            shell_pid,
            fg_pgid: None,
        }
    }

    const SHELL: i32 = 100;

    #[test]
    fn the_outermost_driver_is_the_suite_and_its_binary_ends_the_build() {
        let mut tr = tracker();
        let t0 = Instant::now();
        // claude -> zsh -c -> cargo test -> rustc (building)
        let building = [
            node(101, SHELL, None, &[]),
            node(102, 101, None, &[]),
            node(103, 102, Some((Runner::Cargo, Role::Driver)), &["cargo", "test"]),
            node(104, 103, None, &[]),
        ];
        tr.observe(1, SHELL, &building, None, false, t0, 1_000);
        let out = tr.finish(t0);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].phase, out[0].runner.as_str(), out[0].source), (SuitePhase::Building, "cargo", SuiteSource::None));
        assert_eq!(out[0].started_at, 1_000);

        // The test binary appears: testing. A nested driver (nextest under cargo) is not a Suite.
        let testing = [
            node(101, SHELL, None, &[]),
            node(102, 101, None, &[]),
            node(103, 102, Some((Runner::Cargo, Role::Driver)), &["cargo", "test"]),
            node(105, 103, Some((Runner::Nextest, Role::Driver)), &["cargo-nextest", "nextest", "run"]),
            node(106, 105, Some((Runner::Cargo, Role::TestBinary)), &[]),
        ];
        let t1 = t0 + Duration::from_secs(3);
        tr.observe(1, SHELL, &testing, None, false, t1, 4_000);
        let out = tr.finish(t1);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].phase, SuitePhase::Testing);
        assert_eq!(out[0].elapsed_ms, 3_000);
        assert_eq!(out[0].id, 1);

        // The driver exits: done, lingering; recorded in the history for next time.
        let t2 = t1 + Duration::from_secs(2);
        tr.observe(1, SHELL, &[node(101, SHELL, None, &[])], None, false, t2, 6_000);
        let out = tr.finish(t2);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].phase, out[0].elapsed_ms, out[0].eta_ms, out[0].outcome), (SuitePhase::Done, 5_000, None, None));
        let t3 = t2 + Duration::from_secs(6);
        tr.observe(1, SHELL, &[], None, false, t3, 12_000);
        assert!(tr.finish(t3).is_empty(), "gone after lingering");

        // The same command again: the history gives an estimate from one run.
        tr.observe(1, SHELL, &building, None, false, t3, 12_000);
        let out = tr.finish(t3);
        assert_eq!((out[0].source, out[0].typical_ms, out[0].longest_ms, out[0].runs), (SuiteSource::History, Some(5_000), Some(5_000), 1));
        assert_eq!(out[0].eta_ms, Some(5_000));
        let t4 = t3 + Duration::from_secs(7);
        tr.observe(1, SHELL, &building, None, false, t4, 19_000);
        assert_eq!(tr.finish(t4)[0].eta_ms, Some(0), "never negative");
    }

    #[test]
    fn a_frozen_session_stops_the_clock_and_is_not_recorded() {
        let mut tr = tracker();
        let t0 = Instant::now();
        let nodes = [node(101, SHELL, Some((Runner::Pytest, Role::Driver)), &["python", "-m", "pytest"])];
        tr.observe(1, SHELL, &nodes, None, false, t0, 0);
        tr.finish(t0);
        let t1 = t0 + Duration::from_secs(2);
        tr.observe(1, SHELL, &nodes, None, true, t1, 2_000);
        assert_eq!(tr.finish(t1)[0].elapsed_ms, 0, "frozen: no time passes");
        let t2 = t1 + Duration::from_secs(2);
        tr.observe(1, SHELL, &nodes, None, false, t2, 4_000);
        assert_eq!(tr.finish(t2)[0].elapsed_ms, 2_000);
        let t3 = t2 + Duration::from_secs(1);
        tr.observe(1, SHELL, &[], None, false, t3, 5_000);
        tr.finish(t3);
        let t4 = t3 + Duration::from_secs(6);
        tr.observe(1, SHELL, &nodes, None, false, t4, 11_000);
        assert_eq!(tr.finish(t4)[0].source, SuiteSource::None, "a frozen run leaves no history");
    }

    #[test]
    fn progress_files_stream_and_match_by_writer_pid() {
        let dir = TempDir::new("suite-files");
        let mut tr = tracker();
        let t0 = Instant::now();
        // Two Suites in one Session: an old pytest, and a newer vitest whose reporter (pid 202)
        // writes from inside the driver.
        let nodes = [
            node(101, SHELL, Some((Runner::Pytest, Role::Driver)), &["python", "-m", "pytest"]),
            node(201, SHELL, None, &[]),
            node(202, 201, Some((Runner::Vitest, Role::Driver)), &["node", "vitest", "run"]),
        ];
        tr.observe(1, SHELL, &nodes[..1], Some(dir.path()), false, t0, 0);
        tr.finish(t0);
        let t1 = t0 + Duration::from_secs(1);
        let mut f = fs::File::create(dir.path().join("202.jsonl")).unwrap();
        writeln!(f, r#"{{"ev":"start","runner":"vitest","pid":202}}"#).unwrap();
        writeln!(f, r#"{{"ev":"total","total":4}}"#).unwrap();
        writeln!(f, r#"{{"ev":"case","status":"passed"}}"#).unwrap();
        writeln!(f, "half a line").unwrap();
        fs::write(dir.path().join("notes.txt"), "ignored").unwrap();
        // A file whose writer is gone already: matched to the newest Suite.
        fs::write(dir.path().join("999.jsonl"), "{\"ev\":\"case\",\"status\":\"failed\"}\n").unwrap();
        tr.observe(1, SHELL, &nodes, Some(dir.path()), false, t1, 1_000);
        let out = tr.finish(t1);
        assert_eq!(out.len(), 2);
        let pytest = out.iter().find(|s| s.runner == "pytest").unwrap();
        let vitest = out.iter().find(|s| s.runner == "vitest").unwrap();
        assert_eq!((pytest.source, pytest.done), (SuiteSource::None, 0));
        assert_eq!((vitest.source, vitest.done, vitest.total, vitest.failed), (SuiteSource::Stream, 2, Some(4), 1));
        assert_eq!(vitest.outcome, Some(SuiteOutcome::Failed));
        assert_eq!(vitest.eta_ms, Some(0), "no testing time yet");

        // Two seconds and two more cases in: half done at a case per second.
        let t2 = t1 + Duration::from_secs(2);
        writeln!(f, r#"{{"ev":"case","status":"passed"}}"#).unwrap();
        tr.observe(1, SHELL, &nodes, Some(dir.path()), false, t2, 3_000);
        let vitest = tr.finish(t2).into_iter().find(|s| s.runner == "vitest").unwrap();
        // 2 s of testing for 3 cases, 1 left: 666 ms.
        assert_eq!((vitest.done, vitest.eta_ms), (3, Some(2_000 / 3)));

        // The driver exits: the files are read a last time, then removed; the result lingers.
        writeln!(f, r#"{{"ev":"case","status":"passed"}}"#).unwrap();
        writeln!(f, r#"{{"ev":"end","status":"failed","passed":3,"failed":1}}"#).unwrap();
        let t3 = t2 + Duration::from_secs(1);
        tr.observe(1, SHELL, &nodes[..1], Some(dir.path()), false, t3, 4_000);
        let vitest = tr.finish(t3).into_iter().find(|s| s.runner == "vitest").unwrap();
        // Three cases of 202's plus 999's; 202's `end` says one failed, 999's case is another.
        assert_eq!((vitest.phase, vitest.done, vitest.total, vitest.failed), (SuitePhase::Done, 4, Some(4), 2));
        assert_eq!(vitest.outcome, Some(SuiteOutcome::Failed));
        assert!(!dir.path().join("202.jsonl").exists());
        assert!(!dir.path().join("999.jsonl").exists());
        assert!(dir.path().join("notes.txt").exists());
        let t4 = t3 + Duration::from_secs(1);
        tr.observe(1, SHELL, &nodes[..1], Some(dir.path()), false, t4, 5_000);
        let vitest = tr.finish(t4).into_iter().find(|s| s.runner == "vitest").unwrap();
        assert_eq!(vitest.done, 4, "the numbers outlive the files");
    }

    #[test]
    fn a_closed_session_takes_its_suites_at_once() {
        let mut tr = tracker();
        let t0 = Instant::now();
        let nodes = [node(101, SHELL, Some((Runner::Go, Role::Driver)), &["go", "test"])];
        tr.observe(1, SHELL, &nodes, None, false, t0, 0);
        assert_eq!(tr.finish(t0).len(), 1);
        assert!(tr.tick(&[], &HashSet::new(), &HashMap::new(), t0, 0).is_empty());
    }

    #[test]
    fn real_tree_a_cargo_test_binary_run_by_hand() {
        // <tmp>/target/debug/deps/fake-... is what classify_runner calls a cargo test binary.
        let tmp = TempDir::new("suite-real");
        let deps = tmp.path().join("target/debug/deps");
        fs::create_dir_all(&deps).unwrap();
        let bin = fake_binary(&deps, "fake-9a1cc61f1c687a78");
        let job = spawn_in_own_group(&bin, &SLEEPER_ARGS, tmp.path());
        let me = std::process::id() as i32;
        let mut tr = tracker();
        let t = target(7, me);
        assert!(wait_until(|| {
            let out = tr.tick(&[t], &HashSet::new(), &HashMap::new(), Instant::now(), epoch_ms());
            out.iter().any(|s| s.session_id == 7 && s.runner == "cargo" && s.phase == SuitePhase::Testing)
        }));
        let start = Instant::now();
        for _ in 0..200 {
            tr.tick(&[t], &HashSet::new(), &HashMap::new(), Instant::now(), epoch_ms());
        }
        println!("suite tick: {:?} per Session", start.elapsed() / 200);
        drop(job);
        assert!(wait_until(|| {
            let out = tr.tick(&[t], &HashSet::new(), &HashMap::new(), Instant::now(), epoch_ms());
            out.iter().any(|s| s.session_id == 7 && s.phase == SuitePhase::Done)
        }));
    }
}
