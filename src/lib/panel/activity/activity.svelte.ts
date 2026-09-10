// The latest ActivitySnapshot, sampled only while something watches it. The Panel watches while
// it shows the Activity view (collapsed or not: its header carries the summary).

import { onActivity, watchActivity } from "../../ipc";
import type { ActivitySnapshot } from "../../types";

export const activity = $state<{ snapshot: ActivitySnapshot | null }>({ snapshot: null });

let watchers = 0;
let unlisten: (() => void) | null = null;

/** Start sampling; returns the function that stops it. Nested watches are counted. */
export function watch(): () => void {
  if (watchers++ === 0) {
    const listening = onActivity((snapshot) => {
      if (watchers > 0) activity.snapshot = snapshot;
    });
    unlisten = () => void listening.then((stop) => stop());
    void watchActivity(true);
  }
  let stopped = false;
  return () => {
    if (stopped) return;
    stopped = true;
    if (--watchers === 0) {
      void watchActivity(false);
      unlisten?.();
      unlisten = null;
      activity.snapshot = null;
    }
  };
}
