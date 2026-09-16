// Mirror of src-tauri/src/model.rs. CONTRACT: owned by the tech lead; keep in sync with Rust.

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

/** Everything the sidebar knows about a Session. Pushed by Rust on the `session-info` event. */
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

export const EVENT_SESSION_INFO = "session-info";
export const EVENT_SESSION_EXIT = "session-exit";
export const EVENT_ACTIVITY = "activity";
export const EVENT_USAGE = "usage";
/** The app menu's "Settings…" was chosen. */
export const EVENT_MENU_SETTINGS = "menu-settings";
/** Caffeinate turned off on its own (its `caffeinate` run ended); payload `false`. */
export const EVENT_CAFFEINATE = "caffeinate";
/** Memory Guard froze or thawed a Session, or was turned on or off; payload GuardSnapshot. */
export const EVENT_MEMORY_GUARD = "memory-guard";
