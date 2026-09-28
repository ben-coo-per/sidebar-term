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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// What an Agent session is doing: Running, Needs input or Done (`status.rs`, from the
/// Session's OSC title, BEL and output; docs/architecture.md "Agent status").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentStatus {
    Running,
    NeedsInput,
    Done,
}

/// Everything the sidebar knows about a Session: what the monitor probes (the Foreground
/// process, the agent, cwd, git) and what the Host read in its output (`remote/tap.rs`: the OSC
/// title, BELs) with the Agent status derived from both. Recomputed every monitor tick.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The latest OSC 0 / 2 title the Session's output set (`""` once cleared); `None` before
    /// the first one.
    #[serde(default)]
    pub title: Option<String>,
    /// BELs (0x07) in the Session's output so far. A client that wants to ring one compares.
    #[serde(default)]
    pub bells: u32,
    /// Running / Needs input / Done for an Agent session; `None` otherwise.
    #[serde(default)]
    pub status: Option<AgentStatus>,
    /// Every change of `status` over the last few hours, oldest first (`agents/`): Manager's lane.
    /// The first entry may be older than the window, so the lane knows how it started.
    #[serde(default)]
    pub history: Vec<StatusChange>,
    /// The agent reports to the Host through its hooks (Claude Code started through
    /// sidebar-term's `claude`); `false` for a screen-only agent (Codex, Gemini, a Claude Code
    /// started another way).
    #[serde(default)]
    pub hooked: bool,
    /// What the agent is asking right now, from its hooks; `None` while it asks nothing, and
    /// always for a screen-only agent. While set, `status` is Needs input.
    #[serde(default)]
    pub pending: Option<Pending>,
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
            title: None,
            bells: 0,
            status: None,
            history: Vec::new(),
            hooked: false,
            pending: None,
        }
    }
}

/// One change of a Session's Agent status: to `status` (`None`: the agent left), at `at`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusChange {
    pub status: Option<AgentStatus>,
    /// Epoch ms.
    pub at: u64,
}

/// What kind of question a hooked agent is waiting on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PendingKind {
    /// May it use a tool (edit a file, run a command)?
    Permission,
    /// A question it put to the user, with options (Claude Code's AskUserQuestion).
    Question,
}

/// How a line of a question's detail reads: a diff's removal or addition, or plain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineTone {
    Plain,
    Add,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingLine {
    pub text: String,
    pub tone: LineTone,
}

/// A question a hooked agent is waiting on (`agents/hooks.rs`). The Host holds the agent's hook
/// open until a client answers with one of `options` (by index) or lets go of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    /// Names this question, so an answer to one already gone is refused.
    pub id: u64,
    pub kind: PendingKind,
    /// The question, one line: "Make this edit to src/session.rs?".
    pub text: String,
    /// What it is about, a few lines: the diff of an edit, the command to run.
    pub detail: Vec<PendingLine>,
    /// The answers on offer, the first the default: "Yes", ..., "No, tell Claude what to do".
    pub options: Vec<String>,
    /// When it was asked, epoch ms.
    pub since: u64,
}

/// What an agent did, for Manager's feed: from its hooks (a tool it used, a question it asked,
/// an answer given) or, for a screen-only agent, a change of its status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentEventKind {
    /// It asked the user something.
    Asked,
    /// The user answered it.
    Answered,
    /// The user gave it a prompt.
    Started,
    /// It changed a file.
    Edit,
    /// It read or searched.
    Read,
    /// It ran a command or another tool.
    Command,
    /// A tool it used failed.
    Failed,
    /// It stopped, back at its prompt.
    Idle,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    /// Epoch ms.
    pub at: u64,
    pub session_id: SessionId,
    pub kind: AgentEventKind,
    /// One line: "Updated remote/pairing.rs (+61)", "Ran cargo test -p core".
    pub text: String,
}

/// Payload of the `session-exit` event.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    /// Physical footprint in bytes, as Activity Monitor's "Memory" column (compressed and swapped
    /// pages included); resident size for another user's process, whose footprint is unreadable.
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
    /// Part of `mem_used` macOS keeps for itself (the kernel, the GPU): no process's.
    pub mem_wired: u64,
    /// Part of `mem_used` the compressor occupies. Process footprints count their compressed
    /// pages at full size, so this overlaps them.
    pub mem_compressed: u64,
    pub mem_total: u64,
    /// Every Session with at least one live process.
    pub sessions: Vec<ActivitySession>,
    /// Every Session's processes, plus the busiest others by CPU and by memory. Unordered.
    pub processes: Vec<ActivityProcess>,
}

/// One Session Memory Guard, or the user, has frozen (every process in it stopped with SIGSTOP).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrozenSession {
    pub session_id: SessionId,
    /// The Session's memory (sum of footprints) when it was frozen, bytes.
    pub mem: u64,
    /// When it was frozen, epoch ms.
    pub frozen_at: u64,
    /// Frozen by the user from the Tab, not by Memory Guard's policy: Memory Guard does not thaw it.
    pub manual: bool,
}

/// Memory Guard's state. Payload of `memory-guard`, sent on every change.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardSnapshot {
    pub on: bool,
    /// Freeze a Tab when Memory Used passes this percent of physical memory.
    pub limit_percent: u8,
    /// Frozen Sessions, oldest first: the order they are thawed in.
    pub frozen: Vec<FrozenSession>,
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

/// What a Resume brings back in a Tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResumeKind {
    /// A Claude Code conversation, reopened with `claude --resume <session id>`.
    Claude,
    /// Any other Foreground job, rerun from its argv (`npm run dev`, `uv run app.py`).
    Command,
}

/// How to start again what one Session was running, should the app close while it runs.
/// Recorded by `resume.rs`; the Resume banner offers the previous run's entries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeEntry {
    /// The key the webview gave the Session at spawn (its Tab id). Opaque to Rust.
    pub key: String,
    pub kind: ResumeKind,
    /// The shell command line to type, quoted for zsh/bash: `claude --resume 5b6d…`, `npm run dev`.
    pub line: String,
    /// Canonical directory to run `line` in.
    pub cwd: Option<String>,
}

/// A running Claude Code conversation's files on this Host, for Handoff (`handoff.rs`): where its
/// transcript is, and the project's auto memory beside it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeConversation {
    /// The conversation's session id (`claude --resume <id>`).
    pub id: String,
    /// The directory the conversation runs in (Claude Code's `cwd`).
    pub cwd: String,
    /// The transcript, `<config dir>/projects/<key>/<id>.jsonl`.
    pub transcript: String,
    /// The project's `memory/` directory beside the transcript, when there is one.
    pub memory: Option<String>,
}

/// One memory file shipped with a conversation: its path relative to `memory/`, and its text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationFile {
    pub name: String,
    pub content: String,
}

/// A conversation's files as they travel to another Host: the transcript's text and the
/// memory files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationFiles {
    pub transcript: String,
    #[serde(default)]
    pub memory: Vec<ConversationFile>,
}

/// What would not move with a Tab: the state of its checkout (`handoff::git_status`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    /// The branch checked out; `None` when detached.
    pub branch: Option<String>,
    /// Its upstream (`origin/main`), when it has one.
    pub upstream: Option<String>,
    /// Commits on the branch that its upstream lacks; 0 without an upstream.
    pub ahead: u32,
    /// Changed or untracked paths (`git status --porcelain` lines).
    pub changes: u32,
    /// `origin`'s URL, for a `git clone` typed on the Host; `None` without an `origin`.
    pub remote_url: Option<String>,
}

/// What Handoff needs to know about a local Session before moving its Tab (`handoff_probe`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffProbe {
    /// The Session's facts, freshly probed.
    pub info: SessionInfo,
    /// What the Session is running, as Resume would record it; `None` at a prompt.
    pub entry: Option<ResumeEntry>,
    /// The Claude Code conversation running in it, with its files located; `None` otherwise.
    pub conversation: Option<ClaudeConversation>,
    /// The checkout's git status when the Session's cwd is in a repo; `None` otherwise.
    pub git: Option<GitStatus>,
}

/// Whether a path exists on a Host (the Host protocol's `path_exists`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathExists {
    pub exists: bool,
    /// True when it exists and is a directory.
    pub dir: bool,
}

/// A Group of the sidebar: user-named, user-ordered, holding Tabs in display order (`layout/`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub name: String,
    pub collapsed: bool,
    /// Tab ids, in display order.
    pub tab_ids: Vec<String>,
}

/// A Tab of the sidebar: the entry for one Session, in exactly one Group (`layout/`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub id: String,
    pub group_id: String,
    /// The Session this Tab points at; `None` while it has none (its shell failed to spawn).
    pub session_id: Option<SessionId>,
    /// A rename the user typed, which sticks; `None` means "the automatic Title".
    pub custom_title: Option<String>,
    /// Last known non-remote cwd, where the Tab's Session respawns at the next launch.
    pub last_cwd: Option<String>,
}

/// The whole layout, as the Host holds it. Payload of `layout` and of `layout_get`; every
/// client mirrors it (ADR 0002).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSnapshot {
    /// Counts up on every change, so a client can tell a stale snapshot from a newer one.
    pub revision: u64,
    /// In sidebar order.
    pub groups: Vec<Group>,
    /// Every Tab, by id.
    pub tabs: std::collections::BTreeMap<String, Tab>,
    pub active_tab_id: Option<String>,
}

/// The Host as its clients see it: `hello.host` in the Host protocol (`src/lib/host/protocol.ts`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostInfo {
    /// The machine's hostname.
    pub name: String,
    /// The core's version.
    pub version: String,
    /// The Host's home directory, for the `~` in automatic Titles; `None` when unknown.
    pub home: Option<String>,
}

/// What Tailscale says about this Mac, read from its CLI (`remote/tailscale.rs`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TailscaleState {
    /// The Tailscale CLI was found (the app or a Homebrew install).
    pub installed: bool,
    /// Tailscale is up and logged in.
    pub running: bool,
    /// This Mac's MagicDNS name, `bens-mac.tail1234.ts.net`, once running.
    pub dns_name: Option<String>,
    /// Why Serve could not be set up, or the last CLI error, if any.
    pub error: Option<String>,
}

/// A phone that paired with Remote: it holds a token this Mac accepts (hashed at rest).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteDevice {
    pub id: String,
    /// The name the phone gave itself when pairing.
    pub name: String,
    /// Epoch ms.
    pub created_at: u64,
    /// Epoch ms of its last connection, if it connected since pairing.
    pub last_seen_at: Option<u64>,
    /// The Tailscale login the pairing request came through, when it came through Serve.
    pub login: Option<String>,
}

/// A pairing in progress: the code a phone must present, shown as a QR code in Settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pub code: String,
    /// The page to open on the phone, code included, or null while there is no URL to reach.
    pub url: Option<String>,
    /// Epoch ms.
    pub expires_at: u64,
}

/// The state of Remote. Payload of `remote_state` and of the `remote` event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSnapshot {
    /// Remote is on: the server listens and Tailscale Serve is asked to publish it.
    pub on: bool,
    /// The port the server listens on, on 127.0.0.1 only.
    pub port: u16,
    /// The phone's page, `https://<dns name>/m`, once Tailscale Serve publishes the server.
    pub url: Option<String>,
    /// Why the server is not listening although Remote is on.
    pub error: Option<String>,
    pub tailscale: TailscaleState,
    /// Phones connected right now.
    pub clients: u32,
    pub devices: Vec<RemoteDevice>,
    pub pairing: Option<Pairing>,
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
/// Caffeinate turned off on its own (its `caffeinate` run ended); payload `false`.
pub const EVENT_CAFFEINATE: &str = "caffeinate";
/// Remote's state changed (turned on or off, a phone connected or paired, a pairing expired).
pub const EVENT_REMOTE: &str = "remote";
/// Memory Guard froze or thawed a Session, or was turned on or off; payload `GuardSnapshot`.
pub const EVENT_MEMORY_GUARD: &str = "memory-guard";
/// The layout changed (a Tab or Group made, closed, renamed, moved, activated, a Tab's Session
/// or last cwd changed); payload `LayoutSnapshot`, the whole of it.
pub const EVENT_LAYOUT: &str = "layout";
/// An agent did something (`agents/`); payload `AgentEvent`.
pub const EVENT_AGENT_EVENT: &str = "agent-event";
