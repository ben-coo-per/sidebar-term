// Pure title-derivation rules. No timers, no DOM, no IPC: every input a caller can observe is
// passed in. See docs/architecture.md "Naming".
//
// Agent status (Running / Needs input / Done) is not derived here any more: the Host derives it
// from the Session's OSC title, BELs and output (src-tauri/core/src/status.rs) and sends it as
// `SessionInfo.status`, so the Mac webview, a phone and a headless Host all show one answer.
//
// OWNER: sidebar agent. Kept dependency-free on purpose so it stays trivially unit-testable.

import type { AgentKind, AgentStatus } from "./types";

export type { AgentStatus };

/** Display name for the automatic Title when a Session is an Agent session. */
export const AGENT_NAMES: Record<AgentKind, string> = {
  claude: "Claude Code",
  codex: "Codex",
  gemini: "Gemini",
};

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
 * The automatic Title: agent name, else the OSC title of a running program, else the Foreground process when it
 * isn't the shell, else the cwd basename (`~` for home). Per docs/architecture.md "Naming".
 * A user rename always wins over this and is applied by the caller, not here.
 */
export function computeAutomaticTitle(input: AutomaticTitleInput): string {
  const { agent, oscTitle, foreground, shellIsForeground, cwd, home } = input;
  if (agent) return AGENT_NAMES[agent];
  // While the shell sits at its prompt, its own title (typically `user@host:path` from the
  // user's zsh theme) says nothing the cwd doesn't; only a running program's title is useful.
  if (!shellIsForeground && oscTitle && oscTitle.trim() !== "") return oscTitle.trim();
  if (foreground && !shellIsForeground) return foreground;
  if (!cwd) return "~";
  if (home && cwd === home) return "~";
  return basename(cwd);
}
