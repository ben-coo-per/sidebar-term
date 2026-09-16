// Pure rules for Memory Guard's webview side: its settings section, and how a frozen Tab is
// described. No state, no IPC. The policy itself is Rust's (src-tauri/src/guard.rs).

import type { FrozenSession } from "../types";

/** Limits offered in Settings, percent of physical memory. Rust clamps to 50..95. */
export const GUARD_LIMITS: readonly number[] = [70, 75, 80, 85, 90, 95];
export const DEFAULT_GUARD_LIMIT = 85;
/** Thaw once Memory Used is this many points under the limit (guard.rs `THAW_GAP`). */
export const THAW_GAP = 10;

/** The `memoryGuard` settings section. */
export interface GuardSettings {
  on: boolean;
  limitPercent: number;
}

/** A saved section, or the defaults (off, 85%) for anything missing or malformed. */
export function parseGuardSection(raw: unknown): GuardSettings {
  const r = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
  const limit = r.limitPercent;
  return {
    on: r.on === true,
    limitPercent: typeof limit === "number" && GUARD_LIMITS.includes(limit) ? limit : DEFAULT_GUARD_LIMIT,
  };
}

/** "3:41 PM", in the viewer's locale. */
function clock(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

/** A frozen Tab's tooltip. `mem` is already formatted ("1.21 GB"). */
export function frozenTitle(f: FrozenSession, mem: string): string {
  const by = f.manual ? "you" : "Memory Guard";
  return `Frozen by ${by} at ${clock(f.frozenAt)}, holding ${mem}. Go to this Tab to thaw it.`;
}

/** The Tray button's tooltip. `frozen` counts the Tabs Memory Guard froze, not those frozen by hand. */
export function guardButtonTitle(on: boolean, limitPercent: number, frozen: number): string {
  if (!on) return "Memory Guard: freeze the heaviest Tab when memory gets tight";
  const tabs = frozen === 0 ? "no Tab frozen" : frozen === 1 ? "1 Tab frozen" : `${frozen} Tabs frozen`;
  return `Memory Guard: on (freezes a Tab above ${limitPercent}% memory; ${tabs}). Click to turn off and thaw every Tab`;
}
