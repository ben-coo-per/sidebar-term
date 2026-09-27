//! Linux reads for Activity and Memory Guard: `/proc`, never `ps`.
//!
//! Processes are the numbered entries of `/proc`. Each one's `stat` gives ppid, CPU ticks, resident
//! pages, start time and the kernel-thread flag; it is world-readable, so no setuid helper is
//! needed. The name is the executable path from the `exe` link (this user's processes; other
//! users' refuse it and keep `stat`'s 15-character `comm`, bracketed `[kworker/0:1]` for a kernel
//! thread, as `ps` prints them). A process seen for the first time uses its lifetime average CPU
//! (`ps`'s %cpu on Linux) from `/proc/uptime`.
//!
//! Memory is the proportional set size plus its swapped share: `Pss + SwapPss` from
//! `/proc/<pid>/smaps_rollup`. Private pages count in full, each shared page is divided among the
//! processes mapping it, and pages that went to swap still count. That is the nearest thing to
//! macOS's physical footprint, what the process costs the machine: a Session's sum does not count
//! a shared `libnode` once per process, and a frozen Tab whose pages were swapped out still shows
//! what it holds, as on the Mac. `RssAnon + RssFile` (`/proc/<pid>/status`) counts every shared
//! library in full for every process and drops swap, the same two errors as macOS's resident size,
//! so it is only the fallback: for another user's process, whose `smaps_rollup` is unreadable (it
//! needs ptrace read access), and for kernels without `smaps_rollup` (before 4.14). A kernel thread
//! (no address space) or a process gone between reads falls back to `stat`'s resident pages.
//! `smaps_rollup` walks the process's page tables, so it costs more than `status` and grows with
//! what is mapped: measured at ~30 µs for a 300 KiB process and ~300 µs for a 14 MiB one
//! (`read_costs` below), against ~30 µs per process for the `stat` and `exe` reads.
//!
//! A process's start time is `stat`'s `starttime`, clock ticks since boot; Memory Guard records it
//! with each pid it stops so a recycled pid is never signalled.
//!
//! Memory Used is `MemTotal - MemAvailable` from `/proc/meminfo`, what the kernel says cannot be
//! given to a new program without swapping: Memory Guard's limit means the same as on the Mac,
//! a percent of physical memory in use. Linux has no wired or compressor figure in the Mac's
//! sense, so both parts read zero and the Panel's muted memory segment is all "other".

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use super::{Memory, PsRow};
use std::fs;

/// `PF_KTHREAD` in `stat`'s `flags`: a kernel thread.
const PF_KTHREAD: u64 = 0x0020_0000;

/// Every process on the machine. `None` if `/proc` could not be listed.
pub(crate) fn processes() -> Option<Vec<PsRow>> {
    let dir = fs::read_dir("/proc").ok()?;
    let clock = Clock::read();
    let rows = dir
        .flatten()
        .filter_map(|entry| {
            let pid: i32 = entry.file_name().to_str()?.parse().ok()?;
            let stat = parse_stat(&fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)?;
            let comm = fs::read_link(format!("/proc/{pid}/exe"))
                .ok()
                .map(|exe| exe_name(&exe.to_string_lossy()))
                .unwrap_or_else(|| stat.name());
            Some(stat.row(comm, &clock))
        })
        .collect();
    Some(rows)
}

/// A process's memory in bytes (see above). `None` when it is gone, or when neither
/// `smaps_rollup` nor `status` gives a figure (another user's kernel thread).
pub(crate) fn footprint(pid: i32) -> Option<u64> {
    if let Some(bytes) = fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
        .ok()
        .and_then(|text| parse_smaps_rollup(&text))
    {
        return Some(bytes);
    }
    parse_status_rss(&fs::read_to_string(format!("/proc/{pid}/status")).ok()?)
}

/// A process's start time, in clock ticks since boot. `None` when it is gone.
pub(crate) fn start_time(pid: i32) -> Option<u64> {
    Some(parse_stat(&fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)?.start_ticks)
}

/// Memory Used and physical memory; the wired and compressed parts are zero (see above).
pub(crate) fn memory() -> Memory {
    fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| parse_meminfo(&text))
        .unwrap_or_default()
}

/// What `stat`'s tick and page counts are in, and the time now, for the lifetime CPU average.
struct Clock {
    /// Clock ticks per second (`CLK_TCK`, normally 100).
    ticks_per_sec: u64,
    page_kib: u64,
    /// Seconds since boot (`/proc/uptime`).
    uptime_secs: f64,
}

impl Clock {
    fn read() -> Self {
        // SAFETY: plain queries.
        let (ticks, page) = unsafe {
            (
                libc::sysconf(libc::_SC_CLK_TCK),
                libc::sysconf(libc::_SC_PAGESIZE),
            )
        };
        Self {
            ticks_per_sec: if ticks > 0 { ticks as u64 } else { 100 },
            page_kib: if page > 0 { page as u64 / 1024 } else { 4 },
            uptime_secs: fs::read_to_string("/proc/uptime")
                .ok()
                .and_then(|s| s.split_ascii_whitespace().next()?.parse().ok())
                .unwrap_or(0.0),
        }
    }
}

/// The fields of `/proc/<pid>/stat` Activity and Memory Guard use.
#[derive(Debug, PartialEq)]
struct Stat {
    pid: i32,
    /// The kernel's name for the process, at most 15 bytes.
    comm: String,
    /// `R`, `S`, `D`, `T` (stopped), `Z`, ...
    state: char,
    ppid: i32,
    flags: u64,
    /// User plus system time, in clock ticks.
    cpu_ticks: u64,
    /// When the process started, in clock ticks since boot.
    start_ticks: u64,
    rss_pages: u64,
}

impl Stat {
    fn kernel_thread(&self) -> bool {
        self.flags & PF_KTHREAD != 0
    }

    /// The name to show when the executable path is unreadable: `comm`, bracketed for a kernel
    /// thread as `ps` does.
    fn name(&self) -> String {
        if self.kernel_thread() {
            format!("[{}]", self.comm)
        } else {
            self.comm.clone()
        }
    }

    fn row(&self, comm: String, clock: &Clock) -> PsRow {
        let cpu_ms = self.cpu_ticks * 1000 / clock.ticks_per_sec;
        let age_secs = clock.uptime_secs - self.start_ticks as f64 / clock.ticks_per_sec as f64;
        // `ps`'s %cpu on Linux: CPU time over the process's lifetime.
        let pcpu = if age_secs > 0.0 {
            (cpu_ms as f64 / 10.0 / age_secs) as f32
        } else {
            0.0
        };
        PsRow {
            pid: self.pid,
            ppid: self.ppid,
            rss_kib: self.rss_pages * clock.page_kib,
            cpu_ms,
            pcpu,
            comm,
        }
    }
}

/// `1234 (comm) S 1 1234 1234 0 -1 4194560 ...`. `comm` may contain spaces and parentheses, so
/// it ends at the last `)`; the numbered fields of proc(5) follow, `state` being field 3.
fn parse_stat(line: &str) -> Option<Stat> {
    let (pid, rest) = line.split_once(" (")?;
    let close = rest.rfind(')')?;
    let fields: Vec<&str> = rest[close + 1..].split_ascii_whitespace().collect();
    let field = |n: usize| fields.get(n - 3).copied();
    let num = |n: usize| field(n)?.parse::<u64>().ok();
    Some(Stat {
        pid: pid.trim().parse().ok()?,
        comm: rest[..close].to_owned(),
        state: field(3)?.chars().next()?,
        ppid: field(4)?.parse().ok()?,
        flags: num(9)?,
        cpu_ticks: num(14)? + num(15)?,
        start_ticks: num(22)?,
        rss_pages: field(24)?.parse::<i64>().ok()?.max(0) as u64,
    })
}

/// The executable path behind `/proc/<pid>/exe`, without the ` (deleted)` the kernel appends
/// once the file is unlinked (a Claude Code build its updater replaced).
fn exe_name(link: &str) -> String {
    link.strip_suffix(" (deleted)").unwrap_or(link).to_owned()
}

/// `Pss + SwapPss` of a `smaps_rollup`, in bytes. `None` without a `Pss` line (a kernel thread's
/// is empty).
fn parse_smaps_rollup(text: &str) -> Option<u64> {
    Some((kib(text, "Pss")? + kib(text, "SwapPss").unwrap_or(0)) * 1024)
}

/// `RssAnon + RssFile` of a `status`, in bytes. `None` without them (a kernel thread, or a kernel
/// before 4.5).
fn parse_status_rss(text: &str) -> Option<u64> {
    Some((kib(text, "RssAnon")? + kib(text, "RssFile")?) * 1024)
}

fn parse_meminfo(text: &str) -> Option<Memory> {
    let total = kib(text, "MemTotal")?;
    let available = kib(text, "MemAvailable")?;
    Some(Memory {
        used: total.saturating_sub(available) * 1024,
        wired: 0,
        compressed: 0,
        total: total * 1024,
    })
}

/// The value of the `key:` line of `meminfo`-style text (`MemTotal:       16274092 kB`), in KiB.
/// The colon is part of the match: `Pss` is not `Pss_Anon`.
fn kib(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let value = line.strip_prefix(key)?.strip_prefix(':')?.trim();
        value
            .strip_suffix("kB")
            .unwrap_or(value)
            .trim()
            .parse()
            .ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIREFOX_STAT: &str = "4242 (Web Content (x)) S 1 4242 4242 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 1 0 12345 1000000 512 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0 0 0 0 0 0 0 0 0\n";
    const KWORKER_STAT: &str = "7 (kworker/0:1) I 2 0 0 0 -1 69238880 0 0 0 0 0 3 0 0 20 0 1 0 15 0 0 18446744073709551615 0 0 0 0 0 0 0 2147483647 0 0 0 0 17 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n";

    fn clock() -> Clock {
        Clock {
            ticks_per_sec: 100,
            page_kib: 4,
            uptime_secs: 1123.45,
        }
    }

    #[test]
    fn stat_line_with_spaces_and_parens_in_comm() {
        assert_eq!(
            parse_stat(FIREFOX_STAT),
            Some(Stat {
                pid: 4242,
                comm: "Web Content (x)".into(),
                state: 'S',
                ppid: 1,
                flags: 4194560,
                cpu_ticks: 300,
                start_ticks: 12345,
                rss_pages: 512,
            })
        );
    }

    #[test]
    fn stat_of_a_kernel_thread() {
        let stat = parse_stat(KWORKER_STAT).unwrap();
        assert_eq!((stat.pid, stat.ppid, stat.state), (7, 2, 'I'));
        assert!(stat.kernel_thread());
        assert_eq!(stat.name(), "[kworker/0:1]");
        assert!(!parse_stat(FIREFOX_STAT).unwrap().kernel_thread());
        assert_eq!(parse_stat(FIREFOX_STAT).unwrap().name(), "Web Content (x)");
    }

    #[test]
    fn stat_garbage() {
        for line in [
            "",
            "garbage",
            "12 (x)",
            "12 (x) S 1",
            "x (y) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24",
        ] {
            assert_eq!(parse_stat(line), None, "{line:?}");
        }
    }

    #[test]
    fn row_from_stat() {
        // 300 ticks = 3 s of CPU; started 123.45 s after boot, 1000 s ago: 0.3% for its life.
        let row = parse_stat(FIREFOX_STAT)
            .unwrap()
            .row("/usr/lib/firefox/firefox".into(), &clock());
        assert_eq!(
            row,
            PsRow {
                pid: 4242,
                ppid: 1,
                rss_kib: 2048,
                cpu_ms: 3000,
                pcpu: 0.3,
                comm: "/usr/lib/firefox/firefox".into(),
            }
        );
        // A process older than the clock says (a race) has no lifetime average.
        let young = Clock {
            uptime_secs: 100.0,
            ..clock()
        };
        assert_eq!(
            parse_stat(FIREFOX_STAT)
                .unwrap()
                .row("f".into(), &young)
                .pcpu,
            0.0
        );
    }

    #[test]
    fn exe_names() {
        assert_eq!(exe_name("/usr/bin/node"), "/usr/bin/node");
        assert_eq!(
            exe_name("/home/u/.local/share/claude/versions/2.1.267 (deleted)"),
            "/home/u/.local/share/claude/versions/2.1.267"
        );
    }

    #[test]
    fn smaps_rollup_pss_and_swap() {
        let text =
            "5581a7000000-7ffd1e7ee000 ---p 00000000 00:00 0                          [rollup]\n\
                    Rss:               12340 kB\n\
                    Pss:                5678 kB\n\
                    Pss_Dirty:          4000 kB\n\
                    Pss_Anon:           3000 kB\n\
                    Pss_File:           2678 kB\n\
                    Pss_Shmem:             0 kB\n\
                    Shared_Clean:       6000 kB\n\
                    Private_Dirty:      4000 kB\n\
                    Swap:                200 kB\n\
                    SwapPss:             100 kB\n\
                    Locked:                0 kB\n";
        assert_eq!(parse_smaps_rollup(text), Some((5678 + 100) * 1024));
        // Before SwapPss existed (4.3), and a kernel thread's empty rollup.
        assert_eq!(
            parse_smaps_rollup("Rss: 8 kB\nPss:                4 kB\n"),
            Some(4096)
        );
        assert_eq!(parse_smaps_rollup(""), None);
        assert_eq!(parse_smaps_rollup("Pss_Anon:  4 kB\n"), None);
    }

    #[test]
    fn status_rss_anon_and_file() {
        let text = "Name:\tnode\nUmask:\t0022\nState:\tS (sleeping)\nPid:\t4242\nPPid:\t1\n\
                    VmPeak:\t 1000000 kB\nVmRSS:\t   90000 kB\nRssAnon:\t   50000 kB\n\
                    RssFile:\t   30000 kB\nRssShmem:\t   10000 kB\nVmSwap:\t     500 kB\n";
        assert_eq!(parse_status_rss(text), Some((50000 + 30000) * 1024));
        let kthread = "Name:\tkworker/0:1\nState:\tI (idle)\nPid:\t7\nPPid:\t2\nThreads:\t1\n";
        assert_eq!(parse_status_rss(kthread), None);
    }

    #[test]
    fn meminfo_used_is_total_less_available() {
        let text = "MemTotal:       16274092 kB\nMemFree:         1234567 kB\n\
                    MemAvailable:   10000000 kB\nBuffers:          300000 kB\nCached:          5000000 kB\n";
        assert_eq!(
            parse_meminfo(text),
            Some(Memory {
                used: 6274092 * 1024,
                wired: 0,
                compressed: 0,
                total: 16274092 * 1024,
            })
        );
        assert_eq!(
            parse_meminfo("MemTotal: 16 kB\nMemFree: 1 kB\n"),
            None,
            "before 3.14"
        );
        assert_eq!(kib("MemTotalX: 5 kB\nMemTotal: 7 kB", "MemTotal"), Some(7));
        assert_eq!(
            kib("HugePages_Total:       0\n", "HugePages_Total"),
            Some(0),
            "no unit"
        );
    }

    #[cfg(target_os = "linux")]
    mod real {
        use super::*;

        #[test]
        fn proc_lists_this_process_with_its_parent_and_exe() {
            let rows = processes().expect("/proc listed");
            let me = std::process::id() as i32;
            let own = rows
                .iter()
                .find(|r| r.pid == me)
                .expect("own process listed");
            // SAFETY: plain query.
            assert_eq!(own.ppid, unsafe { libc::getppid() });
            assert_eq!(
                own.comm,
                fs::read_link("/proc/self/exe").unwrap().to_string_lossy()
            );
            assert!(own.rss_kib > 0);
            // init is root's: its stat is still readable, its exe is not.
            let init = rows.iter().find(|r| r.pid == 1).expect("init listed");
            assert!(!init.comm.starts_with('/') || fs::read_link("/proc/1/exe").is_ok());
            // Kernel threads (a container has none of its own, a real box has hundreds).
            assert!(rows
                .iter()
                .filter(|r| r.comm.starts_with('['))
                .all(|r| r.comm.ends_with(']')));
        }

        #[test]
        fn footprint_and_start_time_of_this_process() {
            let me = std::process::id() as i32;
            let pss = footprint(me).expect("own smaps_rollup");
            let rss = parse_status_rss(&fs::read_to_string("/proc/self/status").unwrap()).unwrap();
            assert!(pss > 1 << 20 && pss <= rss, "pss {pss} rss {rss}");
            let start = start_time(me).expect("own stat");
            assert!(start > 0);
            assert_eq!(start_time(me), Some(start), "stable");
            assert_eq!(start_time(i32::MAX), None, "no such process");
        }

        /// What the reads cost, for the numbers in the module doc:
        /// `cargo test read_costs -- --ignored --nocapture`.
        #[test]
        #[ignore]
        fn read_costs() {
            use std::time::Instant;
            let me = std::process::id() as i32;
            let t = Instant::now();
            let rows = processes().unwrap();
            println!("processes(): {} rows in {:?}", rows.len(), t.elapsed());
            let mut readable: Vec<(u64, i32, String)> = rows
                .iter()
                .filter_map(|r| Some((footprint(r.pid)?, r.pid, r.comm.clone())))
                .collect();
            readable.sort_unstable();
            for (mem, pid, comm) in [readable.first().cloned(), readable.last().cloned()]
                .into_iter()
                .flatten()
            {
                let t = Instant::now();
                for _ in 0..50 {
                    footprint(pid);
                }
                println!(
                    "footprint({pid} {comm}) = {} MiB in {:?}",
                    mem >> 20,
                    t.elapsed() / 50
                );
            }
            let t = Instant::now();
            for _ in 0..50 {
                start_time(me);
            }
            println!("start_time(self) in {:?}", t.elapsed() / 50);
            let t = Instant::now();
            for _ in 0..50 {
                memory();
            }
            println!("memory() = {:?} in {:?}", memory(), t.elapsed() / 50);
        }

        #[test]
        fn memory_totals() {
            let m = memory();
            assert!(m.total > 1 << 28, "{m:?}");
            assert!(m.used > 0 && m.used <= m.total, "{m:?}");
            assert_eq!((m.wired, m.compressed), (0, 0));
        }
    }
}
