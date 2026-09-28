// Manager on the phone: every Agent session among the rows as a Lane, in the three lists the
// phone shows (waiting on you, working, idle), by the rules of the Mac's Manager
// (src/lib/manager/model.ts). The phone keeps no Unread, so every idle agent is listed, the one
// that stopped last first. Pure: the clock is passed in.

import type { AgentLabel } from "../agentStatus";
import { currentSince, laneKind, turnSummary, type LaneKind } from "../manager/model";
import type { AgentKind, Pending, StatusChange } from "../types";
import { sessionEvents, type GroupRows, type HostViews, type TabRow } from "./rows";

/** One Agent session, as the phone's Manager lists it. */
export interface Lane extends AgentLabel {
  row: TabRow;
  agent: AgentKind;
  kind: LaneKind;
  /** When its current status began (for a question: when it was asked). */
  since: number;
  history: StatusChange[];
  hooked: boolean;
  /** The question it waits on, when its hooks carried one. */
  pending: Pending | null;
  /** The last thing it did, in its Host's words ("Updated remote/pairing.rs (+61)"); null when nothing is known. */
  doing: string | null;
  /** What its last turn changed ("6 files changed, +142 −37"); null when it edited nothing. */
  summary: string | null;
}

export interface Lanes {
  /** Waiting on the user, the oldest question first. */
  waiting: Lane[];
  /** At work, in sidebar order. */
  working: Lane[];
  /** At their prompt, the one that stopped last first. */
  idle: Lane[];
}

/** The events that say what an agent is doing: not the questions, which the cards show. */
const DOING = new Set(["started", "edit", "read", "command", "failed"]);

function lane(row: TabRow, hosts: HostViews, now: number): Lane | null {
  const info = row.info;
  const host = hosts[row.host];
  if (!info?.agent || !row.label || !host) return null;
  const history = info.history ?? [];
  const pending = info.pending ?? null;
  const events = sessionEvents(host, row.sessionId);
  const last = events.findLast((e) => DOING.has(e.kind));
  return {
    ...row.label,
    row,
    agent: info.agent,
    kind: laneKind(info.status),
    since: pending?.since ?? currentSince(history, now),
    history,
    hooked: info.hooked ?? false,
    pending,
    doing: last?.text ?? null,
    summary: turnSummary(events),
  };
}

/** Every Agent session among `groups`, sorted into the phone's three lists. */
export function lanes(groups: GroupRows[], hosts: HostViews, now: number): Lanes {
  const all = groups.flatMap((g) => g.tabs).flatMap((row) => lane(row, hosts, now) ?? []);
  return {
    waiting: all.filter((l) => l.kind === "waiting").sort((a, b) => a.since - b.since),
    working: all.filter((l) => l.kind === "working"),
    idle: all.filter((l) => l.kind === "idle").sort((a, b) => b.since - a.since),
  };
}
