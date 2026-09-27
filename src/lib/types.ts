// Mirror of src-tauri/core/src/model.rs. CONTRACT: owned by the tech lead; keep in sync with Rust.

/** Identity of one Session (one shell on one pty). Never reused within an app run. */
export type SessionId = number;

/** A known coding agent that can be a Session's Foreground process. */
export type AgentKind = "claude" | "codex" | "gemini";

/** Repo / Worktree / branch facts for a Session's cwd. Drives the Badge. */
export interface GitInfo {
  /** Basename of the main worktree's directory. */
  repoName: string;
  /** Canonical shared git dir. Identity of the repo: equal across its Worktrees. */
  commonDir: string;
  /** Canonical top-level of the Worktree the cwd is in. */
  worktreeRoot: string;
  /** null for the main Worktree; the linked Worktree's name otherwise. */
  worktreeName: string | null;
  /** Branch without refs/heads/; null when detached. */
  branch: string | null;
  /** Short sha when detached; null on a branch. */
  headShort: string | null;
}

/** What an Agent session is doing, derived on the Host (src-tauri/core/src/status.rs). */
export type AgentStatus = "running" | "needs-input" | "done";

/**
 * Everything the sidebar knows about a Session: what the Host probes, what it read in the
 * Session's output (the OSC title, BELs) and the Agent status derived from both. Pushed by Rust
 * on the `session-info` event; sent whole by a Host over the Host protocol.
 */
export interface SessionInfo {
  sessionId: SessionId;
  /** comm of the process describing the Foreground process group, e.g. "zsh", "vim", "claude". */
  foreground: string | null;
  /** True when the shell itself holds the tty (idle at a prompt). */
  shellIsForeground: boolean;
  agent: AgentKind | null;
  cwd: string | null;
  /** Foreground is ssh/mosh/docker/kubectl...: cwd and git describe this Mac, show a remote marker. */
  remote: boolean;
  git: GitInfo | null;
  /** The latest OSC 0/2 title the Session's output set ("" once cleared); null before the first. */
  title: string | null;
  /** BELs in the Session's output so far. */
  bells: number;
  /** Running / Needs input / Done for an Agent session; null otherwise. */
  status: AgentStatus | null;
}

/** A Host as its clients see it (`hello.host` in the Host protocol). */
export interface HostInfo {
  /** The machine's hostname. */
  name: string;
  /** The core's version. */
  version: string;
  /** The Host's home directory, for the `~` in automatic Titles; null when unknown. */
  home: string | null;
}

export interface SessionExit {
  sessionId: SessionId;
  code: number | null;
}

/** One process in an ActivitySnapshot. */
export interface ActivityProcess {
  pid: number;
  /** Executable file name; an agent's command name ("claude") for a coding agent. */
  name: string;
  /** Percent of one core over the last interval (two busy cores read 200). */
  cpu: number;
  /** Physical footprint, bytes, as Activity Monitor's "Memory" column (compressed pages included);
   *  resident size for another user's process. */
  mem: number;
  /** The Session whose shell this process is or descends from; null for everything else. */
  sessionId: SessionId | null;
}

/** Totals over every process of one Session. */
export interface ActivitySession {
  sessionId: SessionId;
  cpu: number;
  mem: number;
  processes: number;
}

/** CPU and memory of the whole Mac, with each Session's share. Pushed on the `activity` event. */
export interface ActivitySnapshot {
  /** Logical cores: the machine's capacity is cpuCount * 100 percent. */
  cpuCount: number;
  /** Sum of every process's cpu, in percent of one core. */
  cpuTotal: number;
  /** Bytes in use as Activity Monitor counts "Memory Used": app + wired + compressed. */
  memUsed: number;
  /** Part of memUsed macOS keeps for itself (the kernel, the GPU): no process's. */
  memWired: number;
  /** Part of memUsed the compressor occupies (overlaps process footprints). */
  memCompressed: number;
  memTotal: number;
  /** Every Session with at least one live process. */
  sessions: ActivitySession[];
  /** Every Session's processes plus the busiest others by CPU and by memory. Unordered. */
  processes: ActivityProcess[];
}

/** One Session Memory Guard, or the user, has frozen (every process in it stopped). */
export interface FrozenSession {
  sessionId: SessionId;
  /** Its memory when frozen, bytes. */
  mem: number;
  /** When it was frozen, epoch ms. */
  frozenAt: number;
  /** Frozen by the user from the Tab: Memory Guard does not thaw it. */
  manual: boolean;
}

/** Memory Guard's state. From `guard_state` / `guard_set`, and pushed on `memory-guard`. */
export interface GuardSnapshot {
  on: boolean;
  /** Freeze a Tab when Memory Used passes this percent of physical memory. */
  limitPercent: number;
  /** Oldest first: the order they are thawed in. */
  frozen: FrozenSession[];
}

/** One usage limit of one coding agent, e.g. Claude Code's 5-hour window. */
export interface UsageWindow {
  /** Short name for the window: "5h", "Week", "Opus wk". */
  label: string;
  /** Percent of the limit used; 100 is the limit (overage can exceed it). */
  usedPercent: number;
  /** When the window resets, epoch ms; null when the agent does not say. */
  resetsAt: number | null;
}

/** One coding agent's usage limits. */
export interface AgentUsage {
  agent: AgentKind;
  /** The agent's limits; empty when never read (see `error`). */
  windows: UsageWindow[];
  /** The plan the agent reports ("free", "plus", "max"), if any. */
  plan: string | null;
  /** When `windows` were read (Claude Code) or recorded by the agent (Codex), epoch ms. */
  updatedAt: number | null;
  /** Why the numbers are missing or stale; `windows` then hold the last good ones, if any. */
  error: string | null;
}

/** Usage limits of the agents chosen in Settings, in the order asked for. Pushed on `usage`. */
export interface UsageSnapshot {
  agents: AgentUsage[];
}

/** What a Resume brings back in a Tab. */
export type ResumeKind = "claude" | "command";

/** How to start again what one Session was running when the app closed. From `resume_leftover`. */
export interface ResumeEntry {
  /** The key the Session was spawned with: its Tab id. */
  key: string;
  /** "claude": `claude --resume <id>`; "command": any other job, rerun from its argv. */
  kind: ResumeKind;
  /** The shell command line to type, already quoted: "claude --resume 5b6d…", "npm run dev". */
  line: string;
  /** Canonical directory to run `line` in. */
  cwd: string | null;
}

/** A running Claude Code conversation's files on this Mac, for Handoff (`handoff_probe`). */
export interface ClaudeConversation {
  /** The conversation's session id (`claude --resume <id>`). */
  id: string;
  /** The directory the conversation runs in. */
  cwd: string;
  /** The transcript's path, `<config dir>/projects/<key>/<id>.jsonl`. */
  transcript: string;
  /** The project's `memory/` directory beside the transcript, when there is one. */
  memory: string | null;
}

/** One memory file shipped with a conversation: its path under `memory/`, and its text. */
export interface ConversationFile {
  name: string;
  content: string;
}

/** A conversation's files as they travel to another Host. */
export interface ConversationFiles {
  transcript: string;
  memory: ConversationFile[];
}

/** What would not move with a Tab: its checkout's state (`handoff_probe`). */
export interface GitStatus {
  /** The branch checked out; null when detached. */
  branch: string | null;
  /** Its upstream (`origin/main`), when it has one. */
  upstream: string | null;
  /** Commits the upstream lacks; 0 without an upstream. */
  ahead: number;
  /** Changed or untracked paths. */
  changes: number;
  /** `origin`'s URL, for a `git clone` typed on the Host; null without an `origin`. */
  remoteUrl: string | null;
}

/** What Handoff needs to know about a local Session before moving its Tab. From `handoff_probe`. */
export interface HandoffProbe {
  /** The Session's facts, freshly probed. */
  info: SessionInfo;
  /** What the Session is running, as Resume would record it; null at a prompt. */
  entry: ResumeEntry | null;
  /** The Claude Code conversation running in it, with its files located; null otherwise. */
  conversation: ClaudeConversation | null;
  /** The checkout's git status when the cwd is in a repo; null otherwise. */
  git: GitStatus | null;
}

/** Whether a path exists on a Host (the Host protocol's `path_exists`). */
export interface PathExists {
  exists: boolean;
  /** True when it exists and is a directory. */
  dir: boolean;
}

/** A Group as the Host holds it: user-named, user-ordered, its Tabs in display order. */
export interface Group {
  id: string;
  name: string;
  collapsed: boolean;
  /** Tab ids, in display order. */
  tabIds: string[];
}

/** A Tab as the Host holds it: the entry for one Session, in exactly one Group. */
export interface Tab {
  id: string;
  groupId: string;
  /** null while the Tab has no Session (its shell failed to spawn). */
  sessionId: SessionId | null;
  /** A rename that sticks; null means "the automatic Title". */
  customTitle: string | null;
  /** Last known non-remote cwd, where the Tab's Session respawns at the next launch. */
  lastCwd: string | null;
}

/** The whole layout, from `layout_get` and the `layout` event; the webview mirrors it. */
export interface LayoutSnapshot {
  /** Counts up on every change: a snapshot older than the one shown is ignored. */
  revision: number;
  /** In sidebar order. */
  groups: Group[];
  /** Every Tab, by id. */
  tabs: Record<string, Tab>;
  activeTabId: string | null;
}

/** What Tailscale says about this Mac, read from its CLI. */
export interface TailscaleState {
  /** The Tailscale CLI was found (the app or a Homebrew install). */
  installed: boolean;
  /** Tailscale is up and logged in. */
  running: boolean;
  /** This Mac's MagicDNS name, `bens-mac.tail1234.ts.net`, once running. */
  dnsName: string | null;
  /** Why Serve could not be set up, or the last CLI error, if any. */
  error: string | null;
}

/** A phone that paired with Remote: it holds a token this Mac accepts. */
export interface RemoteDevice {
  id: string;
  /** The name the phone gave itself when pairing. */
  name: string;
  /** Epoch ms. */
  createdAt: number;
  /** Epoch ms of its last connection, if it connected since pairing. */
  lastSeenAt: number | null;
  /** The Tailscale login the pairing came through, when it came through Serve. */
  login: string | null;
}

/** A pairing in progress: the code a phone must present, shown as a QR code in Settings. */
export interface Pairing {
  /** As shown for typing: `ABCD EFGH`. */
  code: string;
  /** The page to open on the phone, code included, or null while there is no URL to reach. */
  url: string | null;
  /** Epoch ms. */
  expiresAt: number;
}

/** The state of Remote. From `remote_state` / `remote_set` and the `remote` event. */
export interface RemoteSnapshot {
  /** Remote is on: the server listens and Tailscale Serve is asked to publish it. */
  on: boolean;
  /** The port the server listens on, on 127.0.0.1 only. */
  port: number;
  /** The phone's page, `https://<dns name>/m`, once Tailscale Serve publishes the server. */
  url: string | null;
  /** Why the server is not listening although Remote is on. */
  error: string | null;
  tailscale: TailscaleState;
  /** Phones connected right now. */
  clients: number;
  devices: RemoteDevice[];
  pairing: Pairing | null;
}

export const EVENT_SESSION_INFO = "session-info";
export const EVENT_SESSION_EXIT = "session-exit";
export const EVENT_ACTIVITY = "activity";
export const EVENT_USAGE = "usage";
/** The app menu's "Settings…" was chosen. */
export const EVENT_MENU_SETTINGS = "menu-settings";
/** Caffeinate turned off on its own (its `caffeinate` run ended); payload `false`. */
export const EVENT_CAFFEINATE = "caffeinate";
/** Remote's state changed (turned on or off, a phone connected or paired, a pairing expired). */
export const EVENT_REMOTE = "remote";
/** Memory Guard froze or thawed a Session, or was turned on or off; payload GuardSnapshot. */
export const EVENT_MEMORY_GUARD = "memory-guard";
/** The layout changed; payload LayoutSnapshot, the whole of it. */
export const EVENT_LAYOUT = "layout";
