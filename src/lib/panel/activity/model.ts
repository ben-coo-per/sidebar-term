// Pure helpers for the Activity view: sorting, number formatting, and the meter segments that
// split the machine's CPU and memory between Sessions and everything else.

import type { ActivityProcess, ActivitySnapshot, SessionId } from "../../types";

export type SortKey = "cpu" | "mem";

/**
 * Busiest first by `key`. Ties (most processes idle at 0% CPU) put Session processes before
 * others, then order by the other measure, then by name, so the list is stable between ticks.
 */
export function sortProcesses(processes: readonly ActivityProcess[], key: SortKey): ActivityProcess[] {
  const other: SortKey = key === "cpu" ? "mem" : "cpu";
  return [...processes].sort(
    (a, b) =>
      b[key] - a[key] ||
      Number(b.sessionId !== null) - Number(a.sessionId !== null) ||
      b[other] - a[other] ||
      a.name.localeCompare(b.name) ||
      a.pid - b.pid,
  );
}

/** Percent of one core, as Activity Monitor prints it: one decimal below 100, whole above. */
export function formatCpu(pct: number): string {
  return pct >= 99.95 ? Math.round(pct).toString() : pct.toFixed(1);
}

/** Binary units, as Activity Monitor prints them: "812 KB", "3.0 MB", "41.2 MB", "1.21 GB". */
export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = Math.max(0, bytes);
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  const digits = i === 0 || v >= 100 ? 0 : i >= 3 && v < 10 ? 2 : 1;
  return `${v.toFixed(digits)} ${units[i]}`;
}

/** Machine CPU load in percent (0..100): the process sum over every core's capacity. */
export function machineCpu(snapshot: ActivitySnapshot): number {
  return snapshot.cpuCount > 0 ? Math.min(100, snapshot.cpuTotal / snapshot.cpuCount) : 0;
}

/** One stretch of a meter: a Session's share, or everything else's (`sessionId` null). */
export interface Segment {
  sessionId: SessionId | null;
  /** Fraction of the whole meter, 0..1. */
  fraction: number;
}

/**
 * Meter segments for CPU (`cpu`) or memory (`mem`): one per Session with a non-zero share, in
 * `order` (the sidebar's Tab order; unknown Sessions last), then one for everything else.
 * CPU is out of every core's capacity; memory is out of physical memory, and "everything else"
 * is Memory Used minus the Sessions' resident memory.
 */
export function meterSegments(snapshot: ActivitySnapshot, measure: SortKey, order: readonly SessionId[]): Segment[] {
  const whole = measure === "cpu" ? snapshot.cpuCount * 100 : snapshot.memTotal;
  const used = measure === "cpu" ? snapshot.cpuTotal : snapshot.memUsed;
  if (whole <= 0) return [];
  const rank = (id: SessionId) => {
    const i = order.indexOf(id);
    return i === -1 ? order.length : i;
  };
  const sessions = snapshot.sessions
    .filter((s) => s[measure] > 0)
    .sort((a, b) => rank(a.sessionId) - rank(b.sessionId) || a.sessionId - b.sessionId);
  const segments: Segment[] = [];
  let left = 1;
  for (const s of sessions) {
    const fraction = Math.min(left, s[measure] / whole);
    segments.push({ sessionId: s.sessionId, fraction });
    left -= fraction;
  }
  const sessionTotal = sessions.reduce((sum, s) => sum + s[measure], 0);
  const rest = Math.min(left, Math.max(0, used - sessionTotal) / whole);
  if (rest > 0) segments.push({ sessionId: null, fraction: rest });
  return segments;
}

/** The `activity` settings section: whether Tabs show their CPU and memory (on by default). */
export function parseActivitySection(raw: unknown): { tabStats: boolean } {
  const r = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
  return { tabStats: r.tabStats !== false };
}

/** A Tab's stats line: "35% · 1.21 GB". */
export function formatTabStats(cpu: number, mem: number): string {
  return `${Math.round(cpu)}% · ${formatBytes(mem)}`;
}

/**
 * "Everything else" in the memory meter, broken down: the part macOS keeps (wired), the part the
 * compressor occupies, and the rest (other apps). Compressed pages of Session processes are in
 * their footprints too, so the compressor's share is capped at what is left.
 */
export function otherMemoryParts(snapshot: ActivitySnapshot): { wired: number; compressed: number; apps: number } {
  const inTabs = snapshot.sessions.reduce((sum, s) => sum + s.mem, 0);
  const other = Math.max(0, snapshot.memUsed - inTabs);
  const wired = Math.min(other, snapshot.memWired);
  const compressed = Math.min(other - wired, snapshot.memCompressed);
  return { wired, compressed, apps: other - wired - compressed };
}
