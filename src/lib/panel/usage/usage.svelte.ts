// The latest UsageSnapshot, read only while the Panel is showing (its header carries a summary
// even while the Usage view is closed), plus a clock for the "resets in" times.

import { onUsage, watchUsage } from "../../ipc";
import type { AgentKind, UsageSnapshot } from "../../types";

/** How often relative times ("2h 10m") are redrawn between snapshots. */
const CLOCK_MS = 30_000;

export const usage = $state<{ snapshot: UsageSnapshot | null; now: number }>({
  snapshot: null,
  now: Date.now(),
});

let watched: readonly AgentKind[] | null = null;
let stopListening: (() => void) | null = null;
let clock: ReturnType<typeof setInterval> | null = null;

/**
 * Read `agents`' usage; returns the function that stops. Watching again with other agents (the
 * Settings page changed them) replaces the watch without stopping in between, so the view keeps
 * its numbers until the new snapshot, which Rust sends at once.
 */
export function watch(agents: readonly AgentKind[]): () => void {
  watched = agents;
  if (!stopListening) {
    const listening = onUsage((snapshot) => {
      if (watched === null) return;
      usage.snapshot = snapshot;
      usage.now = Date.now();
    });
    stopListening = () => void listening.then((stop) => stop());
    clock = setInterval(() => (usage.now = Date.now()), CLOCK_MS);
  }
  void watchUsage(true, [...agents]);
  return () => {
    if (watched !== agents) return;
    watched = null;
    // A re-watch in the same update (new agents) lands before this runs and cancels the stop.
    queueMicrotask(() => {
      if (watched !== null) return;
      void watchUsage(false, []);
      stopListening?.();
      stopListening = null;
      if (clock !== null) clearInterval(clock);
      clock = null;
      usage.snapshot = null;
    });
  };
}
