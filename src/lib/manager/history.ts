// A Session's status history kept by this client, for a Host whose core is older than Manager
// and sends none (its SessionInfo has no `history`). The rules are the Host's
// (src-tauri/core/src/agents/mod.rs `decorate_at`): the first change only once an agent status
// appears, `null` when the agent leaves, and about 5 h kept, with the change in force at the
// window's start. Pure: the clock is passed in.

import type { AgentEvent, AgentStatus, StatusChange } from "../types";

/** How far back the history goes, as on the Host. */
export const HISTORY_MS = 5 * 60 * 60 * 1000;
/** Changes kept at most, as on the Host. */
const HISTORY_MAX = 2000;

/** `history` with `status` seen at `now`: a new change when it differs, then trimmed. */
export function recordStatus(history: StatusChange[], status: AgentStatus | null, now: number): StatusChange[] {
  const last = history[history.length - 1];
  const changed = last ? last.status !== status : status !== null;
  const next = changed ? [...history, { status, at: now }] : history;
  const cutoff = now - HISTORY_MS;
  let old = 0;
  while (old < next.length && next[old].at < cutoff) old++;
  const drop = Math.max(old - 1, next.length - HISTORY_MAX, 0);
  return drop > 0 ? next.slice(drop) : next;
}

/** The feed line a screen-only agent's change to `status` makes, as on the Host; null for none. */
export function screenOnlyEvent(status: AgentStatus | null): Pick<AgentEvent, "kind" | "text"> | null {
  switch (status) {
    case "running":
      return { kind: "command", text: "Working (screen only)" };
    case "needs-input":
      return { kind: "asked", text: "Waiting on you (screen only)" };
    case "done":
      return { kind: "idle", text: "Idle at prompt (screen only)" };
    default:
      return null;
  }
}
