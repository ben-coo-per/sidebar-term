// Manager's pure rules: which way a lane's time runs, where its segments sit, how lanes are
// ordered, the tick labels, and the short durations it prints. No state, no Svelte: the clock
// is passed in. See docs/architecture.md "Manager".

import type { ManagerZoom } from "../sidebar/settings";
import type { AgentEvent, AgentEventKind, AgentStatus, StatusChange } from "../types";

export const ZOOM_LABELS: Record<ManagerZoom, string> = { "15m": "15m", "1h": "1h", "4h": "4h", start: "Since start" };

const MINUTE = 60_000;
const ZOOM_SPANS: Record<Exclude<ManagerZoom, "start">, number> = { "15m": 15 * MINUTE, "1h": 60 * MINUTE, "4h": 240 * MINUTE };

/** The shortest span "Since start" shows, so a lane that just began is not one wide block. */
const MIN_SPAN = 5 * MINUTE;

/** When a lane began: its first change to an agent status; null for a Session that never had one. */
export function laneStart(history: StatusChange[]): number | null {
  return history.find((c) => c.status !== null)?.at ?? null;
}

/** The span the lanes show, in ms: the zoom's, or since the oldest shown lane began. */
export function windowSpan(zoom: ManagerZoom, starts: number[], now: number): number {
  if (zoom !== "start") return ZOOM_SPANS[zoom];
  const oldest = starts.length ? Math.min(...starts) : now;
  return Math.max(MIN_SPAN, now - oldest);
}

/** One stretch of a lane in one status, as percents of the window. */
export interface Segment {
  left: number;
  width: number;
  status: AgentStatus;
}

/**
 * The stretches of `history` inside the window ending at `now` and spanning `span` ms, each from
 * its change to the next (the last one to now). Stretches without an agent are gaps.
 */
export function segments(history: StatusChange[], now: number, span: number): Segment[] {
  const start = now - span;
  const out: Segment[] = [];
  history.forEach((c, i) => {
    if (c.status === null) return;
    const from = Math.max(c.at, start);
    const to = Math.min(i + 1 < history.length ? history[i + 1].at : now, now);
    if (to <= from) return;
    out.push({ left: ((from - start) / span) * 100, width: ((to - from) / span) * 100, status: c.status });
  });
  return out;
}

/** When the lane's current status began (the last change), or `now` without history. */
export function currentSince(history: StatusChange[], now: number): number {
  return history.length ? history[history.length - 1].at : now;
}

/** `H:MM` on the local clock. */
export function clockLabel(ms: number): string {
  const d = new Date(ms);
  return `${d.getHours()}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** Tick labels at 0, 25, 50 and 75% of the window: `now − span × (1 − p)`. */
export function ticks(now: number, span: number): { percent: number; label: string }[] {
  return [0, 0.25, 0.5, 0.75].map((p) => ({ percent: p * 100, label: clockLabel(now - span * (1 - p)) }));
}

/** A short duration: "<1m", "14m", "2h 10m", "3h". */
export function formatDuration(ms: number): string {
  const minutes = Math.floor(Math.max(0, ms) / MINUTE);
  if (minutes < 1) return "<1m";
  if (minutes < 60) return `${minutes}m`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m ? `${h}h ${m}m` : `${h}h`;
}

/** What a lane's agent is doing, in Manager's words. */
export type LaneKind = "waiting" | "working" | "idle";

export function laneKind(status: AgentStatus | null): LaneKind {
  return status === "needs-input" ? "waiting" : status === "running" ? "working" : "idle";
}

const KIND_RANK: Record<LaneKind, number> = { waiting: 0, working: 1, idle: 2 };

/** What ordering a lane needs: its kind, since when, and its place in the sidebar (ties). */
export interface Orderable {
  key: string;
  kind: LaneKind;
  since: number;
  order: number;
}

/** Waiting (longest wait first), then working, then idle; ties keep sidebar order. */
export function sortLanes<T extends Orderable>(lanes: T[]): T[] {
  return [...lanes].sort(
    (a, b) => KIND_RANK[a.kind] - KIND_RANK[b.kind] || (a.kind === "waiting" ? a.since - b.since : 0) || a.order - b.order,
  );
}

/**
 * The order to show: `next` (sorted), unless `frozen` (the pointer is over the lanes), when the
 * lanes keep their places, new ones join at the end and gone ones leave.
 */
export function holdOrder(shown: string[], next: string[], frozen: boolean): string[] {
  if (!frozen) return next;
  const live = new Set(next);
  const kept = shown.filter((k) => live.has(k));
  const had = new Set(kept);
  return [...kept, ...next.filter((k) => !had.has(k))];
}

/** How a feed row's dot reads, by what the agent did. */
export type EventTone = "waiting" | "working" | "muted" | "failed";

export const EVENT_TONES: Record<AgentEventKind, EventTone> = {
  asked: "waiting",
  edit: "working",
  read: "working",
  command: "working",
  failed: "failed",
  answered: "muted",
  started: "muted",
  idle: "muted",
};

const EDIT = /^(?:Updated|Wrote) (.+?)(?: \((?:\+(\d+))?(?: ?−(\d+))?\))?$/;

/**
 * What an agent's last turn changed, from its events (oldest first): "6 files changed, +142 −37"
 * over the edits since its last prompt; null when it edited nothing.
 */
export function turnSummary(events: AgentEvent[]): string | null {
  let from = 0;
  events.forEach((e, i) => {
    if (e.kind === "started") from = i + 1;
  });
  const files = new Set<string>();
  let added = 0;
  let removed = 0;
  for (const e of events.slice(from)) {
    if (e.kind !== "edit") continue;
    const m = EDIT.exec(e.text);
    if (!m) continue;
    files.add(m[1]);
    added += Number(m[2] ?? 0);
    removed += Number(m[3] ?? 0);
  }
  if (files.size === 0) return null;
  const counts = [added ? `+${added}` : "", removed ? `−${removed}` : ""].filter(Boolean).join(" ");
  return `${files.size} file${files.size === 1 ? "" : "s"} changed${counts ? `, ${counts}` : ""}`;
}

/** The last `n` non-empty lines of a screen, trailing blanks dropped. */
export function lastLines(lines: string[], n: number): string[] {
  return lines.map((l) => l.replace(/\s+$/, "")).filter((l) => l.trim() !== "").slice(-n);
}

/** The prompt of the last "Started “…”" among a Session's events (oldest first); null without one. */
export function lastPrompt(events: AgentEvent[]): string | null {
  for (let i = events.length - 1; i >= 0; i--) {
    if (events[i].kind !== "started") continue;
    const m = /^Started “(.*)”$/su.exec(events[i].text);
    if (m) return m[1];
  }
  return null;
}
