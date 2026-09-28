//! `/proc`-based reads of the Foreground process group (members, comm, path, argv, cwd) and of
//! a process's children (for Suites' walk of a Session's tree).
//! The Linux backend of `detect::os`, for the Host daemon (docs/adr/0002). Never panics: an
//! unreadable or vanished `/proc/<pid>` reads as `None` / empty. OWNER: detection agent.
//!
//! Reads (`proc(5)`): `/proc/<pid>/stat` (comm, state, ppid, pgrp), `/proc/<pid>/exe` and
//! `/proc/<pid>/cwd` (symlinks the kernel resolves for us), `/proc/<pid>/cmdline` and
//! `/proc/<pid>/environ` (NUL-separated). Permissions differ from libproc: `stat` and `cmdline`
//! are readable for every process, so a `sudo`'d job's argv is known here (and is rerun by
//! Resume); `exe`, `cwd` and `environ` need ptrace read access (same uid, or root), so they
//! read as `None` / empty for another user's process, as on macOS. A process caught in the
//! last microseconds of its exec reads the same way (its argv area and dumpable flag are not
//! set yet); the next tick sees it whole.
//!
//! Members of a process group: there is no `proc_listpids(PROC_PGRP_ONLY)`, so one scan of every
//! `/proc/<pid>/stat` (a few ms) is cached for `SCAN_TTL` and shared by every probe of one
//! monitor tick, as `activity.rs` runs one `ps` per sample. The group leader is always read
//! afresh, so a job that just started is seen at once; a member that joined since the scan (the
//! native `codex` under its npm launcher) is seen by the next one. Every candidate's `stat` is
//! re-read, so a pid that exited or was reused since the scan is never reported.

use super::{non_empty, Proc};
use libc::pid_t;
use std::fs::{self, File};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long one `/proc` scan serves `group_members`. Shorter than any poller's tick (the monitor's
/// 500 ms, Resume's 1 s), so each tick scans once and probes every Session from that.
const SCAN_TTL: Duration = Duration::from_millis(100);
/// `TASK_COMM_LEN - 1`: a `comm` longer than this is not the kernel's.
const MAX_COMM: usize = 15;
/// Largest `/proc` file we read whole (a `cmdline` or `environ` is normally a few KiB).
const MAX_FILE: u64 = 1 << 20;

/// One `/proc` scan: `(pid, pgrp)` of every process, and when it was taken.
struct Scan {
    at: Instant,
    pgrps: Arc<[(pid_t, pid_t)]>,
}

static SCAN: Mutex<Option<Scan>> = Mutex::new(None);

/// Pids of every process in process group `pgid`, ascending. Empty when the group does not
/// exist (or on any error).
pub fn group_members(pgid: pid_t) -> Vec<pid_t> {
    if pgid <= 0 {
        return Vec::new();
    }
    let mut members: Vec<pid_t> = cached_scan()
        .iter()
        .filter(|&&(_, g)| g == pgid)
        .map(|&(p, _)| p)
        .collect();
    // The leader may have started since the scan; a job's leader is the common case.
    if !members.contains(&pgid) {
        members.push(pgid);
    }
    // A pid from the scan may have exited, or been reused by a process of another group.
    members.retain(|&pid| stat(pid).is_some_and(|s| s.pgrp == pgid));
    members.sort_unstable();
    members.dedup();
    members
}

/// Pids of the direct children of `ppid`, ascending. Empty when it has none (or on any error).
/// Read from `/proc/<ppid>/task/*/children` (a child is listed under the thread that forked it);
/// a kernel without that file (`CONFIG_PROC_CHILDREN` off) is answered by a `/proc` scan.
pub fn children(ppid: pid_t) -> Vec<pid_t> {
    if ppid <= 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let tasks = match fs::read_dir(format!("/proc/{ppid}/task")) {
        Ok(tasks) => tasks,
        Err(_) => return Vec::new(),
    };
    let mut listed = false;
    for task in tasks.flatten() {
        if let Ok(text) = fs::read_to_string(task.path().join("children")) {
            listed = true;
            out.extend(text.split_whitespace().filter_map(|p| p.parse::<pid_t>().ok()));
        }
    }
    if !listed {
        out = scan_children(ppid);
    }
    out.retain(|&p| p > 0);
    out.sort_unstable();
    out.dedup();
    out
}

/// Children of `ppid` by a scan of every `/proc/<pid>/stat`.
fn scan_children(ppid: pid_t) -> Vec<pid_t> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse::<pid_t>().ok()))
        .filter(|&pid| stat(pid).is_some_and(|s| s.ppid == ppid))
        .collect()
}

/// The `(pid, pgrp)` pairs of the last scan, taken afresh once `SCAN_TTL` has passed.
fn cached_scan() -> Arc<[(pid_t, pid_t)]> {
    let mut guard = SCAN.lock().unwrap_or_else(PoisonError::into_inner);
    match guard.as_ref() {
        Some(scan) if scan.at.elapsed() < SCAN_TTL => scan.pgrps.clone(),
        _ => {
            let pgrps: Arc<[(pid_t, pid_t)]> = scan_pgrps().into();
            *guard = Some(Scan {
                at: Instant::now(),
                pgrps: pgrps.clone(),
            });
            pgrps
        }
    }
}

/// `(pid, pgrp)` of every process in `/proc`, in directory order.
fn scan_pgrps() -> Vec<(pid_t, pid_t)> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(512);
    let mut buf = Vec::with_capacity(512);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|n| n.parse::<pid_t>().ok()) else {
            continue;
        };
        buf.clear();
        let path = entry.path().join("stat");
        if read_into(&path, &mut buf).is_some() {
            if let Some(s) = parse_stat(&String::from_utf8_lossy(&buf)) {
                out.push((pid, s.pgrp));
            }
        }
    }
    out
}

/// `comm` and parent of a live process (`/proc/<pid>/stat`). Works across uids. `None` for a
/// zombie or a vanished pid. `comm` is the name the process was exec'd by, truncated to 15
/// bytes: for the native Claude Code install (`~/.local/bin/claude`, a symlink) that is `claude`.
pub fn short_info(pid: pid_t) -> Option<Proc> {
    if pid <= 0 {
        return None;
    }
    proc_of(stat(pid)?)
}

/// The `Proc` of a `stat`, unless the process is a zombie (`Z`) or already dead (`X`).
fn proc_of(s: Stat) -> Option<Proc> {
    if matches!(s.state, 'Z' | 'X') {
        return None;
    }
    Some(Proc {
        pid: s.pid,
        ppid: s.ppid,
        comm: s.comm,
    })
}

/// Resolved executable path (`/proc/<pid>/exe`). Same-uid processes only (ptrace read access);
/// `None` for another user's process and for a kernel thread. A deleted executable (the
/// auto-updater removes old Claude Code versions while they run) still reads as its path: the
/// kernel's ` (deleted)` suffix is stripped, since the path is what classification wants.
pub fn exe_path(pid: pid_t) -> Option<String> {
    link_target(pid, "exe")
}

/// Current working directory (`/proc/<pid>/cwd`), as the kernel's canonical path. `None` for
/// processes of another uid (EACCES, e.g. `sudo`). A deleted directory reads as its former
/// path (` (deleted)` stripped).
pub fn cwd(pid: pid_t) -> Option<String> {
    link_target(pid, "cwd")
}

fn link_target(pid: pid_t, name: &str) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    let target = fs::read_link(proc_path(pid, name)).ok()?;
    non_empty(strip_deleted(&target.to_string_lossy()).to_owned())
}

/// A `/proc` link target without the ` (deleted)` the kernel appends once the file is unlinked.
fn strip_deleted(target: &str) -> &str {
    target.strip_suffix(" (deleted)").unwrap_or(target)
}

/// Full argv (`/proc/<pid>/cmdline`). Readable across uids. `None` for a kernel thread, a
/// zombie or a vanished pid (an empty `cmdline`).
pub fn argv(pid: pid_t) -> Option<Vec<String>> {
    if pid <= 0 {
        return None;
    }
    let mut buf = Vec::new();
    read_into(&proc_path(pid, "cmdline"), &mut buf)?;
    parse_cmdline(&buf)
}

/// Full argv and the environment the process was exec'd with (`/proc/<pid>/environ`), as
/// `KEY=value` strings. `None` when argv is unreadable; the environment alone is empty for
/// another user's process (`environ` needs ptrace read access).
pub fn argv_env(pid: pid_t) -> Option<(Vec<String>, Vec<String>)> {
    let argv = argv(pid)?;
    let mut buf = Vec::new();
    let env = match read_into(&proc_path(pid, "environ"), &mut buf) {
        Some(()) => parse_nul_list(&buf),
        None => Vec::new(),
    };
    Some((argv, env))
}

/// argv from a `cmdline` file: NUL-terminated strings. Empty for a kernel thread (nothing to
/// report: `None`). A process that rewrote its argv area (`postgres: checkpointer`) reads as
/// however it left it.
pub(crate) fn parse_cmdline(bytes: &[u8]) -> Option<Vec<String>> {
    let args = parse_nul_list(bytes);
    if args.is_empty() {
        None
    } else {
        Some(args)
    }
}

/// NUL-terminated strings, as `cmdline` and `environ` hold them (the last one's NUL may be
/// missing when a process rewrote the area).
fn parse_nul_list(bytes: &[u8]) -> Vec<String> {
    let bytes = bytes.strip_suffix(b"\0").unwrap_or(bytes);
    if bytes.is_empty() {
        return Vec::new();
    }
    bytes
        .split(|&b| b == 0)
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

/// The fields of `/proc/<pid>/stat` the probe uses.
#[derive(Debug, PartialEq, Eq)]
struct Stat {
    pid: pid_t,
    comm: String,
    /// `R`, `S`, `D`, `Z` (zombie), `T`, `X` (dead), ...
    state: char,
    ppid: pid_t,
    pgrp: pid_t,
}

fn stat(pid: pid_t) -> Option<Stat> {
    let mut buf = Vec::with_capacity(512);
    read_into(&proc_path(pid, "stat"), &mut buf)?;
    parse_stat(&String::from_utf8_lossy(&buf)).filter(|s| s.pid == pid)
}

/// `pid (comm) state ppid pgrp session tty_nr tpgid ...` (`proc_pid_stat(5)`). `comm` is the
/// executable's file name truncated to 15 bytes and may hold spaces and parentheses, so the
/// fields after it are found from the last `)`.
fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close < open || close - open - 1 > MAX_COMM {
        return None;
    }
    let pid = text[..open].trim().parse().ok()?;
    let comm = text[open + 1..close].to_owned();
    let mut fields = text[close + 1..].split_ascii_whitespace();
    let state = fields.next()?.chars().next()?;
    let ppid = fields.next()?.parse().ok()?;
    let pgrp = fields.next()?.parse().ok()?;
    Some(Stat {
        pid,
        comm,
        state,
        ppid,
        pgrp,
    })
}

fn proc_path(pid: pid_t, name: &str) -> PathBuf {
    PathBuf::from(format!("/proc/{pid}/{name}"))
}

/// Read `path` (up to `MAX_FILE`) into `buf`. `None` if it cannot be opened or read (gone,
/// EACCES). `/proc` files have no size, so this is the one way to read them.
fn read_into(path: &std::path::Path, buf: &mut Vec<u8>) -> Option<()> {
    File::open(path).ok()?.take(MAX_FILE).read_to_end(buf).ok()?;
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::testutil::{spawn_in_own_group, wait_until, TempDir};

    #[test]
    fn stat_lines() {
        // A plain process; only the first fields matter.
        let s = parse_stat("4242 (sleep) S 4200 4242 4200 34816 4242 4194304 100 0 0 0 0 0 0 0 20 0 1 0 12345 5820416 200 18446744073709551615\n").unwrap();
        assert_eq!(
            s,
            Stat {
                pid: 4242,
                comm: "sleep".into(),
                state: 'S',
                ppid: 4200,
                pgrp: 4242
            }
        );
        // A comm with spaces and parentheses (an executable named `my (odd) app`), found from
        // the last `)`; and the 15-byte truncation the kernel applies.
        let s = parse_stat("7 (my (odd) app) R 1 7 7 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 1 0 0 0").unwrap();
        assert_eq!((s.comm.as_str(), s.state, s.ppid, s.pgrp), ("my (odd) app", 'R', 1, 7));
        let s = parse_stat("8 (codex-aarch64-u) S 1 8 8 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 1 0 0 0").unwrap();
        assert_eq!(s.comm, "codex-aarch64-u");
        // Zombies and dead tasks are not live processes.
        let z = parse_stat("9 (zsh) Z 1 9 9 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 1 0 0 0").unwrap();
        assert_eq!(z.state, 'Z');
        assert_eq!(proc_of(z), None);
        let x = parse_stat("9 (zsh) X 1 9 9 0 -1 0").unwrap();
        assert_eq!(proc_of(x), None);
        let live = parse_stat("9 (zsh) S 1 9 9 0 -1 0").unwrap();
        assert_eq!(
            proc_of(live),
            Some(Proc {
                pid: 9,
                ppid: 1,
                comm: "zsh".into()
            })
        );
        // Truncated or odd lines never panic.
        for bad in [
            "",
            "12",
            "12 (x",
            "12 (x)",
            "12 (x) S",
            "12 (x) S 1",
            "12 (x) S one 2",
            ") 12 (x S 1 2",
            "12 (this name is far too long for a comm) S 1 2",
            "x (x) S 1 2",
        ] {
            assert_eq!(parse_stat(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn cmdline_shapes() {
        assert_eq!(parse_cmdline(b"sleep\x0030\0").unwrap(), ["sleep", "30"]);
        assert_eq!(parse_cmdline(b"/bin/sleep\x0030\0").unwrap(), ["/bin/sleep", "30"]);
        // A trailing empty argument (`cmd ""`) is kept; only the final NUL goes.
        assert_eq!(parse_cmdline(b"cmd\0\0").unwrap(), ["cmd", ""]);
        // A rewritten argv area: no NULs at all.
        assert_eq!(
            parse_cmdline(b"postgres: checkpointer   ").unwrap(),
            ["postgres: checkpointer   "]
        );
        // A kernel thread (or a zombie) has an empty cmdline: nothing to report.
        assert_eq!(parse_cmdline(b""), None);
        assert_eq!(parse_cmdline(b"\0"), None);
        // environ: same shape.
        assert_eq!(parse_nul_list(b"HOME=/x\0PATH=/bin\0"), ["HOME=/x", "PATH=/bin"]);
        assert!(parse_nul_list(b"").is_empty());
        // Non-UTF-8 bytes read lossily rather than failing.
        assert_eq!(parse_cmdline(b"caf\xe9\0").unwrap(), ["caf\u{fffd}"]);
    }

    #[test]
    fn deleted_link_targets_keep_their_path() {
        assert_eq!(
            strip_deleted("/home/u/.local/share/claude/versions/2.1.267 (deleted)"),
            "/home/u/.local/share/claude/versions/2.1.267"
        );
        assert_eq!(strip_deleted("/usr/bin/sleep"), "/usr/bin/sleep");
        assert_eq!(strip_deleted("/tmp/x (deleted) (deleted)"), "/tmp/x (deleted)");
    }

    #[test]
    fn init_reads_across_uids_except_paths_and_env() {
        // pid 1 is root's: stat and cmdline are readable, exe, cwd and environ are not.
        assert!(short_info(1).is_some());
        assert!(argv(1).is_some());
        // SAFETY: plain query.
        if unsafe { libc::geteuid() } != 0 {
            assert_eq!(exe_path(1), None);
            assert_eq!(cwd(1), None);
            assert_eq!(argv_env(1).map(|(_, env)| env), Some(vec![]));
        }
    }

    #[test]
    fn a_member_that_is_not_the_leader_is_found_by_the_scan() {
        let dir = TempDir::new("proc-scan");
        // `; true` keeps sh from exec'ing: sh stays the leader, sleep is its child.
        let job = spawn_in_own_group("/bin/sh", &["-c", "/bin/sleep 30; true"], dir.path());
        let pgid = job.pid();
        // Two members, the second having exec'd (a fresh fork still reads as `sh`).
        let sleeping = || {
            let members = group_members(pgid);
            members.len() == 2
                && members[0] == pgid
                && short_info(members[1]).is_some_and(|p| p.comm == "sleep")
        };
        assert!(wait_until(sleeping), "sleep never joined: {:?}", group_members(pgid));
        let child = short_info(group_members(pgid)[1]).expect("child");
        assert_eq!(child.ppid, pgid);
        assert_eq!(argv(child.pid).unwrap(), ["/bin/sleep", "30"]);
    }

    #[test]
    fn the_scan_cache_never_reports_a_gone_or_unseen_member_wrongly() {
        let dir = TempDir::new("proc-cache");
        let mut a = spawn_in_own_group("/bin/sleep", &["30"], dir.path());
        assert_eq!(group_members(a.pid()), vec![a.pid()]); // scanned now
        a.kill();
        // Gone since the scan: dropped at once, not at the next scan.
        assert!(group_members(a.pid()).is_empty());
        // Started since the scan: a leader is found without waiting for the next scan.
        let b = spawn_in_own_group("/bin/sleep", &["30"], dir.path());
        assert_eq!(group_members(b.pid()), vec![b.pid()]);

        let start = Instant::now();
        let n = scan_pgrps().len();
        println!("/proc scan: {n} processes in {:?}", start.elapsed());
        assert!(n >= 2);
    }
}
