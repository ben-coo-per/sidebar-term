//! Shared types crossing the Rust <-> webview boundary. Mirrored by `src/lib/types.ts`.
//! CONTRACT: owned by the tech lead. Change only by agreement; keep the TS mirror in sync.

use serde::{Deserialize, Serialize};

/// Identity of one Session (one shell on one pty). Allocated by `SessionManager`, never reused.
pub type SessionId = u32;

/// A known coding agent that can be a Session's Foreground process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// One process in an `ActivitySnapshot`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityProcess {
    pub pid: i32,
    /// Executable file name (`WindowServer`, `cargo`); an agent's command name (`claude`) when
    /// the executable is a coding agent's.
    pub name: String,
    /// Percent of one core over the last sample interval, as Activity Monitor shows it
    /// (a process using two cores reads 200).
    pub cpu: f32,
    /// Resident memory in bytes.
    pub mem: u64,
    /// The Session whose shell this process is or descends from; `None` for everything else.
    pub session_id: Option<SessionId>,
}

/// Totals over every process of one Session.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySession {
    pub session_id: SessionId,
    pub cpu: f32,
    pub mem: u64,
    pub processes: u32,
}

/// CPU and memory of the whole Mac, with each Session's share. Payload of `activity`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySnapshot {
    /// Logical cores: the machine's capacity is `cpu_count * 100` percent.
    pub cpu_count: u32,
    /// Sum of every process's `cpu`, in percent of one core.
    pub cpu_total: f32,
    /// Bytes in use as Activity Monitor counts "Memory Used": app + wired + compressed.
    pub mem_used: u64,
    pub mem_total: u64,
    /// Every Session with at least one live process.
    pub sessions: Vec<ActivitySession>,
    /// Every Session's processes, plus the busiest others by CPU and by memory. Unordered.
    pub processes: Vec<ActivityProcess>,
}

/// One usage limit of one coding agent, e.g. Claude Code's 5-hour window.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    /// Short name for the window: `5h`, `Week`, `Opus wk`.
    pub label: String,
    /// Percent of the limit used; 100 is the limit (overage can exceed it).
    pub used_percent: f32,
    /// When the window resets, epoch ms; `None` when the agent does not say.
    pub resets_at: Option<u64>,
}

/// One coding agent's usage limits. Payload element of `usage`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsage {
    pub agent: AgentKind,
    /// The agent's limits; empty when never read (see `error`).
    pub windows: Vec<UsageWindow>,
    /// The plan the agent reports (`free`, `plus`, `max`), if any.
    pub plan: Option<String>,
    /// When `windows` were read (Claude Code) or recorded by the agent (Codex), epoch ms.
    pub updated_at: Option<u64>,
    /// Why the numbers are missing or stale; `windows` then hold the last good ones, if any.
    pub error: Option<String>,
}

/// Usage limits of the agents chosen in Settings, in the order asked for. Payload of `usage`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub agents: Vec<AgentUsage>,
}

/// Event names. Frontend listens with `listen(EVENT_SESSION_INFO, ...)`.
pub const EVENT_SESSION_INFO: &str = "session-info";
pub const EVENT_SESSION_EXIT: &str = "session-exit";
/// Sent every activity tick while the webview watches (`activity_watch`).
pub const EVENT_ACTIVITY: &str = "activity";
/// Sent when the watched agents' usage changes, and right away on `usage_watch`.
pub const EVENT_USAGE: &str = "usage";
/// The app menu's "Settings…" was chosen: the webview opens the Settings page.
pub const EVENT_MENU_SETTINGS: &str = "menu-settings";
