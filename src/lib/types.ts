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

export const EVENT_SESSION_INFO = "session-info";
export const EVENT_SESSION_EXIT = "session-exit";
