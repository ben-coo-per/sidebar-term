//! Facts about a Session derived from the OS and the filesystem, never from the pty stream.
//! OWNER: detection agent. See docs/research/agent-detection.md and docs/research/cwd-git.md.

pub mod git;
pub mod process;
#[cfg(test)]
pub(crate) mod testutil;

use crate::model::{AgentKind, ProbeTarget, SessionInfo};
use std::path::Path;

/// Compute the current `SessionInfo` for one Session. Pure w.r.t. app state: reads only
/// libproc and the filesystem. Must be fast (target: well under 1 ms per call when the
/// git dir is cached by the OS) and must never panic.
///
/// - The Foreground process group is `fg_pgid`; when that is `None`, the shell's pid, or a
///   group that can no longer be read (it just exited), the shell is foreground.
/// - Every member of the group is classified: the agent is not always the leader (Codex's npm
///   launcher `node` leads and spawns the native `codex`; Gemini relaunches itself as a child).
/// - `cwd` is the agent member's if any, else the group leader's, falling back to the shell's
///   cwd when unreadable (EPERM for another uid, e.g. `sudo`). `foreground` is the agent's
///   command name (`claude` / `codex` / `gemini`) if any, else the group leader's `comm`.
/// - A remote hop in the group (`ssh`, `docker exec`, ...) suppresses `git`.
pub fn probe(target: &ProbeTarget) -> SessionInfo {
    let shell = target.shell_pid;
    let fg = target.fg_pgid.filter(|&pgid| pgid > 0 && pgid != shell);

    let (pgid, members, shell_is_foreground) = match fg.map(|pgid| (pgid, read_group(pgid))) {
        Some((pgid, members)) if !members.is_empty() => (pgid, members, false),
        _ => {
            let mut members = read_group(shell);
            if members.is_empty() {
                members.extend(Member::read(shell));
            }
            (shell, members, true)
        }
    };

    // Leader first, then ascending pid: the order in which members describe the group.
    let mut members = members;
    members.sort_by_key(|m| (m.pid != pgid, m.pid));

    // Prefer a member identified by its own name or executable (the native `codex`) over one
    // identified by argv (its `node` launcher); among equals, the leader, then the oldest.
    let agent_member = members
        .iter()
        .find(|m| m.agent_by_name().is_some())
        .or_else(|| members.iter().find(|m| m.agent().is_some()));
    let focus = agent_member.or(members.first());

    let remote = members.iter().any(|m| process::is_remote(&m.comm, &m.argv));
    let cwd = focus
        .and_then(|m| process::cwd(m.pid))
        .or_else(|| process::cwd(shell));
    let git = if remote {
        None
    } else {
        cwd.as_deref().and_then(|c| git::resolve(Path::new(c)))
    };

    let agent = agent_member.and_then(Member::agent);
    // An agent's own `comm` is unhelpful: native Claude Code's is its version ("2.1.267", the
    // file the `claude` symlink resolves to), npm installs' is "node". Name the agent instead.
    let foreground = match agent {
        Some(kind) => Some(agent_command(kind).to_owned()),
        None => focus.map(|m| m.comm.clone()),
    };

    SessionInfo {
        session_id: target.session_id,
        foreground,
        shell_is_foreground,
        agent,
        cwd,
        remote,
        git,
    }
}

/// The command a user types to start `kind`.
fn agent_command(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Claude => "claude",
        AgentKind::Codex => "codex",
        AgentKind::Gemini => "gemini",
    }
}

/// One member of the Foreground process group with what classification needs.
struct Member {
    pid: i32,
    comm: String,
    /// Resolved executable path; empty if unreadable.
    path: String,
    /// Read only for comms where it matters (`process::needs_argv`); empty otherwise.
    argv: Vec<String>,
}

impl Member {
    fn read(pid: i32) -> Option<Self> {
        let p = process::short_info(pid)?;
        let path = process::exe_path(pid).unwrap_or_default();
        let argv = if process::needs_argv(&p.comm) {
            process::argv(pid).unwrap_or_default()
        } else {
            Vec::new()
        };
        Some(Self {
            pid,
            comm: p.comm,
            path,
            argv,
        })
    }

    fn agent(&self) -> Option<AgentKind> {
        process::classify_agent(&self.comm, &self.path, &self.argv)
    }

    fn agent_by_name(&self) -> Option<AgentKind> {
        process::classify_agent::<&str>(&self.comm, &self.path, &[])
    }
}

fn read_group(pgid: i32) -> Vec<Member> {
    process::group_members(pgid)
        .into_iter()
        .filter_map(Member::read)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::testutil::{
        fake_binary, git, init_repo, spawn_in_own_group, wait_until, TempDir, SLEEPER_ARGS,
    };
    use std::time::Instant;

    fn target(shell_pid: i32, fg_pgid: Option<i32>) -> ProbeTarget {
        ProbeTarget {
            session_id: 7,
            shell_pid,
            fg_pgid,
        }
    }

    #[test]
    fn shell_at_the_prompt() {
        let tmp = TempDir::new("probe-shell");
        let mut shell = spawn_in_own_group("/bin/sleep", &["30"], tmp.path());
        for fg in [Some(shell.pid()), None] {
            let info = probe(&target(shell.pid(), fg));
            assert_eq!(
                info,
                SessionInfo {
                    session_id: 7,
                    foreground: Some("sleep".into()),
                    shell_is_foreground: true,
                    agent: None,
                    cwd: Some(tmp.canonical_str().into()),
                    remote: false,
                    git: None,
                }
            );
        }
        shell.kill();
        // Shell gone: nothing to report, and no panic.
        let info = probe(&target(shell.pid(), None));
        assert_eq!(info, SessionInfo::empty(7));
    }

    #[test]
    fn foreground_job_in_a_repo_subdirectory() {
        let tmp = TempDir::new("probe-job");
        let repo = init_repo(tmp.path(), "myrepo");
        let sub = repo.join("src");
        std::fs::create_dir_all(&sub).unwrap();
        let shell = spawn_in_own_group("/bin/sleep", &["30"], tmp.path());
        let job = spawn_in_own_group("/bin/sleep", &["30"], &sub);

        let info = probe(&target(shell.pid(), Some(job.pid())));
        assert!(!info.shell_is_foreground);
        assert_eq!(info.foreground.as_deref(), Some("sleep"));
        assert_eq!(info.agent, None);
        assert!(!info.remote);
        assert_eq!(info.cwd.as_deref(), Some(tmp.canon("myrepo/src").as_str()));
        let git = info.git.expect("git info");
        assert_eq!(git.repo_name, "myrepo");
        assert_eq!(git.branch.as_deref(), Some("main"));
        assert_eq!(git.worktree_root, tmp.canon("myrepo"));
    }

    #[test]
    fn agent_that_is_not_the_group_leader() {
        let tmp = TempDir::new("probe-agent");
        let repo = init_repo(tmp.path(), "myrepo");
        git(&repo, &["worktree", "add", "-q", "-b", "feat", "../wt"]);
        let wt = tmp.path().join("wt");
        let claude = fake_binary(tmp.path(), "claude");
        let shell = spawn_in_own_group("/bin/sleep", &["30"], tmp.path());
        // `; true` keeps sh from exec'ing: sh stays the leader, claude is its child.
        let script = format!("'{}' {}; true", claude.display(), SLEEPER_ARGS.join(" "));
        let job = spawn_in_own_group("/bin/sh", &["-c", &script], &wt);
        let has_claude = || {
            let members = process::group_members(job.pid());
            members.len() == 2
                && members
                    .iter()
                    .any(|&p| process::short_info(p).is_some_and(|p| p.comm == "claude"))
        };
        assert!(wait_until(has_claude), "claude child never appeared");

        let info = probe(&target(shell.pid(), Some(job.pid())));
        assert_eq!(info.agent, Some(AgentKind::Claude));
        assert_eq!(info.foreground.as_deref(), Some("claude"));
        assert!(!info.shell_is_foreground);
        assert!(!info.remote);
        assert_eq!(info.cwd.as_deref(), Some(tmp.canon("wt").as_str()));
        let git = info.git.expect("git info");
        assert_eq!(git.worktree_name.as_deref(), Some("wt"));
        assert_eq!(git.branch.as_deref(), Some("feat"));
        assert_eq!(git.common_dir, tmp.canon("myrepo/.git"));
    }

    #[test]
    fn remote_hop_suppresses_git() {
        let tmp = TempDir::new("probe-remote");
        let repo = init_repo(tmp.path(), "myrepo");
        let ssh = fake_binary(tmp.path(), "ssh");
        let shell = spawn_in_own_group("/bin/sleep", &["30"], &repo);
        let job = spawn_in_own_group(&ssh, &SLEEPER_ARGS, &repo);
        assert!(wait_until(
            || process::short_info(job.pid()).is_some_and(|p| p.comm == "ssh")
        ));

        let info = probe(&target(shell.pid(), Some(job.pid())));
        assert_eq!(info.foreground.as_deref(), Some("ssh"));
        assert!(info.remote);
        assert_eq!(info.git, None);
        assert_eq!(info.cwd.as_deref(), Some(tmp.canon("myrepo").as_str()));
    }

    #[test]
    fn unreadable_foreground_falls_back_to_the_shell() {
        let tmp = TempDir::new("probe-fallback");
        let shell = spawn_in_own_group("/bin/sleep", &["30"], tmp.path());

        // Foreground group that no longer exists: treat the shell as foreground.
        let mut gone = spawn_in_own_group("/bin/sleep", &["30"], tmp.path());
        let gone_pid = gone.pid();
        gone.kill();
        let info = probe(&target(shell.pid(), Some(gone_pid)));
        assert!(info.shell_is_foreground);
        assert_eq!(info.foreground.as_deref(), Some("sleep"));
        assert_eq!(info.cwd.as_deref(), Some(tmp.canonical_str()));

        // Foreground process of another uid (like `sudo`): its cwd is EPERM, so the shell's
        // cwd is reported. launchd (pid 1, pgid 1) stands in for a root-owned process.
        let info = probe(&target(shell.pid(), Some(1)));
        assert!(!info.shell_is_foreground);
        assert_eq!(info.foreground.as_deref(), Some("launchd"));
        assert_eq!(info.cwd.as_deref(), Some(tmp.canonical_str()));
    }

    #[test]
    fn probe_is_well_under_a_millisecond() {
        let tmp = TempDir::new("probe-timing");
        let repo = init_repo(tmp.path(), "myrepo");
        git(&repo, &["worktree", "add", "-q", "-b", "feat", "../wt"]);
        let deep = tmp.path().join("wt/a/b/c/d");
        std::fs::create_dir_all(&deep).unwrap();
        let shell = spawn_in_own_group("/bin/sleep", &["30"], &repo);
        let job = spawn_in_own_group("/bin/sleep", &["30"], &deep);

        for (label, t) in [
            ("shell at prompt", target(shell.pid(), None)),
            ("job in worktree", target(shell.pid(), Some(job.pid()))),
        ] {
            let first = probe(&t);
            assert!(first.git.is_some(), "{label}: {first:?}");
            const N: u32 = 500;
            let start = Instant::now();
            for _ in 0..N {
                assert_eq!(probe(&t), first);
            }
            let avg = start.elapsed() / N;
            println!("probe ({label}): {avg:?} per call over {N} calls");
            assert!(
                avg.as_micros() < 500,
                "{label}: probe took {avg:?} on average"
            );
        }
    }

    #[test]
    fn native_claude_layout_even_after_its_binary_is_deleted() {
        // ~/.local/bin/claude -> ~/.local/share/claude/versions/<version>, as installed.
        let tmp = TempDir::new("probe-native-claude");
        let versions = tmp.path().join("share/claude/versions");
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&versions).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let exe = versions.join("2.1.999");
        // A real copy (not a hard link), so deleting it really orphans the running image.
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        std::os::unix::fs::symlink(&exe, bin.join("claude")).unwrap();
        let shell = spawn_in_own_group("/bin/sleep", &["30"], tmp.path());
        let job = spawn_in_own_group(bin.join("claude"), &SLEEPER_ARGS, tmp.path());
        // The kernel's comm is the resolved file name, not the symlink's.
        assert!(wait_until(
            || process::short_info(job.pid()).is_some_and(|p| p.comm == "2.1.999")
        ));

        let t = target(shell.pid(), Some(job.pid()));
        let info = probe(&t);
        assert_eq!(info.agent, Some(AgentKind::Claude));
        assert_eq!(info.foreground.as_deref(), Some("claude"));

        // The auto-updater removes old versions while sessions still run them.
        std::fs::remove_file(&exe).unwrap();
        assert_eq!(process::exe_path(job.pid()), None);
        let info = probe(&t);
        assert_eq!(info.agent, Some(AgentKind::Claude));
        assert_eq!(info.foreground.as_deref(), Some("claude"));
    }

    /// Classify whatever agents are really running on this machine.
    /// Run with `cargo test real_agents -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_agents() {
        let out = std::process::Command::new("pgrep")
            .args(["-x", "claude|codex|gemini|node"])
            .output()
            .unwrap();
        for pid in String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.trim().parse::<i32>().ok())
        {
            let Some(m) = Member::read(pid) else { continue };
            let pgid = unsafe { libc::getpgid(pid) };
            let info = probe(&target(std::process::id() as i32, Some(pgid)));
            let start = Instant::now();
            for _ in 0..200 {
                probe(&target(std::process::id() as i32, Some(pgid)));
            }
            println!(
                "pid {pid} comm={} path={} argv={:?}\n  -> agent={:?}; probe(pgid {pgid}) = {info:?} in {:?}",
                m.comm,
                m.path,
                m.argv.iter().take(3).collect::<Vec<_>>(),
                m.agent(),
                start.elapsed() / 200
            );
        }
    }
}
