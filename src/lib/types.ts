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
  /** Resident memory, bytes. */
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
  /** Bytes in use as Activity Monitor counts "Memory Used". */
  memUsed: number;
  memTotal: number;
  /** Every Session with at least one live process. */
  sessions: ActivitySession[];
  /** Every Session's processes plus the busiest others by CPU and by memory. Unordered. */
  processes: ActivityProcess[];
}

export const EVENT_SESSION_INFO = "session-info";
export const EVENT_SESSION_EXIT = "session-exit";
export const EVENT_ACTIVITY = "activity";
