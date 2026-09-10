// Pure rules for the Usage view: which agents it can show, what a window reads now, and how
// times and percents are written. No state, no IPC; `now` is passed in.

import type { AgentKind, AgentUsage, UsageWindow } from "../../types";

/** Agents with a usage source, in display order. Gemini CLI has none yet. */
export const USAGE_AGENTS: readonly AgentKind[] = ["claude", "codex"];

/** Names for the collapsed header, where room is short. */
export const AGENT_SHORT_NAMES: Record<AgentKind, string> = {
  claude: "Claude",
  codex: "Codex",
  gemini: "Gemini",
};

/** From this percent a bar reads as nearly spent (amber), and from `FULL_PERCENT` as spent (red). */
export const HIGH_PERCENT = 80;
export const FULL_PERCENT = 95;

export type UsageLevel = "ok" | "high" | "full";

export function usageLevel(percent: number): UsageLevel {
  if (percent >= FULL_PERCENT) return "full";
  if (percent >= HIGH_PERCENT) return "high";
  return "ok";
}

/** Percent used now: 0 once the window has reset since it was read. */
export function currentPercent(w: UsageWindow, now: number): number {
  return w.resetsAt !== null && w.resetsAt <= now ? 0 : w.usedPercent;
}

/** Share of the bar to fill, 0..1. */
export function fillFraction(percent: number): number {
  return Math.max(0, Math.min(1, percent / 100));
}

export function formatPercent(percent: number): string {
  return `${Math.round(percent)}%`;
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** "42m", "2h 10m", "3d 4h"; "<1m" under a minute. */
export function formatDuration(ms: number): string {
  if (ms < MINUTE) return "<1m";
  if (ms < HOUR) return `${Math.floor(ms / MINUTE)}m`;
  if (ms < DAY) {
    const m = Math.floor((ms % HOUR) / MINUTE);
    return `${Math.floor(ms / HOUR)}h${m ? ` ${m}m` : ""}`;
  }
  const h = Math.floor((ms % DAY) / HOUR);
  return `${Math.floor(ms / DAY)}d${h ? ` ${h}h` : ""}`;
}

/** Time left until the window resets; "" when unknown, "reset" once it has. */
export function formatResetsIn(w: UsageWindow, now: number): string {
  if (w.resetsAt === null) return "";
  return w.resetsAt <= now ? "reset" : formatDuration(w.resetsAt - now);
}

/** "3h ago"; "just now" under a minute. */
export function formatAgo(at: number, now: number): string {
  return now - at < MINUTE ? "just now" : `${formatDuration(now - at)} ago`;
}

/** The fullest of an agent's windows now, for the collapsed header; null when it has none. */
export function peakPercent(a: AgentUsage, now: number): number | null {
  if (a.windows.length === 0) return null;
  return Math.max(...a.windows.map((w) => currentPercent(w, now)));
}

/** The `usage` settings section -> the agents chosen, in display order. Every one by default. */
export function parseUsageSection(raw: unknown): AgentKind[] {
  const agents = typeof raw === "object" && raw !== null ? (raw as { agents?: unknown }).agents : undefined;
  if (!Array.isArray(agents)) return [...USAGE_AGENTS];
  return USAGE_AGENTS.filter((a) => agents.includes(a));
}
