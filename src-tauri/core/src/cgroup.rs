//! Cgroups for a Host's Sessions (Linux, cgroup v2; the daemon, `sidebar-termd --cgroups`).
//!
//! Every process a Session starts is the Host's to keep in check, but a Host that shares one
//! cgroup with its Sessions is throttled with them: when they reach the memory limit the kernel
//! reclaims and stalls the whole group, the daemon included, and every client sees every Session
//! go grey although they all still run. So the daemon's systemd unit delegates its cgroup
//! (`Delegate=yes`), and the daemon splits it:
//!
//! ```text
//! sidebar-termd.service/      the delegated root: no processes of its own (cgroup v2's rule)
//! ├── daemon/                 the daemon alone; memory.low keeps its pages in RAM
//! └── sessions/               every Session together: memory.high / max / swap.max are here
//!     ├── <run>-<id>/         one Session's shell and everything it starts
//!     └── ...
//! ```
//!
//! The limits sit on `sessions/`, never above the daemon, so Sessions at their limit slow down
//! themselves and each other but never the daemon. Each Session gets its own leaf so the Host can
//! tell its clients which Session uses the memory ([`Cgroups::sample`]: the `pressure` message).
//! A Session's shell is moved into its leaf by a `sh` that writes its own pid there and then
//! execs the shell ([`Cgroups::wrap`]), so nothing it starts is ever outside it. `<run>` is the
//! daemon's pid: a leaf a previous run left populated (a process that outlived its hangup) is
//! never reused for a new Session, and is removed once it empties ([`Cgroups::sweep`]).

use crate::model::{HostPressure, PressureLevel, SessionId, SessionMemory};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

/// The daemon's own group, under the delegated root. The unit's `DelegateSubgroup=` names it too.
pub const DAEMON_GROUP: &str = "daemon";
/// The group every Session's leaf sits in, which carries the limits.
pub const SESSIONS_GROUP: &str = "sessions";
/// The daemon's `memory.low`: reclaim leaves this much of its memory alone while it can take
/// from elsewhere. Its resident size is ~25 MB; this covers its heap, the binary and growth.
pub const DAEMON_LOW: u64 = 256 << 20;
/// Controllers enabled for the daemon / sessions split: memory (required) and cpu (when the
/// user manager has it), which splits CPU time evenly between the daemon and all the Sessions.
const CONTROLLERS: [&str; 2] = ["memory", "cpu"];
/// How many of the biggest Sessions a sample names.
const TOP: usize = 3;

/// The limits on every Session together; `None` is no limit (`max`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub high: Option<u64>,
    pub max: Option<u64>,
    pub swap_max: Option<u64>,
}

/// The daemon's cgroup split, set up by [`Cgroups::setup`].
#[derive(Debug)]
pub struct Cgroups {
    sessions: PathBuf,
    /// Prefix of this run's leaves.
    run: String,
}

impl Cgroups {
    /// Split the cgroup this process runs in, as the module doc says, and move this process into
    /// `daemon/` (already there under `DelegateSubgroup=daemon`). Fails when the cgroup is not
    /// writable (no `Delegate=yes`) or other processes share it; the Sessions then share the
    /// daemon's cgroup, as before.
    pub fn setup(limits: Limits) -> Result<Self, String> {
        let text = fs::read_to_string("/proc/self/cgroup").map_err(|e| format!("/proc/self/cgroup: {e}"))?;
        let rel = own_cgroup(&text).ok_or("not on cgroup v2 (no 0:: line in /proc/self/cgroup)")?;
        let own = Path::new("/sys/fs/cgroup").join(rel.trim_start_matches('/'));
        Self::setup_at(&own, limits, std::process::id())
    }

    /// [`Cgroups::setup`] with this process's cgroup at `own` and its pid `pid`.
    pub fn setup_at(own: &Path, limits: Limits, pid: u32) -> Result<Self, String> {
        let root = if own.file_name().is_some_and(|n| n == DAEMON_GROUP) {
            own.parent().ok_or("the cgroup has no parent")?.to_path_buf()
        } else {
            own.to_path_buf()
        };
        let daemon = root.join(DAEMON_GROUP);
        let sessions = root.join(SESSIONS_GROUP);
        mkdir(&daemon)?;
        if own != daemon {
            write(&daemon.join("cgroup.procs"), &pid.to_string())?;
        }
        // cgroup v2: a group that hands controllers to its children holds no processes itself.
        let left = fs::read_to_string(root.join("cgroup.procs")).unwrap_or_default();
        let others: Vec<&str> = left.split_whitespace().collect();
        if !others.is_empty() {
            return Err(format!(
                "{} still holds processes {} (started before the split); Sessions share the daemon's cgroup",
                root.display(),
                others.join(" ")
            ));
        }
        let available = fs::read_to_string(root.join("cgroup.controllers")).unwrap_or_default();
        if !available.split_whitespace().any(|c| c == "memory") {
            return Err(format!(
                "no memory controller in {} (is Delegate=yes set?)",
                root.display()
            ));
        }
        enable(&root, &available)?;
        // Best effort: protection only, and a kernel without memory.low still works.
        let _ = write(&daemon.join("memory.low"), &DAEMON_LOW.to_string());
        mkdir(&sessions)?;
        enable(&sessions, &available)?;
        write(&sessions.join("memory.high"), &limit_value(limits.high))?;
        write(&sessions.join("memory.max"), &limit_value(limits.max))?;
        // No swap accounting (swapaccount=0) leaves no memory.swap.max: then there is none to set.
        let swap = sessions.join("memory.swap.max");
        if swap.exists() || limits.swap_max.is_some() {
            write(&swap, &limit_value(limits.swap_max))?;
        }
        Ok(Self {
            sessions,
            run: pid.to_string(),
        })
    }

    /// The group every Session's leaf sits in.
    pub fn sessions_dir(&self) -> &Path {
        &self.sessions
    }

    fn leaf(&self, id: SessionId) -> PathBuf {
        self.sessions.join(format!("{}-{id}", self.run))
    }

    /// Wrap a new Session's command so its shell starts in the Session's own leaf: `sh` writes
    /// its pid into the leaf, then execs the command, keeping that pid. A leaf that cannot be
    /// made leaves the command as it is (the Session then runs in the daemon's group, which the
    /// log says); a failed move says so in the Session's Terminal and starts the shell anyway.
    pub fn wrap(&self, id: SessionId, argv: &mut Vec<OsString>) {
        let leaf = self.leaf(id);
        if let Err(e) = mkdir(&leaf) {
            eprintln!("cgroup: Session {id} runs in the daemon's cgroup: {e}");
            return;
        }
        let procs = leaf.join("cgroup.procs");
        let mut wrapped: Vec<OsString> = vec![
            "/bin/sh".into(),
            "-c".into(),
            WRAP_SCRIPT.into(),
            procs.into_os_string(),
        ];
        wrapped.append(argv);
        *argv = wrapped;
    }

    /// Remove a Session's leaf once it has exited. Still populated (a process outlived the
    /// hangup) it stays, and [`Cgroups::sweep`] removes it once empty.
    pub fn release(&self, id: SessionId) {
        let _ = fs::remove_dir(self.leaf(id));
    }

    /// Remove every empty leaf that is not one of `live`'s: what earlier runs left, and leaves
    /// [`Cgroups::release`] found still populated.
    pub fn sweep(&self, live: &[SessionId]) {
        let keep: HashSet<String> = live.iter().map(|id| format!("{}-{id}", self.run)).collect();
        let Ok(dir) = fs::read_dir(&self.sessions) else { return };
        for entry in dir.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if !path.is_dir() || keep.contains(&name) {
                continue;
            }
            let events = fs::read_to_string(path.join("cgroup.events")).unwrap_or_default();
            if !populated(&events) {
                let _ = fs::remove_dir(&path);
            }
        }
    }

    /// The Sessions' memory now, together and per Session (this run's leaves), and how much they
    /// stall on it. `None` when the group cannot be read.
    pub fn sample(&self) -> Option<HostPressure> {
        let read = |p: &Path, f: &str| fs::read_to_string(p.join(f)).ok();
        let mem = read(&self.sessions, "memory.current")?.trim().parse().ok()?;
        let high = read(&self.sessions, "memory.high").and_then(|v| parse_limit(&v));
        let max = read(&self.sessions, "memory.max").and_then(|v| parse_limit(&v));
        let swap = read(&self.sessions, "memory.swap.current")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        let swap_max = read(&self.sessions, "memory.swap.max").and_then(|v| parse_limit(&v));
        let (some, full) = read(&self.sessions, "memory.pressure")
            .map(|v| parse_psi(&v))
            .unwrap_or((0.0, 0.0));
        let prefix = format!("{}-", self.run);
        let mut top: Vec<SessionMemory> = fs::read_dir(&self.sessions)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let session_id = name.strip_prefix(&prefix)?.parse().ok()?;
                let path = entry.path();
                Some(SessionMemory {
                    session_id,
                    mem: read(&path, "memory.current")?.trim().parse().ok()?,
                    swap: read(&path, "memory.swap.current")
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0),
                })
            })
            .collect();
        top.sort_by(|a, b| b.mem.cmp(&a.mem));
        top.truncate(TOP);
        Some(HostPressure {
            level: level(mem, high.or(max), swap, swap_max, some, full),
            mem,
            high,
            max,
            swap,
            swap_max,
            some,
            full,
            top,
        })
    }
}

/// `$0` is the leaf's `cgroup.procs`, `$@` the Session's command.
const WRAP_SCRIPT: &str = r#"{ echo $$ > "$0"; } 2>/dev/null || echo "sidebar-termd: this Session could not move into its cgroup ($0); it shares the daemon's" >&2; exec "$@""#;

/// The cgroup v2 path in `/proc/<pid>/cgroup`'s text (its `0::` line).
pub fn own_cgroup(text: &str) -> Option<&str> {
    text.lines().find_map(|l| l.strip_prefix("0::")).map(str::trim).filter(|p| p.starts_with('/'))
}

/// How hard the Sessions press, from their memory and stall times. Throttled: at the limit
/// where the kernel throttles (within 2%), or all of them stalled a fifth of the time. Tight:
/// past 85% of it, swap nearly full, or stalls noticeable.
pub fn level(mem: u64, limit: Option<u64>, swap: u64, swap_max: Option<u64>, some: f32, full: f32) -> PressureLevel {
    let at = |limit: u64, percent: u64| mem.saturating_mul(100) >= limit.saturating_mul(percent);
    if full >= 20.0 || limit.is_some_and(|l| at(l, 98)) {
        PressureLevel::Throttled
    } else if some >= 10.0
        || full >= 5.0
        || limit.is_some_and(|l| at(l, 85))
        || swap_max.is_some_and(|s| s > 0 && swap.saturating_mul(10) >= s.saturating_mul(9))
    {
        PressureLevel::Tight
    } else {
        PressureLevel::Ok
    }
}

/// A `memory.high` / `memory.max` value: bytes, or `None` for `max` (no limit).
pub fn parse_limit(v: &str) -> Option<u64> {
    v.trim().parse().ok()
}

fn limit_value(v: Option<u64>) -> String {
    v.map_or_else(|| "max".to_owned(), |b| b.to_string())
}

/// The avg10 of the "some" and "full" lines of a `memory.pressure` file.
pub fn parse_psi(text: &str) -> (f32, f32) {
    let avg10 = |kind: &str| {
        text.lines()
            .find(|l| l.starts_with(kind))
            .and_then(|l| l.split_whitespace().find_map(|f| f.strip_prefix("avg10=")))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0)
    };
    (avg10("some"), avg10("full"))
}

/// A size as systemd writes it: bytes, or with a K / M / G / T suffix (powers of 1024).
pub fn parse_size(v: &str) -> Option<u64> {
    let v = v.trim();
    let (num, shift) = match v.chars().last()?.to_ascii_uppercase() {
        'K' => (&v[..v.len() - 1], 10),
        'M' => (&v[..v.len() - 1], 20),
        'G' => (&v[..v.len() - 1], 30),
        'T' => (&v[..v.len() - 1], 40),
        _ => (v, 0),
    };
    let n: f64 = num.trim().parse().ok().filter(|n: &f64| n.is_finite() && *n >= 0.0)?;
    Some((n * (1u64 << shift) as f64) as u64)
}

fn populated(events: &str) -> bool {
    events.lines().any(|l| l.trim() == "populated 1")
}

fn mkdir(dir: &Path) -> Result<(), String> {
    match fs::create_dir(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(format!("{}: {e}", dir.display())),
    }
}

fn write(file: &Path, value: &str) -> Result<(), String> {
    fs::write(file, value).map_err(|e| format!("{}: {e}", file.display()))
}

/// Hand `CONTROLLERS` (those `available`) to `group`'s children. One at a time: a controller
/// the kernel refuses must not keep memory from being enabled.
fn enable(group: &Path, available: &str) -> Result<(), String> {
    let file = group.join("cgroup.subtree_control");
    for c in CONTROLLERS {
        if !available.split_whitespace().any(|a| a == c) {
            continue;
        }
        if let Err(e) = write(&file, &format!("+{c}")) {
            if c == "memory" {
                return Err(e);
            }
            eprintln!("cgroup: {c} controller not enabled: {e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sidebar-cgroup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_v2_path_is_read_from_proc() {
        assert_eq!(
            own_cgroup("0::/user.slice/user-1000.slice/user@1000.service/app.slice/sidebar-termd.service\n"),
            Some("/user.slice/user-1000.slice/user@1000.service/app.slice/sidebar-termd.service")
        );
        assert_eq!(own_cgroup("12:memory:/x\n0::/a/daemon\n"), Some("/a/daemon"));
        assert_eq!(own_cgroup("12:memory:/x\n"), None);
    }

    #[test]
    fn sizes_parse_as_systemd_writes_them() {
        assert_eq!(parse_size("9G"), Some(9 << 30));
        assert_eq!(parse_size("512m"), Some(512 << 20));
        assert_eq!(parse_size("1.5G"), Some(3 << 29));
        assert_eq!(parse_size("4096"), Some(4096));
        assert_eq!(parse_size("G"), None);
        assert_eq!(parse_size("-1G"), None);
        assert_eq!(parse_size("lots"), None);
        assert_eq!(parse_limit("max\n"), None);
        assert_eq!(parse_limit("9663676416\n"), Some(9663676416));
    }

    #[test]
    fn pressure_lines_parse() {
        let text = "some avg10=44.63 avg60=38.58 avg300=20.00 total=3944727308\nfull avg10=42.40 avg60=36.69 avg300=19.06 total=3781834110\n";
        assert_eq!(parse_psi(text), (44.63, 42.40));
        assert_eq!(parse_psi(""), (0.0, 0.0));
    }

    #[test]
    fn the_level_follows_the_limit_swap_and_stalls() {
        let g = 1u64 << 30;
        use PressureLevel::*;
        // What bennet showed during a stall: at memory.high, swap full, stalled 42% of the time.
        assert_eq!(level(9 * g, Some(9 * g), 2 * g, Some(2 * g), 44.0, 42.0), Throttled);
        assert_eq!(level(9 * g - g / 100, Some(9 * g), 0, None, 0.0, 0.0), Throttled);
        assert_eq!(level(8 * g, Some(9 * g), 0, None, 0.0, 0.0), Tight);
        assert_eq!(level(2 * g, Some(9 * g), 2 * g, Some(2 * g), 0.0, 0.0), Tight);
        assert_eq!(level(2 * g, Some(9 * g), 0, Some(2 * g), 12.0, 1.0), Tight);
        assert_eq!(level(2 * g, Some(9 * g), 0, Some(2 * g), 1.0, 0.0), Ok);
        assert_eq!(level(20 * g, None, 0, None, 0.0, 0.0), Ok, "no limit, no stalls");
        assert_eq!(level(0, None, 0, None, 0.0, 25.0), Throttled);
    }

    /// The split, against a directory standing in for the delegated cgroup.
    #[test]
    fn setup_splits_the_group_and_sets_the_limits() {
        let root = tmp("setup");
        fs::write(root.join("cgroup.controllers"), "cpu memory pids\n").unwrap();
        fs::write(root.join("cgroup.procs"), "").unwrap();
        // The kernel makes these in each new group; here only what setup reads back.
        let limits = Limits { high: Some(9 << 30), max: Some(10 << 30), swap_max: None };
        let cg = Cgroups::setup_at(&root, limits, 885).unwrap();
        assert_eq!(fs::read_to_string(root.join("daemon/cgroup.procs")).unwrap(), "885");
        assert_eq!(fs::read_to_string(root.join("daemon/memory.low")).unwrap(), DAEMON_LOW.to_string());
        // One write per controller; the last one stays in the stand-in file.
        assert_eq!(fs::read_to_string(root.join("cgroup.subtree_control")).unwrap(), "+cpu");
        assert_eq!(fs::read_to_string(root.join("sessions/memory.high")).unwrap(), (9u64 << 30).to_string());
        assert_eq!(fs::read_to_string(root.join("sessions/memory.max")).unwrap(), (10u64 << 30).to_string());
        assert!(!root.join("sessions/memory.swap.max").exists(), "no swap limit asked, none there");

        let mut argv: Vec<OsString> = vec!["/bin/zsh".into(), "-l".into()];
        cg.wrap(7, &mut argv);
        assert_eq!(argv[0], "/bin/sh");
        assert_eq!(argv[3], root.join("sessions/885-7/cgroup.procs").into_os_string());
        assert_eq!(&argv[4..], &[OsString::from("/bin/zsh"), "-l".into()]);
        assert!(root.join("sessions/885-7").is_dir());

        // A sample names this run's Sessions by id, most memory first.
        fs::write(root.join("sessions/memory.current"), "5000\n").unwrap();
        fs::write(root.join("sessions/memory.pressure"), "some avg10=12.00 x\nfull avg10=3.00 x\n").unwrap();
        fs::write(root.join("sessions/885-7/memory.current"), "1000\n").unwrap();
        cg.wrap(8, &mut vec![]);
        fs::write(root.join("sessions/885-8/memory.current"), "3000\n").unwrap();
        fs::create_dir(root.join("sessions/1-3")).unwrap(); // an earlier run's
        fs::write(root.join("sessions/1-3/memory.current"), "9000\n").unwrap();
        let s = cg.sample().unwrap();
        assert_eq!(s.mem, 5000);
        assert_eq!((s.some, s.full), (12.0, 3.0));
        assert_eq!(s.level, PressureLevel::Tight);
        assert_eq!(s.top.iter().map(|t| t.session_id).collect::<Vec<_>>(), [8, 7]);

        // The sweep removes empty leaves that are not live; a populated one stays.
        fs::remove_file(root.join("sessions/1-3/memory.current")).unwrap();
        fs::write(root.join("sessions/885-8/cgroup.events"), "populated 1\n").unwrap();
        fs::remove_file(root.join("sessions/885-7/memory.current")).unwrap();
        fs::remove_file(root.join("sessions/885-7/cgroup.procs")).ok();
        fs::remove_file(root.join("sessions/885-8/memory.current")).unwrap();
        cg.sweep(&[]);
        assert!(!root.join("sessions/1-3").exists());
        assert!(!root.join("sessions/885-7").exists());
        assert!(root.join("sessions/885-8").exists(), "still populated");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn setup_refuses_a_group_others_still_run_in() {
        let root = tmp("busy");
        fs::write(root.join("cgroup.controllers"), "memory\n").unwrap();
        fs::write(root.join("cgroup.procs"), "1234\n5678\n").unwrap();
        let err = Cgroups::setup_at(&root, Limits::default(), 885).unwrap_err();
        assert!(err.contains("1234 5678"), "{err}");
        fs::remove_dir_all(&root).unwrap();

        let root = tmp("nomem");
        fs::write(root.join("cgroup.controllers"), "cpu pids\n").unwrap();
        fs::write(root.join("cgroup.procs"), "").unwrap();
        assert!(Cgroups::setup_at(&root, Limits::default(), 885).unwrap_err().contains("Delegate=yes"));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn under_delegate_subgroup_the_daemon_is_already_in_place() {
        let root = tmp("subgroup");
        fs::write(root.join("cgroup.controllers"), "memory\n").unwrap();
        fs::write(root.join("cgroup.procs"), "").unwrap();
        fs::create_dir(root.join("daemon")).unwrap();
        let cg = Cgroups::setup_at(&root.join("daemon"), Limits::default(), 885).unwrap();
        assert!(!root.join("daemon/cgroup.procs").exists(), "no move needed");
        assert_eq!(cg.sessions_dir(), root.join("sessions"));
        assert_eq!(fs::read_to_string(root.join("sessions/memory.high")).unwrap(), "max");
        fs::remove_dir_all(&root).unwrap();
    }
}
