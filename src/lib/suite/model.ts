// Pure rules for showing a Suite: the text in the Tab's stats slot, the bar under its Badge, the
// tooltip, and the `suite` settings section. No state, no IPC, no clock: everything comes from
// the SuiteSnapshot Rust pushes each tick. Mockups: docs/research/test-progress.md "Presentation".

import type { SuiteSnapshot } from "../types";

/** What colours the bar and text: running (blue), an estimate (blue, half opacity), failures
 *  (red), longer than any run in the history (amber), finished (as running). */
export type SuiteTone = "running" | "estimate" | "failed" | "overrun" | "done";

export interface SuiteView {
  /** The stats slot: `12/48 · 2✗ · ~1m`, `1m 30s of ~2m 10s`, `testing · 1m 30s`. */
  text: string;
  /** For a narrow sidebar: the count alone, or the elapsed time. */
  short: string;
  /** Filled share of the bar, 0..1; null for an indeterminate (sliding) bar. */
  fraction: number | null;
  /** Share of the bar in red, at its left edge: failed / total. */
  failedFraction: number;
  tone: SuiteTone;
  /** The full line, for the row's tooltip. */
  title: string;
}

/** `42 s`, `1m 30s`, `4m 05s`, `1h 02m`. Whole seconds, rounded down. */
export function formatDuration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${String(m % 60).padStart(2, "0")}m`;
}

/** An ETA rounded as the eye wants it: to 5 s under a minute (never under 5 s), to 15 s above. */
export function roundEta(ms: number): number {
  if (ms < 60_000) return Math.max(5_000, Math.round(ms / 5_000) * 5_000);
  return Math.round(ms / 15_000) * 15_000;
}

/** `~45 s`, `~1m`, `~2m 15s`: a rounded ETA, minutes without `00s`. */
export function formatEta(ms: number): string {
  const rounded = roundEta(ms);
  const text = formatDuration(rounded);
  return `~${text.replace(/ 00s$/, "")}`;
}

/** How long the ETA is, in words, for a tooltip: `about 1 min left`, `about 45 s left`. */
function etaWords(ms: number): string {
  const rounded = roundEta(ms);
  if (rounded < 60_000) return `about ${rounded / 1000} s left`;
  const m = Math.floor(rounded / 60_000);
  const s = (rounded % 60_000) / 1000;
  return `about ${m} min${s ? ` ${s} s` : ""} left`;
}

/** The runner's display name on the Tab: `cargo test`, `go test`, the rest as they are. */
export function runnerLabel(runner: string): string {
  return runner === "cargo" || runner === "go" ? `${runner} test` : runner;
}

export function suiteView(s: SuiteSnapshot): SuiteView {
  const runner = runnerLabel(s.runner);
  const elapsed = formatDuration(s.elapsedMs);
  const failedText = s.failed > 0 ? `${s.failed} failed` : s.outcome === "failed" ? "failed" : "";

  if (s.phase === "done") {
    const count = s.total !== null ? `${s.done}/${s.total}` : s.done > 0 ? `${s.done} done` : "";
    const failed = s.failed > 0 || s.outcome === "failed";
    const text = [count, failedText].filter(Boolean).join(" · ") || "done";
    return {
      text,
      short: count || (failed ? "failed" : "done"),
      fraction: 1,
      failedFraction: s.total ? Math.min(1, s.failed / s.total) : failed ? 1 : 0,
      tone: failed ? "failed" : "done",
      title: [runner, `finished in ${elapsed}`, count.replace("/", " of "), failedText].filter(Boolean).join(" · "),
    };
  }

  if (s.phase === "building") {
    return {
      text: `building · ${elapsed}`,
      short: elapsed,
      fraction: null,
      failedFraction: 0,
      tone: "running",
      title: `${runner} · building · ${elapsed}`,
    };
  }

  const approx = s.runs === 1 ? "about " : "~";
  const fromRuns = s.runs === 1 ? "from 1 run" : `from ${s.runs} runs`;

  if (s.source === "stream") {
    const eta = s.etaMs !== null ? formatEta(s.etaMs) : null;
    const failed = s.failed > 0 ? `${s.failed}✗` : null;
    if (s.total !== null) {
      const count = `${s.done}/${s.total}`;
      return {
        text: [count, failed, eta].filter(Boolean).join(" · "),
        short: count,
        fraction: Math.min(1, s.done / s.total),
        failedFraction: Math.min(1, s.failed / s.total),
        tone: s.failed > 0 ? "failed" : "running",
        title: [runner, `${s.done} of ${s.total}`, failedText, elapsed + (s.etaMs !== null ? `, ${etaWords(s.etaMs)}` : "")]
          .filter(Boolean)
          .join(" · "),
      };
    }
    // Tests done but no total (jest): the bar follows the history when there is one.
    const count = `${s.done} done`;
    return {
      text: [count, failed, eta ?? elapsed].filter(Boolean).join(" · "),
      short: count,
      fraction: s.typicalMs ? Math.min(1, s.elapsedMs / s.typicalMs) : null,
      failedFraction: 0,
      tone: s.failed > 0 ? "failed" : "running",
      title: [runner, count, failedText, elapsed + (s.etaMs !== null ? `, ${etaWords(s.etaMs)}` : "")].filter(Boolean).join(" · "),
    };
  }

  if (s.typicalMs !== null && s.typicalMs > 0) {
    const typical = formatDuration(s.typicalMs);
    if (s.longestMs !== null && s.elapsedMs > s.longestMs) {
      return {
        text: `${elapsed} · longer than usual`,
        short: elapsed,
        fraction: 1,
        failedFraction: 0,
        tone: "overrun",
        title: `${runner} · ${elapsed}, longer than any of the last ${s.runs === 1 ? "run" : `${s.runs} runs`} (longest ${formatDuration(s.longestMs)})`,
      };
    }
    if (s.elapsedMs > s.typicalMs) {
      return {
        text: `${elapsed} · usually ${approx}${typical}`,
        short: elapsed,
        fraction: 1,
        failedFraction: 0,
        tone: "estimate",
        title: `${runner} · ${elapsed}, usually ${approx}${typical} (${fromRuns})`,
      };
    }
    return {
      text: `${elapsed} of ${approx}${typical}`,
      short: elapsed,
      fraction: Math.min(1, s.elapsedMs / s.typicalMs),
      failedFraction: 0,
      tone: "estimate",
      title: `${runner} · ${elapsed} of ${approx}${typical}${s.etaMs !== null ? `, ${etaWords(s.etaMs)}` : ""} (${fromRuns})`,
    };
  }

  return {
    text: `testing · ${elapsed}`,
    short: elapsed,
    fraction: null,
    failedFraction: 0,
    tone: "running",
    title: `${runner} · testing · ${elapsed} (no earlier run to compare with)`,
  };
}

/** The Suite shown on a Tab when its Session runs several: the newest. */
export function newestSuite(suites: readonly SuiteSnapshot[]): SuiteSnapshot | null {
  let best: SuiteSnapshot | null = null;
  for (const s of suites) {
    if (!best || s.startedAt > best.startedAt || (s.startedAt === best.startedAt && s.id > best.id)) best = s;
  }
  return best;
}

/** Suites for the Panel's Tests block: newest first. */
export function sortSuites(suites: readonly SuiteSnapshot[]): SuiteSnapshot[] {
  return [...suites].sort((a, b) => b.startedAt - a.startedAt || b.id - a.id);
}

/** The `suite` settings section: whether new Sessions get the progress variables (on by default). */
export function parseSuiteSection(raw: unknown): { progress: boolean } {
  const r = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
  return { progress: r.progress !== false };
}
