//! The OS reads behind `detect`: the members of a process group and, per pid, its comm,
//! executable path, argv, environment and cwd. One backend per OS, chosen at compile time,
//! with one meaning: `macos.rs` (libproc and `sysctl`) and `linux.rs` (`/proc`). The
//! classification of what is read (`detect::process`) is shared and pure. OWNER: detection agent.
//!
//! Every read is best-effort and never panics: a vanished pid, a zombie or another user's
//! process reads as `None` / empty. What each backend can read of another user's process (a
//! `sudo`'d Foreground process) differs and is documented on each function.

use libc::pid_t;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
pub use linux::{argv, argv_env, cwd, exe_path, group_members, short_info};
#[cfg(target_os = "macos")]
pub use macos::{argv, argv_env, cwd, exe_path, group_members, short_info};

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("detect::os has no backend for this OS (see detect/os/mod.rs)");

/// One live (non-zombie) process, as far as the probe cares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: pid_t,
    /// Parent pid.
    pub ppid: pid_t,
    /// The kernel's short process name: the exec'd file's name truncated to `MAXCOMLEN`
    /// (16 bytes on macOS, 15 on Linux). Whose name differs: macOS resolves symlinks first
    /// (a `claude -> versions/2.1.267` symlink reads as `2.1.267`), Linux keeps the name as
    /// exec'd (`claude`).
    pub comm: String,
}

pub(crate) fn non_empty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Live checks that hold for both backends; each backend tests what is its own.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::testutil::{spawn_in_own_group, TempDir, SLEEPER_ENV};

    #[test]
    fn reads_a_real_process_group() {
        let dir = TempDir::new("proc");
        let mut child = spawn_in_own_group("/bin/sleep", &["30"], dir.path());
        let pid = child.pid();

        assert_eq!(group_members(pid), vec![pid]);
        let info = short_info(pid).expect("short info");
        assert_eq!(
            info,
            Proc {
                pid,
                ppid: std::process::id() as pid_t,
                comm: "sleep".into()
            }
        );
        // `/bin/sleep` is a real file on macOS; on merged-usr Linux `/bin` links to `/usr/bin`.
        let sleep = std::fs::canonicalize("/bin/sleep").unwrap();
        assert_eq!(exe_path(pid).as_deref(), sleep.to_str());
        assert_eq!(cwd(pid).as_deref(), Some(dir.canonical_str()));
        assert_eq!(argv(pid).unwrap(), ["/bin/sleep", "30"]);
        let (argv, env) = argv_env(pid).unwrap();
        assert_eq!(argv, ["/bin/sleep", "30"]);
        if cfg!(target_os = "macos") {
            // A platform binary: the kernel withholds its environment. Other binaries' is parsed
            // as in `procargs2_environment_stops_at_the_apple_strings` (checked live by
            // `resume_live`).
            assert!(env.is_empty(), "{env:?}");
        } else {
            assert!(env.contains(&format!("{SLEEPER_ENV}=1")), "{env:?}");
        }

        child.kill();
        assert!(short_info(pid).is_none());
        assert!(group_members(pid).is_empty());
        assert!(cwd(pid).is_none());
    }

    #[test]
    fn bad_pids_read_as_nothing() {
        for pid in [0, -1, i32::MAX] {
            assert!(group_members(pid).is_empty());
            assert!(short_info(pid).is_none());
            assert!(exe_path(pid).is_none());
            assert!(cwd(pid).is_none());
            assert!(argv(pid).is_none());
            assert!(argv_env(pid).is_none());
        }
        // pid 1 (launchd / init) is root's: its name is readable, its cwd is not.
        assert!(short_info(1).is_some());
        // SAFETY: plain query.
        if unsafe { libc::geteuid() } != 0 {
            assert!(cwd(1).is_none());
        }
    }
}
