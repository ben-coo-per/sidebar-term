//! Activity: CPU and memory of every process on the machine, each Session's processes attributed
//! to it, for the sidebar Panel's Activity view, the Tabs' stats and Memory Guard. Sampled only
//! while one of them needs it.
//!
//! The OS reads sit in one module per platform, `activity/macos.rs` (`ps`, `proc_pid_rusage`,
//! Mach host statistics) and `activity/linux.rs` (`/proc`), behind the same four functions:
//! [`processes`], [`footprint`], [`start_time`] and `memory`. Each module says what its numbers
//! mean and why. Everything else here (attribution, the CPU maths, what to send) is shared.
//!
//! CPU% is the change in accumulated CPU time between two samples over the wall time between them,
//! as Activity Monitor shows it (100 = one core). A process without a previous sample uses the
//! OS's own average instead (`ps`'s decaying %cpu on macOS, the lifetime average on Linux).
//!
//! Memory is what the process costs the machine: on macOS the physical footprint, Activity
//! Monitor's "Memory" column; on Linux the proportional set size plus swap. Both count compressed
//! or swapped pages, and neither counts a shared page in full for every process sharing it. Both
//! are readable for this user's processes (every Session's); other users' processes fall back to
//! resident size.
//!
//! A process belongs to a Session when it is the Session's shell or descends from it by ppid. A
//! daemon that detaches (reparents to launchd or init) leaves its Session.
//!
//! Sampling runs while the webview watches (the Panel's Activity view, Tab stats) or while Memory
//! Guard is on (`guard.rs`), which gets every sample; only the webview's watch emits `activity`.

use crate::detect::{agent_command, process::classify_agent};
use crate::host::Events;
use crate::model::{
    ActivityProcess, ActivitySession, ActivitySnapshot, ProbeTarget, SessionId, EVENT_ACTIVITY,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
#[path = "activity/macos.rs"]
mod os;
// Compiled into every test build so its fixture-text tests run on any OS.
#[cfg(any(target_os = "linux", test))]
#[path = "activity/linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
use linux as os;

/// The OS reads, one module per platform (see above):
/// - `processes()`: every process on the machine, or `None` if they could not be listed.
/// - `footprint(pid)`: a process's memory in bytes; `None` when it is gone or another user's.
/// - `start_time(pid)`: when the process started, in an OS unit that tells a recycled pid from
///   the process that had it before; `None` when it is gone (macOS: or another user's).
pub(crate) use os::{footprint, processes, start_time};

/// Time between samples while watched.
const TICK: Duration = Duration::from_secs(2);
/// A previous sample older than this (watching was paused) is too stale to diff against.
const STALE: Duration = Duration::from_secs(6);
/// How many of the busiest non-Session processes to send, by CPU and again by memory.
const TOP: usize = 40;

/// Who wants samples. Sampling runs while either does.
#[derive(Clone, Copy, Default)]
struct Watchers {
    /// The webview: samples are emitted as `activity`.
    webview: bool,
    /// Memory Guard: samples go to the `on_sample` callback only.
    guard: bool,
}

impl Watchers {
    fn any(self) -> bool {
        self.webview || self.guard
    }
}

/// Handle to the sampling thread (app state). Starts idle.
pub struct Activity {
    watchers: Arc<(Mutex<Watchers>, Condvar)>,
}

impl Activity {
    /// Start or stop emitting `activity` to the webview. Starting samples at once, then every `TICK`.
    pub fn watch(&self, on: bool) {
        self.update(|w| w.webview = on);
    }

    /// Keep sampling for Memory Guard (the `on_sample` callback), whether or not the webview watches.
    pub fn sample_for_guard(&self, on: bool) {
        self.update(|w| w.guard = on);
    }

    fn update(&self, change: impl FnOnce(&mut Watchers)) {
        let (lock, cvar) = &*self.watchers;
        change(&mut lock.lock().unwrap_or_else(PoisonError::into_inner));
        cvar.notify_all();
    }
}

/// Start the sampling thread, idle until watched. While watched, sample every `TICK`: emit the
/// `ActivitySnapshot` if the webview watches, and hand it to `on_sample` if Memory Guard does.
/// `targets` lists live Sessions, as for the monitor.
///
/// The thread never exits: an unreadable process list or a panic skips the tick.
pub fn spawn<F, S>(events: Arc<dyn Events>, targets: F, on_sample: S) -> Activity
where
    F: Fn() -> Vec<ProbeTarget> + Send + 'static,
    S: Fn(&ActivitySnapshot, &[ProbeTarget]) + Send + 'static,
{
    let watchers = Arc::new((Mutex::new(Watchers::default()), Condvar::new()));
    let shared = Arc::clone(&watchers);
    let started = thread::Builder::new()
        .name("activity".into())
        .spawn(move || {
            let (lock, cvar) = &*shared;
            let cpu_count = thread::available_parallelism().map_or(1, |n| n.get() as u32);
            let mut sampler = Sampler::default();
            loop {
                let who = *cvar
                    .wait_while(lock.lock().unwrap_or_else(PoisonError::into_inner), |w| {
                        !w.any()
                    })
                    .unwrap_or_else(PoisonError::into_inner);
                let sampled = catch_unwind(AssertUnwindSafe(|| {
                    let rows = processes()?;
                    let memory = os::memory();
                    let targets = targets();
                    let snapshot =
                        sampler.snapshot(&rows, Instant::now(), &targets, cpu_count, memory);
                    if who.guard {
                        on_sample(&snapshot, &targets);
                    }
                    Some(snapshot)
                }));
                match sampled {
                    Ok(Some(snapshot)) => {
                        if who.webview {
                            events.emit(EVENT_ACTIVITY, &snapshot);
                        }
                    }
                    Ok(None) => eprintln!("activity: could not list processes; skipping this tick"),
                    Err(_) => {
                        eprintln!("activity: sampling panicked; skipping this tick");
                        sampler = Sampler::default();
                    }
                }
                // Sleep a tick, waking early only to stop.
                drop(
                    cvar.wait_timeout_while(
                        lock.lock().unwrap_or_else(PoisonError::into_inner),
                        TICK,
                        |w| w.any(),
                    )
                    .unwrap_or_else(PoisonError::into_inner),
                );
            }
        });
    if let Err(e) = started {
        eprintln!("activity: could not start thread: {e}");
    }
    Activity { watchers }
}

/// One process, as the OS lists it: a `ps` row on macOS, a `/proc/<pid>` on Linux.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PsRow {
    pub pid: i32,
    pub ppid: i32,
    /// Resident size: the memory of last resort, when [`footprint`] cannot read the process.
    rss_kib: u64,
    /// Accumulated user + system CPU time in milliseconds.
    cpu_ms: u64,
    /// The OS's own CPU% (decaying on macOS, lifetime on Linux): used for a process without a
    /// previous sample.
    pcpu: f32,
    /// Executable path, or a bare name such as a login shell's `-zsh` or a Linux kernel thread's
    /// `[kworker/0:1]`.
    comm: String,
}

#[cfg(test)]
impl PsRow {
    pub(crate) fn test(pid: i32, ppid: i32, comm: &str) -> Self {
        Self {
            pid,
            ppid,
            rss_kib: 1024,
            cpu_ms: 0,
            pcpu: 1.5,
            comm: comm.into(),
        }
    }
}

/// What the Activity view calls a process: its executable's file name, or the agent's command
/// name for a coding agent (native Claude Code's file name is its version, `2.1.267`).
fn display_name(comm: &str) -> String {
    // A Linux kernel thread, `[kworker/0:1]`: the brackets say the slash is not a path's.
    if comm.starts_with('[') && comm.ends_with(']') {
        return comm.to_owned();
    }
    let comm = comm.strip_prefix('-').unwrap_or(comm);
    let file = comm.rsplit('/').next().unwrap_or(comm);
    match classify_agent::<&str>(file, comm, &[]) {
        Some(kind) => agent_command(kind).to_owned(),
        None => file.to_owned(),
    }
}

/// pid -> Session for every live process that is a Session's shell or descends from one.
fn attribute(rows: &[PsRow], targets: &[ProbeTarget]) -> HashMap<i32, SessionId> {
    let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
    for r in rows.iter().filter(|r| r.pid != r.ppid) {
        children.entry(r.ppid).or_default().push(r.pid);
    }
    let live: HashSet<i32> = rows.iter().map(|r| r.pid).collect();
    let mut owner = HashMap::new();
    for t in targets.iter().filter(|t| live.contains(&t.shell_pid)) {
        let mut stack = vec![t.shell_pid];
        while let Some(pid) = stack.pop() {
            if owner.contains_key(&pid) {
                continue;
            }
            owner.insert(pid, t.session_id);
            stack.extend(children.get(&pid).into_iter().flatten());
        }
    }
    owner
}

/// "Memory Used", two of its parts (macOS's; zero on Linux), and physical memory, in bytes.
/// Zero when unreadable.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Memory {
    used: u64,
    wired: u64,
    compressed: u64,
    total: u64,
}

/// Turns successive process samples into snapshots: keeps each process's CPU time to diff against.
#[derive(Default)]
struct Sampler {
    prev_cpu_ms: HashMap<i32, u64>,
    prev_at: Option<Instant>,
}

impl Sampler {
    fn snapshot(
        &mut self,
        rows: &[PsRow],
        at: Instant,
        targets: &[ProbeTarget],
        cpu_count: u32,
        memory: Memory,
    ) -> ActivitySnapshot {
        let elapsed_ms = self
            .prev_at
            .map(|prev| at.saturating_duration_since(prev))
            .filter(|d| *d <= STALE && !d.is_zero())
            .map(|d| d.as_secs_f64() * 1000.0);
        let owner = attribute(rows, targets);
        let processes = rows
            .iter()
            .map(|r| {
                let cpu = match (elapsed_ms, self.prev_cpu_ms.get(&r.pid)) {
                    (Some(ms), Some(&before)) => {
                        (r.cpu_ms.saturating_sub(before) as f64 / ms * 100.0) as f32
                    }
                    _ => r.pcpu,
                };
                ActivityProcess {
                    pid: r.pid,
                    name: display_name(&r.comm),
                    cpu,
                    mem: footprint(r.pid).unwrap_or(r.rss_kib * 1024),
                    session_id: owner.get(&r.pid).copied(),
                }
            })
            .collect();
        self.prev_cpu_ms = rows.iter().map(|r| (r.pid, r.cpu_ms)).collect();
        self.prev_at = Some(at);
        summarize(processes, cpu_count, memory)
    }
}

/// Totals, per-Session sums, and the processes worth sending: every Session process plus the top
/// `TOP` others by CPU and the top `TOP` others by memory.
fn summarize(processes: Vec<ActivityProcess>, cpu_count: u32, memory: Memory) -> ActivitySnapshot {
    let cpu_total = processes.iter().map(|p| p.cpu).sum();
    let mut sessions = BTreeMap::<SessionId, ActivitySession>::new();
    for p in &processes {
        let Some(session_id) = p.session_id else {
            continue;
        };
        let s = sessions.entry(session_id).or_insert(ActivitySession {
            session_id,
            cpu: 0.0,
            mem: 0,
            processes: 0,
        });
        s.cpu += p.cpu;
        s.mem += p.mem;
        s.processes += 1;
    }

    let (mut sent, mut others): (Vec<_>, Vec<_>) =
        processes.into_iter().partition(|p| p.session_id.is_some());
    others.sort_unstable_by(|a, b| b.mem.cmp(&a.mem));
    let by_mem: HashSet<i32> = others.iter().take(TOP).map(|p| p.pid).collect();
    others.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    sent.extend(
        others
            .into_iter()
            .enumerate()
            .filter(|(rank, p)| *rank < TOP || by_mem.contains(&p.pid))
            .map(|(_, p)| p),
    );

    ActivitySnapshot {
        cpu_count,
        cpu_total,
        mem_used: memory.used,
        mem_wired: memory.wired,
        mem_compressed: memory.compressed,
        mem_total: memory.total,
        sessions: sessions.into_values().collect(),
        processes: sent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(pid: i32, ppid: i32, cpu_ms: u64, comm: &str) -> PsRow {
        PsRow {
            cpu_ms,
            ..PsRow::test(pid, ppid, comm)
        }
    }

    fn target(session_id: SessionId, shell_pid: i32) -> ProbeTarget {
        ProbeTarget {
            session_id,
            shell_pid,
            fg_pgid: None,
        }
    }

    #[test]
    fn display_names() {
        let cases = [
            (
                "/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer",
                "WindowServer",
            ),
            (
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome Helper (Renderer)",
                "Google Chrome Helper (Renderer)",
            ),
            ("-zsh", "zsh"),
            ("/bin/zsh", "zsh"),
            ("kernel_task", "kernel_task"),
            ("/Users/u/.local/share/claude/versions/2.1.267", "claude"),
            (
                "/opt/homebrew/Caskroom/codex/0.120.0/codex-aarch64-apple-darwin",
                "codex",
            ),
            ("/opt/homebrew/bin/node", "node"),
        ];
        for (comm, want) in cases {
            assert_eq!(display_name(comm), want, "{comm}");
        }
    }

    #[test]
    fn linux_names() {
        let cases = [
            ("[kworker/u16:2]", "[kworker/u16:2]"),
            ("[rcu_preempt]", "[rcu_preempt]"),
            ("/usr/bin/bash", "bash"),
            ("/home/u/.local/share/claude/versions/2.1.267", "claude"),
            (
                "/usr/lib/node_modules/@openai/codex/bin/codex-x86_64-unknown-linux-musl",
                "codex-x86_64-unknown-linux-musl",
            ),
            ("systemd-journal", "systemd-journal"),
        ];
        for (comm, want) in cases {
            assert_eq!(display_name(comm), want, "{comm}");
        }
    }

    #[test]
    fn attributes_descendants_to_their_session() {
        // Session 1: shell 100 -> cargo 101 -> rustc 102, 103. Session 2: shell 200 -> vim 201.
        // 300 detached from session 1 (reparented to launchd). 0 is its own parent.
        let rows = [
            row(0, 0, 0, "kernel_task"),
            row(1, 0, 0, "launchd"),
            row(100, 1, 0, "-zsh"),
            row(101, 100, 0, "cargo"),
            row(102, 101, 0, "rustc"),
            row(103, 101, 0, "rustc"),
            row(200, 1, 0, "-zsh"),
            row(201, 200, 0, "vim"),
            row(300, 1, 0, "node"),
        ];
        let owner = attribute(&rows, &[target(1, 100), target(2, 200), target(3, 999)]);
        let mut got: Vec<_> = owner.into_iter().collect();
        got.sort();
        assert_eq!(
            got,
            [(100, 1), (101, 1), (102, 1), (103, 1), (200, 2), (201, 2)]
        );
    }

    #[test]
    fn attribution_survives_a_ppid_cycle() {
        let rows = [
            row(100, 1, 0, "zsh"),
            row(101, 102, 0, "a"),
            row(102, 101, 0, "b"),
            row(103, 100, 0, "c"),
        ];
        let owner = attribute(&rows, &[target(1, 100)]);
        assert_eq!(owner.len(), 2);
    }

    #[test]
    fn cpu_is_the_time_delta_over_wall_time() {
        let mut s = Sampler::default();
        let t0 = Instant::now();
        let targets = [target(1, 100)];
        let mem = Memory {
            used: 8,
            total: 16,
            ..Memory::default()
        };

        // First sample: no history, so ps's own %cpu.
        let first = s.snapshot(&[row(100, 1, 1_000, "zsh")], t0, &targets, 8, mem);
        assert_eq!(first.processes[0].cpu, 1.5);

        // 2 s later: 3 s of CPU time -> 150%. A new process has no history yet.
        let rows = [row(100, 1, 4_000, "zsh"), row(101, 100, 50, "cargo")];
        let second = s.snapshot(&rows, t0 + Duration::from_secs(2), &targets, 8, mem);
        let cpu: HashMap<i32, f32> = second.processes.iter().map(|p| (p.pid, p.cpu)).collect();
        assert!((cpu[&100] - 150.0).abs() < 0.01, "{cpu:?}");
        assert_eq!(cpu[&101], 1.5);
        assert_eq!(second.sessions.len(), 1);
        assert!((second.sessions[0].cpu - 151.5).abs() < 0.01);
        assert_eq!(second.sessions[0].processes, 2);
        assert_eq!(second.sessions[0].mem, 2 * 1024 * 1024);

        // After a pause longer than STALE the history is ignored.
        let late = s.snapshot(&rows, t0 + Duration::from_secs(60), &targets, 8, mem);
        assert!(late.processes.iter().all(|p| p.cpu == 1.5));
    }

    #[test]
    fn sends_every_session_process_and_the_busiest_others() {
        let mut processes = Vec::new();
        let mut add = |pid: i32, cpu: f32, mem: u64, session_id: Option<SessionId>| {
            processes.push(ActivityProcess {
                pid,
                name: format!("p{pid}"),
                cpu,
                mem,
                session_id,
            })
        };
        // Idle Session processes are always sent.
        add(1, 0.0, 0, Some(1));
        add(2, 0.0, 0, Some(2));
        // 2*TOP others: pids 1000.. by CPU rank, 2000.. by memory rank, 3000.. neither.
        for i in 0..TOP as i32 {
            add(1000 + i, 50.0 + i as f32, 1, None);
            add(2000 + i, 0.0, 1_000_000 + i as u64, None);
            add(3000 + i, 0.0, 10, None);
        }
        let snap = summarize(processes, 10, Memory::default());
        let pids: HashSet<i32> = snap.processes.iter().map(|p| p.pid).collect();
        assert!(pids.contains(&1) && pids.contains(&2));
        assert!((0..TOP as i32).all(|i| pids.contains(&(1000 + i)) && pids.contains(&(2000 + i))));
        assert!(!(0..TOP as i32).any(|i| pids.contains(&(3000 + i))));
        assert_eq!(snap.processes.len(), 2 + 2 * TOP);
        let expected: f32 = (0..TOP).map(|i| 50.0 + i as f32).sum();
        assert!((snap.cpu_total - expected).abs() < 0.01);
        assert_eq!(
            snap.sessions
                .iter()
                .map(|s| s.session_id)
                .collect::<Vec<_>>(),
            [1, 2]
        );
    }

    /// The whole pipeline on this OS: a child of the test process, busy in a loop, shows up
    /// attributed to its Session with a plausible CPU share and memory.
    #[test]
    fn real_child_is_attributed_with_sane_cpu_and_memory() {
        use std::os::unix::process::CommandExt;
        /// Kills the child's whole process group: the busy loop must not outlive the test.
        struct Reap(std::process::Child);
        impl Drop for Reap {
            fn drop(&mut self) {
                // SAFETY: our own test child's group.
                unsafe { libc::killpg(self.0.id() as i32, libc::SIGKILL) };
                let _ = self.0.wait();
            }
        }
        // sh -> sh: the outer sh waits on a busy child, like a shell on its foreground job.
        let sh = Reap(
            std::process::Command::new("/bin/sh")
                .args(["-c", "/bin/sh -c 'while :; do :; done' & wait"])
                .process_group(0)
                .spawn()
                .unwrap(),
        );
        let shell = sh.0.id() as i32;
        let targets = [target(1, shell)];
        let mut sampler = Sampler::default();
        thread::sleep(Duration::from_millis(300));
        sampler.snapshot(
            &processes().unwrap(),
            Instant::now(),
            &targets,
            8,
            os::memory(),
        );
        thread::sleep(Duration::from_millis(500));
        let snap = sampler.snapshot(
            &processes().unwrap(),
            Instant::now(),
            &targets,
            8,
            os::memory(),
        );

        let session = snap
            .sessions
            .iter()
            .find(|s| s.session_id == 1)
            .expect("attributed");
        assert_eq!(session.processes, 2, "{snap:?}");
        let ours: Vec<_> = snap
            .processes
            .iter()
            .filter(|p| p.session_id == Some(1))
            .collect();
        println!("{session:?}\n{ours:#?}");
        assert!(ours.iter().any(|p| p.pid == shell), "{ours:?}");
        assert!(
            ours.iter()
                .all(|p| p.name == "sh" || p.name == "dash" || p.name == "bash"),
            "{ours:?}"
        );
        // A busy loop: some CPU, never more than its two processes could use. A loaded machine
        // (other test suites, a container build) can leave it very little, so the floor is low.
        assert!(session.cpu > 1.0 && session.cpu <= 250.0, "{session:?}");
        assert!(
            ours.iter().all(|p| p.mem > 64 * 1024 && p.mem < 256 << 20),
            "{ours:?}"
        );
        assert!(snap.mem_total > 1 << 30 && snap.mem_used > 0 && snap.mem_used <= snap.mem_total);
    }
}
