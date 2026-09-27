//! macOS reads for Activity and Memory Guard: `ps`, `proc_pid_rusage` and Mach host statistics.
//!
//! Processes come from `/bin/ps`, not libproc. `PROC_PIDTASKINFO` (CPU time, resident size) is
//! EPERM for other users' processes, about a third of them, including the usual top consumers
//! (WindowServer, kernel_task, mds_stores). `ps` is setuid root, reads them all, and costs ~20 ms
//! per call. A process seen for the first time uses `ps`'s own decaying %cpu.
//!
//! Memory is the physical footprint, Activity Monitor's "Memory" column (`proc_pid_rusage`): what
//! the process costs the Mac, compressed and swapped pages included, shared pages not. Resident
//! size gets both wrong: it counts a shared executable in full and drops what the compressor holds,
//! so under memory pressure it shows a 200 MB process at 5 MB. `proc_pid_rusage` works for this
//! user's processes (every Session's), so other users' processes fall back to `ps`'s resident size.
//!
//! A process's start time is `ri_proc_start_abstime`, from the same call; Memory Guard records it
//! with each pid it stops so a recycled pid is never signalled.
//!
//! Memory Used is Activity Monitor's: app memory + wired + compressed (`host_statistics64`);
//! physical memory is `hw.memsize`.

use super::{Memory, PsRow};
use std::mem::{size_of, MaybeUninit};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Every process on the Mac except the `ps` itself. `None` if `ps` could not run.
pub(crate) fn processes() -> Option<Vec<PsRow>> {
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

/// A process's physical footprint and start time (`proc_pid_rusage`). `None` when it is gone or
/// belongs to another user.
fn rusage(pid: i32) -> Option<(u64, u64)> {
    let mut info = MaybeUninit::<libc::rusage_info_v0>::zeroed();
    // SAFETY: `info` is a writable, zeroed `rusage_info_v0`, the struct the V0 flavor fills.
    let rc = unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V0, info.as_mut_ptr().cast()) };
    if rc != 0 {
        return None;
    }
    // SAFETY: filled by the kernel; plain data (all-zero is valid too).
    let info = unsafe { info.assume_init() };
    Some((info.ri_phys_footprint, info.ri_proc_start_abstime))
}

/// A process's physical footprint in bytes. `None` when it is gone or belongs to another user.
pub(crate) fn footprint(pid: i32) -> Option<u64> {
    rusage(pid).map(|(footprint, _)| footprint)
}

/// A process's start time (`ri_proc_start_abstime`). `None` when it is gone or belongs to
/// another user.
pub(crate) fn start_time(pid: i32) -> Option<u64> {
    rusage(pid).map(|(_, start)| start)
}

/// Memory Used, its wired and compressed parts, and physical memory.
pub(crate) fn memory() -> Memory {
    let used = mem_used().unwrap_or_default();
    Memory {
        used: used.used,
        wired: used.wired,
        compressed: used.compressed,
        total: mem_total().unwrap_or(0),
    }
}

#[derive(Default)]
struct MemUsed {
    used: u64,
    wired: u64,
    compressed: u64,
}

#[allow(deprecated)] // libc points at the `mach2` crate; one call is not worth the dependency.
fn host_port() -> libc::mach_port_t {
    // A send right that lives as long as the app; fetched once so it is not leaked per call.
    static HOST: OnceLock<libc::mach_port_t> = OnceLock::new();
    // SAFETY: no arguments; returns this task's host port.
    *HOST.get_or_init(|| unsafe { libc::mach_host_self() })
}

/// Activity Monitor's "Memory Used": app memory (internal minus purgeable) + wired + compressed.
fn mem_used() -> Option<MemUsed> {
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
    let page = page as u64;
    let app = u64::from(s.internal_page_count).saturating_sub(u64::from(s.purgeable_count));
    let wired = u64::from(s.wire_count) * page;
    let compressed = u64::from(s.compressor_page_count) * page;
    Some(MemUsed {
        used: app * page + wired + compressed,
        wired,
        compressed,
    })
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

#[cfg(test)]
mod tests {
    use super::super::display_name;
    use super::*;

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
    fn real_ps_sees_other_users_processes() {
        let rows = processes().expect("ps runs");
        let me = std::process::id() as i32;
        assert!(rows.iter().any(|r| r.pid == me), "own process listed");
        // launchd is root's: libproc refuses its task info, ps does not.
        let launchd = rows.iter().find(|r| r.pid == 1).expect("launchd listed");
        assert!(launchd.rss_kib > 0);
        assert_eq!(display_name(&launchd.comm), "launchd");
    }

    #[test]
    fn real_memory_totals() {
        let m = memory();
        assert!(m.total > 1 << 30, "{m:?}");
        assert!(m.used > 0 && m.used <= m.total, "{m:?}");
        assert!(m.wired > 0 && m.wired + m.compressed <= m.used, "{m:?}");
    }

    #[test]
    fn footprint_of_own_process() {
        let me = std::process::id() as i32;
        assert!(footprint(me).unwrap() > 0 && start_time(me).unwrap() > 0);
        // launchd is root's: refused, so the sampler falls back to resident size.
        assert_eq!(footprint(1), None);
        assert_eq!(start_time(1), None);
    }
}
