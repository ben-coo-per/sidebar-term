// Memory Guard: Rust freezes the heaviest Tab when memory gets tight and thaws it once memory
// frees up (src-tauri/src/guard.rs). This mirrors its state for the Tray button and the Tabs, keeps
// whether it is on and its limit in the `memoryGuard` settings section, and tells Rust which
// Session is in view.

import { guardState, guardVisible, onGuard, setGuard } from "../ipc";
import { loadSection, saveSection } from "../settings/store";
import type { FrozenSession, GuardSnapshot, SessionId } from "../types";
import { DEFAULT_GUARD_LIMIT, parseGuardSection } from "./model";

export const memoryGuard = $state<{ on: boolean; limitPercent: number; frozen: FrozenSession[] }>({
  on: false,
  limitPercent: DEFAULT_GUARD_LIMIT,
  frozen: [],
});

function apply(s: GuardSnapshot) {
  memoryGuard.on = s.on;
  memoryGuard.limitPercent = s.limitPercent;
  memoryGuard.frozen = s.frozen;
}

/** The Session's freeze, if Memory Guard has it frozen. */
export function frozenSession(sessionId: SessionId | null): FrozenSession | undefined {
  return sessionId === null ? undefined : memoryGuard.frozen.find((f) => f.sessionId === sessionId);
}

/** Follow Rust's state and turn Memory Guard on as saved; returns the function that stops following. */
export function initMemoryGuard(): () => void {
  const listening = onGuard(apply);
  void (async () => {
    // What Rust has now (a webview reload keeps it), then what Settings says.
    apply(await guardState());
    const saved = parseGuardSection(await loadSection("memoryGuard"));
    apply(await setGuard(saved.on, saved.limitPercent));
  })().catch((e) => console.error("memory guard:", e));
  return () => void listening.then((stop) => stop());
}

async function set(on: boolean, limitPercent: number): Promise<void> {
  try {
    apply(await setGuard(on, limitPercent));
    saveSection("memoryGuard", { on: memoryGuard.on, limitPercent: memoryGuard.limitPercent });
  } catch (e) {
    console.error("memory guard:", e);
  }
}

/** Turn Memory Guard on or off (off thaws every frozen Tab). */
export function toggleMemoryGuard(): Promise<void> {
  return set(!memoryGuard.on, memoryGuard.limitPercent);
}

export function setGuardLimit(limitPercent: number): Promise<void> {
  return set(memoryGuard.on, limitPercent);
}

/** Tell Rust which Session is in view: never frozen, and thawed if it was. */
export function setVisibleSession(sessionId: SessionId | null): void {
  void guardVisible(sessionId).catch((e) => console.error("memory guard:", e));
}
