//! Shared types crossing the Rust <-> webview boundary. Mirrored by `src/lib/types.ts`.
//! CONTRACT: owned by the tech lead. Change only by agreement; keep the TS mirror in sync.

use serde::Serialize;

/// Identity of one Session (one shell on one pty). Allocated by `SessionManager`, never reused.
pub type SessionId = u32;

/// A known coding agent that can be a Session's Foreground process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
    Gemini,
}

/// Repo / Worktree / branch facts for a Session's cwd. Drives the Badge.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitInfo {
    /// Display name of the repo: basename of the main worktree's directory.
    pub repo_name: String,
    /// Canonical path of the shared git dir (`git rev-parse --git-common-dir`, absolutised).
    /// This is the repo's identity: every Worktree of one repo has the same value.
    pub common_dir: String,
    /// Canonical top-level directory of the Worktree the cwd is in.
    pub worktree_root: String,
    /// `None` for the main Worktree; the linked Worktree's name (basename of
    /// `<common>/worktrees/<name>`) otherwise.
    pub worktree_name: Option<String>,
    /// Current branch without `refs/heads/`; `None` when HEAD is detached.
    pub branch: Option<String>,
    /// First 7 hex chars of HEAD when detached; `None` on a branch.
    pub head_short: Option<String>,
}

/// Everything the sidebar knows about a Session, recomputed by the monitor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: SessionId,
    /// `comm` of the process that best describes the Foreground process group
    /// (the agent if one is found, else the group leader), e.g. "zsh", "vim", "claude".
    pub foreground: Option<String>,
    /// True when the shell itself holds the tty (idle at a prompt).
    pub shell_is_foreground: bool,
    /// Set when the Foreground process group contains a known coding agent.
    pub agent: Option<AgentKind>,
    /// Canonical cwd of the Foreground process (falls back to the shell's cwd).
    pub cwd: Option<String>,
    /// True when the Foreground process is a remote hop (ssh, mosh, docker exec/run,
    /// kubectl exec, et, ...). `cwd` and `git` then describe this Mac, not the far side,
    /// and the UI shows a remote marker instead of a Badge.
    pub remote: bool,
    pub git: Option<GitInfo>,
}

impl SessionInfo {
    #[allow(dead_code)] // used by tests and as a safe default
    pub fn empty(session_id: SessionId) -> Self {
        Self {
            session_id,
            foreground: None,
            shell_is_foreground: true,
            agent: None,
            cwd: None,
            remote: false,
            git: None,
        }
    }
}

/// Payload of the `session-exit` event.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionExit {
    pub session_id: SessionId,
    pub code: Option<i32>,
}

/// What the monitor needs to probe one live Session.
#[derive(Clone, Copy, Debug)]
pub struct ProbeTarget {
    pub session_id: SessionId,
    /// Pid of the shell spawned on the pty (also its session id after setsid).
    pub shell_pid: i32,
    /// Foreground process group of the pty (`tcgetpgrp` on the master), if readable.
    pub fg_pgid: Option<i32>,
}

/// Event names. Frontend listens with `listen(EVENT_SESSION_INFO, ...)`.
pub const EVENT_SESSION_INFO: &str = "session-info";
pub const EVENT_SESSION_EXIT: &str = "session-exit";
