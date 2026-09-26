// The Suites Rust knows about, mirrored from the `suite` event: every live (or just finished)
// test suite under any Session. The Tab row shows its Session's newest; the Activity view lists
// them all. Pure rules in ./model.ts.

import { onSuite } from "../ipc";
import type { SessionId, SuiteSnapshot } from "../types";
import { newestSuite } from "./model";

export const suites = $state<{ list: SuiteSnapshot[] }>({ list: [] });

/** Follow Rust's Suites; returns the function that stops following. */
export function initSuites(): () => void {
  const listening = onSuite((list) => {
    suites.list = list;
  });
  return () => void listening.then((stop) => stop());
}

/** The Suite a Tab shows: its Session's newest, or null. */
export function suiteForSession(sessionId: SessionId | null): SuiteSnapshot | null {
  if (sessionId === null) return null;
  return newestSuite(suites.list.filter((s) => s.sessionId === sessionId));
}
