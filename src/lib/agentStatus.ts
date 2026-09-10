// Pure title-parsing and status/title derivation rules. No timers, no DOM, no IPC:
// every input a caller can observe (title, timestamps, agent kind) is passed in, so callers
// can inject their own clock. See docs/architecture.md "Agent status" and "Naming".
//
// OWNER: sidebar agent. Kept dependency-free on purpose so it stays trivially unit-testable.

import type { AgentKind } from "./types";

export type AgentStatus = "running" | "needs-input" | "done";

/** Display name for the automatic Title when a Session is an Agent session. */
export const AGENT_NAMES: Record<AgentKind, string> = {
  claude: "Claude Code",
  codex: "Codex",
  gemini: "Gemini",
};

/** A braille spinner frame, per Codex's `tui.terminal_title` "activity" item. */
const BRAILLE_SPINNER = /^[⠀-⣿]/;

/** How long Claude Code counts as "Running" after the last output activity. */
export const CLAUDE_RUNNING_WINDOW_MS = 3000;

export interface AgentStatusInput {
  agent: AgentKind | null;
  /** Latest OSC 0/2 title, "" (or null) when none has been set / it was cleared. */
  title: string | null;
  /** Caller-supplied clock reading, ms. Never read from Date.now() internally. */
  now: number;
  /** Timestamp of the last output-activity event on this Session, ms, or null if none yet. */
  lastActivityAt: number | null;
  /** Timestamp of the last BEL received while this agent was foreground, ms, or null. */
  lastBellAt: number | null;
}

/**
 * Derive Running / Needs input / Done for an Agent session, per the table in
 * docs/architecture.md. Returns null when `agent` is null (not an Agent session).
 */
export function computeAgentStatus(input: AgentStatusInput): AgentStatus | null {
  const { agent, now, lastActivityAt, lastBellAt } = input;
  if (!agent) return null;
  const title = input.title ?? "";

  if (agent === "codex") {
    if (BRAILLE_SPINNER.test(title)) return "running";
    if (title.includes("Action Required")) return "needs-input";
    return "done";
  }

  if (agent === "gemini") {
    if (title.startsWith("✦")) return "running";
    if (title.startsWith("✋")) return "needs-input";
    return "done"; // "◇" (Ready), or no marker yet: treat as idle/done.
  }

  // Claude Code: no state in the title; derive from activity and BEL instead.
  const activeRecently = lastActivityAt !== null && now - lastActivityAt < CLAUDE_RUNNING_WINDOW_MS;
  if (activeRecently) return "running";
  // A BEL that landed after the last activity (and hasn't been superseded by fresh activity)
  // means the agent is still waiting on that prompt.
  if (lastBellAt !== null && (lastActivityAt === null || lastBellAt >= lastActivityAt)) {
    return "needs-input";
  }
  return "done";
}

export interface AutomaticTitleInput {
  agent: AgentKind | null;
  /** Latest OSC 0/2 title as set by the Foreground process, or null/"" if none. */
  oscTitle: string | null;
  /** `comm` of the Foreground process, e.g. "zsh", "vim". */
  foreground: string | null;
  shellIsForeground: boolean;
  cwd: string | null;
  /** The user's home directory, if known, for the `~` special case. */
  home: string | null;
}

function basename(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  if (trimmed === "") return "/";
  const idx = trimmed.lastIndexOf("/");
  return idx === -1 ? trimmed : trimmed.slice(idx + 1);
}

/**
 * The automatic Title: agent name, else the OSC title, else the Foreground process when it
 * isn't the shell, else the cwd basename (`~` for home). Per docs/architecture.md "Naming".
 * A user rename always wins over this and is applied by the caller, not here.
 */
export function computeAutomaticTitle(input: AutomaticTitleInput): string {
  const { agent, oscTitle, foreground, shellIsForeground, cwd, home } = input;
  if (agent) return AGENT_NAMES[agent];
  if (oscTitle && oscTitle.trim() !== "") return oscTitle.trim();
  if (foreground && !shellIsForeground) return foreground;
  if (!cwd) return "~";
  if (home && cwd === home) return "~";
  return basename(cwd);
}
