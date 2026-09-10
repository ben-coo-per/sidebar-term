//! Activity: CPU and memory of every process on the Mac, each Session's processes attributed to
//! it, for the sidebar Panel's Activity view. Sampled only while the webview watches.
//!
//! Source: `/bin/ps`, not libproc. `PROC_PIDTASKINFO` (CPU time, resident size) is EPERM for other
//! users' processes, about a third of them, including the usual top consumers (WindowServer,
//! kernel_task, mds_stores). `ps` is setuid root, reads them all, and costs ~20 ms per call.
//!
//! CPU% is the change in accumulated CPU time between two samples over the wall time between them,
//! as Activity Monitor shows it (100 = one core). A process without a previous sample uses `ps`'s own
//! decaying %cpu instead.
//!
//! A process belongs to a Session when it is the Session's shell or descends from it by ppid. A
//! daemon that detaches (reparents to launchd) leaves its Session.

use crate::detect::{agent_command, process::classify_agent};
use crate::model::{
    ActivityProcess, ActivitySession, ActivitySnapshot, ProbeTarget, SessionId, EVENT_ACTIVITY,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::mem::{size_of, MaybeUninit};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Time between samples while watched.
const TICK: Duration = Duration::from_secs(2);
/// A previous sample older than this (watching was paused) is too stale to diff against.
const STALE: Duration = Duration::from_secs(6);
/// How many of the busiest non-Session processes to send, by CPU and again by memory.
const TOP: usize = 40;

/// Handle to the sampling thread (Tauri state). Starts idle.
pub struct Activity {
    watching: Arc<(Mutex<bool>, Condvar)>,
}

impl Activity {
    /// Start or stop sampling. Starting samples at once, then every `TICK`.
    pub fn watch(&self, on: bool) {
        let (lock, cvar) = &*self.watching;
        *lock.lock().unwrap_or_else(PoisonError::into_inner) = on;
        cvar.notify_all();
    }
}

/// Start the sampling thread, idle until `Activity::watch(true)`. While watched, emit an
/// `ActivitySnapshot` every `TICK`. `targets` lists live Sessions, as for the monitor.
///
/// The thread never exits: a failed `ps` or a panic skips the tick.
pub fn spawn<F>(app: AppHandle, targets: F) -> Activity
where
    F: Fn() -> Vec<ProbeTarget> + Send + 'static,
{
    let watching = Arc::new((Mutex::new(false), Condvar::new()));
    let shared = Arc::clone(&watching);
    let started = thread::Builder::new()
        .name("activity".into())
        .spawn(move || {
            let (lock, cvar) = &*shared;
            let cpu_count = thread::available_parallelism().map_or(1, |n| n.get() as u32);
            let mut sampler = Sampler::default();
            loop {
                drop(
                    cvar.wait_while(lock.lock().unwrap_or_else(PoisonError::into_inner), |on| {
                        !*on
                    })
                    .unwrap_or_else(PoisonError::into_inner),
                );
                let sampled = catch_unwind(AssertUnwindSafe(|| {
                    let rows = run_ps()?;
                    let memory = Memory::read();
                    Some(sampler.snapshot(&rows, Instant::now(), &targets(), cpu_count, memory))
                }));
                match sampled {
                    Ok(Some(snapshot)) => {
                        if let Err(e) = app.emit(EVENT_ACTIVITY, &snapshot) {
                            eprintln!("activity: emit failed: {e}");
                        }
                    }
                    Ok(None) => eprintln!("activity: ps failed; skipping this tick"),
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
                        |on| *on,
                    )
                    .unwrap_or_else(PoisonError::into_inner),
                );
            }
        });
    if let Err(e) = started {
        eprintln!("activity: could not start thread: {e}");
    }
    Activity { watching }
}

/// One row of `ps` output.
#[derive(Clone, Debug, PartialEq)]
struct PsRow {
    pid: i32,
    ppid: i32,
    rss_kib: u64,
    /// Accumulated user + system CPU time in milliseconds (`ps` prints centiseconds).
    cpu_ms: u64,
    /// `ps`'s own %cpu, a decaying average: used for a process without a previous sample.
    pcpu: f32,
    /// Executable path, or a bare name such as a login shell's `-zsh`.
    comm: String,
}

/// Every process on the Mac except the `ps` itself. `None` if `ps` could not run.
fn run_ps() -> Option<Vec<PsRow>> {
    let child = Command::new("/bin/ps")
        .args(["-axww", "-o", "pid=,ppid=,rss=,time=,%cpu=,comm="])
        .env("LC_ALL", "C") // a `.` decimal point in %cpu
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let own = child.id() as i32;
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    let mut rows = parse_ps(&String::from_utf8_lossy(&out.stdout));
    rows.retain(|r| r.pid != own);
    Some(rows)
}

fn parse_ps(out: &str) -> Vec<PsRow> {
    out.lines().filter_map(parse_ps_line).collect()
}

/// `  432     1  41584 303:18.87  32.8 /System/.../WindowServer`: five whitespace-separated
/// fields, then `comm` to the end of the line (it may contain spaces).
fn parse_ps_line(line: &str) -> Option<PsRow> {
    let mut rest = line;
    let pid = next_field(&mut rest)?.parse().ok()?;
    let ppid = next_field(&mut rest)?.parse().ok()?;
    let rss_kib = next_field(&mut rest)?.parse().ok()?;
    let cpu_ms = parse_cpu_time(next_field(&mut rest)?)?;
    let pcpu = next_field(&mut rest)?.parse().ok()?;
    let comm = rest.trim();
    if comm.is_empty() {
        return None;
    }
    Some(PsRow {
        pid,
        ppid,
        rss_kib,
        cpu_ms,
        pcpu,
        comm: comm.to_owned(),
    })
}

fn next_field<'a>(rest: &mut &'a str) -> Option<&'a str> {
    let s = rest.trim_start();
    if s.is_empty() {
        return None;
    }
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    let (field, tail) = s.split_at(end);
    *rest = tail;
    Some(field)
}

/// `ps` `time`, `[dd-][hh:]mm:ss.cc` (minutes may exceed 59), in milliseconds.
fn parse_cpu_time(s: &str) -> Option<u64> {
    let (days, clock) = match s.split_once('-') {
        Some((d, c)) => (d.parse::<u64>().ok()?, c),
        None => (0, s),
    };
    let mut parts = clock.rsplit(':');
    let secs = parts.next()?;
    let (whole, frac) = secs.split_once('.').unwrap_or((secs, ""));
    if frac.len() > 3 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut ms = whole.parse::<u64>().ok()? * 1000;
    if !frac.is_empty() {
        ms += frac.parse::<u64>().ok()? * 10u64.pow(3 - frac.len() as u32);
    }
    let mut unit = 60_000;
    for part in parts {
        ms += part.parse::<u64>().ok()? * unit;
        unit *= 60;
    }
    Some(ms + days * 86_400_000)
}

/// What the Activity view calls a process: its executable's file name, or the agent's command
/// name for a coding agent (native Claude Code's file name is its version, `2.1.267`).
fn display_name(comm: &str) -> String {
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

/// "Memory Used" and physical memory, in bytes. Zero when unreadable.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Memory {
    used: u64,
    total: u64,
}

impl Memory {
    fn read() -> Self {
        Self {
            used: mem_used().unwrap_or(0),
            total: mem_total().unwrap_or(0),
        }
    }
}

#[allow(deprecated)] // libc points at the `mach2` crate; one call is not worth the dependency.
fn host_port() -> libc::mach_port_t {
    // A send right that lives as long as the app; fetched once so it is not leaked per call.
    static HOST: OnceLock<libc::mach_port_t> = OnceLock::new();
    // SAFETY: no arguments; returns this task's host port.
    *HOST.get_or_init(|| unsafe { libc::mach_host_self() })
}

/// Activity Monitor's "Memory Used": app memory (internal minus purgeable) + wired + compressed.
fn mem_used() -> Option<u64> {
    let mut stats = MaybeUninit::<libc::vm_statistics64>::zeroed();
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: `stats` is a zeroed, writable `vm_statistics64` and `count` is its size in words.
    let rc = unsafe {
        libc::host_statistics64(
            host_port(),
            libc::HOST_VM_INFO64,
            stats.as_mut_ptr().cast(),
            &mut count,
        )
    };
    if rc != libc::KERN_SUCCESS {
        return None;
    }
    // SAFETY: filled by the kernel; plain data (all-zero is valid too).
    let s = unsafe { stats.assume_init() };
    // SAFETY: plain query.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page <= 0 {
        return None;
    }
    let pages = u64::from(s.internal_page_count).saturating_sub(u64::from(s.purgeable_count))
        + u64::from(s.wire_count)
        + u64::from(s.compressor_page_count);
    Some(pages * page as u64)
}

fn mem_total() -> Option<u64> {
    let mut bytes: u64 = 0;
    let mut len = size_of::<u64>();
    // SAFETY: `bytes` is a writable u64 and `len` says so; the name is NUL-terminated.
    let rc = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut bytes as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && bytes > 0).then_some(bytes)
}

/// Turns successive `ps` samples into snapshots: keeps each process's CPU time to diff against.
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
                    mem: r.rss_kib * 1024,
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
            pid,
            ppid,
            rss_kib: 1024,
            cpu_ms,
            pcpu: 1.5,
            comm: comm.into(),
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
    fn cpu_time_formats() {
        let cases = [
            ("0:00.00", Some(0)),
            ("0:01.23", Some(1_230)),
            ("303:18.87", Some(303 * 60_000 + 18_870)),
            ("1:02:03.40", Some(3_723_400)),
            ("2-01:00:00.00", Some(2 * 86_400_000 + 3_600_000)),
            ("0:05", Some(5_000)),
            ("0:05.5", Some(5_500)),
            ("", None),
            ("x:00.00", None),
            ("0:00.-1", None),
            ("0:00.1234", None),
        ];
        for (s, want) in cases {
            assert_eq!(parse_cpu_time(s), want, "{s:?}");
        }
    }

    #[test]
    fn ps_lines() {
        let out = "  432     1  41584 303:18.87  32.8 /System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer\n\
                   96148     1  52640   1:05.34   6.0 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome Helper (Renderer)\n\
                   2542  1609   3296   0:00.03   0.0 -zsh\n\
                   garbage line\n\
                   12 1 3 0:00.00 0.0\n";
        let rows = parse_ps(out);
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0],
            PsRow {
                pid: 432,
                ppid: 1,
                rss_kib: 41584,
                cpu_ms: 303 * 60_000 + 18_870,
                pcpu: 32.8,
                comm: "/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer"
                    .into(),
            }
        );
        assert_eq!(
            rows[1].comm,
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome Helper (Renderer)"
        );
        assert_eq!(rows[2].comm, "-zsh");
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
        let mem = Memory { used: 8, total: 16 };

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

    #[test]
    fn real_ps_sees_other_users_processes() {
        let rows = run_ps().expect("ps runs");
        let me = std::process::id() as i32;
        assert!(rows.iter().any(|r| r.pid == me), "own process listed");
        // launchd is root's: libproc refuses its task info, ps does not.
        let launchd = rows.iter().find(|r| r.pid == 1).expect("launchd listed");
        assert!(launchd.rss_kib > 0);
        assert_eq!(display_name(&launchd.comm), "launchd");
    }

    #[test]
    fn real_memory_totals() {
        let m = Memory::read();
        assert!(m.total > 1 << 30, "{m:?}");
        assert!(m.used > 0 && m.used <= m.total, "{m:?}");
    }
}
