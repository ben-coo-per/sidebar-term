// The latest UsageSnapshot, read only while something shows it (the Panel, whose header carries a
// summary even while the Usage view is closed, or the window bar), plus a clock for the
// "resets in" times.

import { onUsage, watchUsage } from "../../ipc";
import type { AgentKind, UsageSnapshot } from "../../types";

/** How often relative times ("2h 10m") are redrawn between snapshots. */
const CLOCK_MS = 30_000;

export const usage = $state<{ snapshot: UsageSnapshot | null; now: number }>({
  snapshot: null,
  now: Date.now(),
});

/** One entry per watch in force, newest last: the newest one's agents are the ones read. */
const watches: { agents: readonly AgentKind[] }[] = [];
let stopListening: (() => void) | null = null;
let clock: ReturnType<typeof setInterval> | null = null;

/**
 * Read `agents`' usage; returns the function that stops. Several may watch at once; reading stops
 * when the last one stops. Watching again with other agents (the Settings page changed them)
 * replaces the watch without stopping in between, so the view keeps its numbers until the new
 * snapshot, which Rust sends at once.
 */
export function watch(agents: readonly AgentKind[]): () => void {
  const entry = { agents };
  watches.push(entry);
  if (!stopListening) {
    const listening = onUsage((snapshot) => {
      if (watches.length === 0) return;
      usage.snapshot = snapshot;
      usage.now = Date.now();
    });
    stopListening = () => void listening.then((stop) => stop());
    clock = setInterval(() => (usage.now = Date.now()), CLOCK_MS);
  }
  void watchUsage(true, [...agents]);
  return () => {
    const at = watches.indexOf(entry);
    if (at === -1) return;
    watches.splice(at, 1);
    // A re-watch in the same update (new agents) lands before this runs and cancels the stop.
    queueMicrotask(() => {
      if (watches.length > 0) {
        void watchUsage(true, [...watches[watches.length - 1].agents]);
        return;
      }
      void watchUsage(false, []);
      stopListening?.();
      stopListening = null;
      if (clock !== null) clearInterval(clock);
      clock = null;
      usage.snapshot = null;
    });
  };
}
