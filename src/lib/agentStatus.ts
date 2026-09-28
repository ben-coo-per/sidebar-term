// Pure title-derivation rules. No timers, no DOM, no IPC: every input a caller can observe is
// passed in. See docs/architecture.md "Naming".
//
// Agent status (Running / Needs input / Done) is not derived here any more: the Host derives it
// from the Session's OSC title, BELs and output (src-tauri/core/src/status.rs) and sends it as
// `SessionInfo.status`, so the Mac webview, a phone and a headless Host all show one answer.
//
// OWNER: sidebar agent. Kept dependency-free on purpose so it stays trivially unit-testable.

import type { AgentKind, AgentStatus, GitInfo } from "./types";

export type { AgentStatus };

/** Display name for the automatic Title when a Session is an Agent session. */
export const AGENT_NAMES: Record<AgentKind, string> = {
  claude: "Claude Code",
  codex: "Codex",
  gemini: "Gemini",
};

export interface AutomaticTitleInput {
  agent: AgentKind | null;
  /** Repo facts of the cwd: an agent is named after its project. Null outside a repo or on a remote hop. */
  git?: GitInfo | null;
  /** The last prompt the user gave the agent, when the caller knows it (Manager's feed). */
  lastPrompt?: string | null;
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
 * The automatic Title: for an agent, its project and a few words on what it is at
 * (`jack · fix the pairing`, see `agentLabel`), else the OSC title of a running program, else the Foreground process when it
 * isn't the shell, else the cwd basename (`~` for home). Per docs/architecture.md "Naming".
 * A user rename always wins over this and is applied by the caller, not here.
 */
export function computeAutomaticTitle(input: AutomaticTitleInput): string {
  const { agent, oscTitle, foreground, shellIsForeground, cwd, home } = input;
  if (agent) {
    const { project, description } = agentLabel({
      customTitle: null,
      git: input.git ?? null,
      cwd,
      home,
      oscTitle,
      agent,
      lastPrompt: input.lastPrompt ?? null,
    });
    return description ? `${project} · ${description}` : project;
  }
  // While the shell sits at its prompt, its own title (typically `user@host:path` from the
  // user's zsh theme) says nothing the cwd doesn't; only a running program's title is useful.
  if (!shellIsForeground && oscTitle && oscTitle.trim() !== "") return oscTitle.trim();
  if (foreground && !shellIsForeground) return foreground;
  if (!cwd) return "~";
  if (home && cwd === home) return "~";
  return basename(cwd);
}

// --- What to call an agent: Manager's lanes and every automatic Title -------------------------------------------------------------------

/** Words a description keeps at most: enough to jog the memory, not to explain. */
const DESCRIPTION_WORDS = 3;

/** How each agent's name reads in its own title, which a description never merely repeats. */
const AGENT_WORDS: Record<AgentKind, string[]> = {
  claude: ["claude", "claude code"],
  codex: ["codex"],
  gemini: ["gemini"],
};

/** What an agent is called (a Tab's automatic Title, Manager's lanes): the project it works in, and a few words on what it is at. */
export interface AgentLabel {
  project: string;
  description: string | null;
}

export interface AgentLabelInput {
  /** A rename the user typed: it is the name, and nothing is added to it. */
  customTitle: string | null;
  git: GitInfo | null;
  cwd: string | null;
  home: string | null;
  /** The Session's latest OSC title. */
  oscTitle: string | null;
  agent: AgentKind | null;
  /** The last prompt the user gave the agent, from its events. */
  lastPrompt: string | null;
}

/** The agent's own words in its title, status marker gone; null when the title says nothing of its own. */
function titleWords(agent: AgentKind | null, title: string | null): string | null {
  const t = title?.trim() ?? "";
  if (!t) return null;
  switch (agent) {
    case "claude":
      // "◐ Fix the badge", "✳ Fix the badge": Claude Code's summary of the conversation.
      return t.replace(/^[◐◑✳]\s*/u, "") || null;
    case "codex":
    case "gemini":
      // A spinner, "Working… (x)", "Action Required", "Ready (x)": status and project, no subject.
      return null;
    default:
      return t;
  }
}

/** Acronyms, camelCase, paths and identifiers keep their case: "API", "moveTabToHost", "v2". */
function properLooking(word: string): boolean {
  return /[A-Z].*[A-Z]|[a-z][A-Z]|[_./\d]/.test(word);
}

/** At most `DESCRIPTION_WORDS` words of `text`, lower-cased but for proper-looking ones, no trailing punctuation. */
function shorten(text: string): string | null {
  const words = text
    .replace(/[“”"]/g, "")
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, DESCRIPTION_WORDS)
    .map((w) => (properLooking(w) ? w : w.toLowerCase()));
  return words.join(" ").replace(/[\s.,;:!?…—–-]+$/u, "") || null;
}

/** A rename, else the repo (with its linked Worktree), else the cwd's folder (`~` for home), else the agent. */
function projectOf(input: AgentLabelInput): string {
  if (input.customTitle) return input.customTitle;
  if (input.git) return input.git.worktreeName ? `${input.git.repoName}/${input.git.worktreeName}` : input.git.repoName;
  if (input.cwd) {
    if (input.home && input.cwd.replace(/\/+$/, "") === input.home.replace(/\/+$/, "")) return "~";
    const base = input.cwd.replace(/\/+$/, "").split("/").pop();
    if (base) return base;
  }
  return input.agent ? AGENT_NAMES[input.agent] : "Agent";
}

/**
 * What an agent is called: its project, and one to three words on what it is at, from the
 * agent's own title (Claude Code's summary of the conversation) or else the user's last prompt.
 * Loose on purpose: it jogs the memory, it does not report. No description after a rename, or
 * when the words would only repeat the project or the agent's name.
 */
export function agentLabel(input: AgentLabelInput): AgentLabel {
  const project = projectOf(input);
  if (input.customTitle) return { project, description: null };
  const source = titleWords(input.agent, input.oscTitle) ?? (input.lastPrompt?.trim() || null);
  const description = source ? shorten(source) : null;
  if (!description) return { project, description: null };
  const repeats = [project, input.git?.repoName ?? "", ...(input.agent ? AGENT_WORDS[input.agent] : [])]
    .filter(Boolean)
    .map((s) => s.toLowerCase());
  return { project, description: repeats.includes(description.toLowerCase()) ? null : description };
}
