// Manager: the window mode that shows every agent Tab as a lane, the questions agents are
// waiting on (answered in place from their hooks, or "Open Tab" for a screen-only agent), the
// finished work not looked at yet, and every agent's events. It has no list of its own: it reads
// the same Tabs (src/lib/layout.svelte.ts) and Session facts (src/lib/sessions.svelte.ts) as the
// sidebar. This module holds what Manager adds: the clock, the lane order it holds while the
// pointer is over the lanes, which card and lane have focus, and the "Sent" rows of answers
// given. See docs/architecture.md "Manager" and CONTEXT.md.

import { activateTab, activeTab, layout, orderedTabIds, setMode, tabIdForSession, type Tab } from "../layout.svelte";
import { hostHome, sessionOf, tabIsUnread, tabTitle } from "../sessions.svelte";
import { agentAnswer, agentRelease } from "../ipc";
import { hostAnswer, hostName, hostRelease } from "../host/hosts.svelte";
import { isLocal, sessionKey } from "../host/ids";
import type { AgentKind, AgentStatus, GitInfo, Pending, StatusChange } from "../types";
import { feed, newestFirst, type HostAgentEvent } from "./feed.svelte";
import {
  agentLabel,
  currentSince,
  holdOrder,
  laneKind,
  laneStart,
  lastPrompt,
  sortLanes,
  turnSummary,
  type AgentLabel,
  type LaneKind,
} from "./model";

/** How often the lanes and relative times redraw. */
export const CLOCK_MS = 30_000;
/** How long a Sent row stays once its agent is back at work. */
export const SENT_LINGER_MS = 4_000;
/** Feed rows shown at most. */
const FEED_SHOWN = 200;

/** An answer given from a card, shown in the card's place until its agent picks up again. */
export interface Sent {
  tabId: string;
  title: string;
  choice: string;
  pendingId: number;
  /** The agent's status left Needs input: the row fades out from then. */
  resumed: boolean;
  fading: boolean;
}

export const manager = $state({
  /** Manager's clock: moves every `CLOCK_MS` while it shows. */
  now: Date.now(),
  /** The pointer is over the lanes: they keep their order until it leaves. */
  hovering: false,
  /** The lane order last shown (Tab ids). */
  shown: [] as string[],
  /** The card with the focus ring (Tab id); null: the oldest. */
  cardFocus: null as string | null,
  /** The lane with the focus ring (↑ / ↓), if any. */
  laneFocus: null as string | null,
  sent: [] as Sent[],
  /** Where Manager's columns were scrolled, restored when it shows again. */
  scroll: { lanes: 0, needs: 0, feed: 0 },
});

/** One agent Tab, as a lane. */
export interface Lane extends AgentLabel {
  tab: Tab;
  /** The Tab's Title, for tooltips; Manager shows `project` and `description`. */
  title: string;
  agent: AgentKind;
  kind: LaneKind;
  status: AgentStatus | null;
  /** When its current status began (for a question: when it was asked). */
  since: number;
  /** When the lane began. */
  start: number;
  history: StatusChange[];
  hooked: boolean;
  pending: Pending | null;
  unread: boolean;
  git: GitInfo | null;
  remote: boolean;
  /** The paired Host's name for a Tab on one; null on this Mac. */
  host: string | null;
}

/** Every Tab whose Session runs an agent now, in sidebar order. */
function agentLanes(): Lane[] {
  const out: Lane[] = [];
  for (const id of orderedTabIds()) {
    const tab = layout.tabs[id];
    const info = tab ? sessionOf(tab)?.info : null;
    if (!tab || !info?.agent) continue;
    const history = info.history ?? [];
    const pending = info.pending ?? null;
    out.push({
      tab,
      title: tabTitle(tab),
      ...labelOf(tab),
      agent: info.agent,
      kind: laneKind(info.status),
      status: info.status,
      since: pending?.since ?? currentSince(history, manager.now),
      start: laneStart(history) ?? manager.now,
      history,
      hooked: info.hooked ?? false,
      pending,
      unread: tabIsUnread(tab),
      git: info.git,
      remote: info.remote,
      host: isLocal(tab.host) ? null : hostName(tab.host),
    });
  }
  return out;
}

/** The lanes in the order to show: blocked first, held while the pointer is over them. */
export function lanes(): Lane[] {
  const all = agentLanes();
  const sorted = sortLanes(all.map((l, order) => ({ key: l.tab.id, kind: l.kind, since: l.since, order, lane: l })));
  const order = holdOrder(manager.shown, sorted.map((s) => s.key), manager.hovering);
  const byId = new Map(all.map((l) => [l.tab.id, l]));
  return order.map((id) => byId.get(id)).filter((l): l is Lane => !!l);
}

/** Remember the order shown (Manager calls this after each render of the lanes). */
export function rememberOrder(ids: string[]): void {
  if (ids.length !== manager.shown.length || ids.some((id, i) => manager.shown[i] !== id)) manager.shown = ids;
}

/** A Sent row that stands in for this question. */
function sentFor(lane: Lane): Sent | undefined {
  return manager.sent.find((s) => s.tabId === lane.tab.id && (lane.pending === null || lane.pending.id === s.pendingId));
}

/** The agents waiting on the user, oldest question first: Manager's cards. */
export function waiting(): Lane[] {
  return agentLanes()
    .filter((l) => l.kind === "waiting" && !sentFor(l))
    .sort((a, b) => a.since - b.since);
}

/** The card with the focus ring: the one the user moved to, else the oldest. */
export function focusedCard(cards: Lane[]): Lane | null {
  return cards.find((c) => c.tab.id === manager.cardFocus) ?? cards[0] ?? null;
}

/** Move the card focus by `step`, wrapping. */
export function moveCardFocus(cards: Lane[], step: 1 | -1): void {
  if (cards.length === 0) return;
  const at = Math.max(0, cards.indexOf(focusedCard(cards)!));
  manager.cardFocus = cards[(at + step + cards.length) % cards.length].tab.id;
  manager.laneFocus = null;
}

/** Move the lane focus by `step`, stopping at the ends. */
export function moveLaneFocus(shown: Lane[], step: 1 | -1): void {
  if (shown.length === 0) return;
  const at = shown.findIndex((l) => l.tab.id === manager.laneFocus);
  const next = at === -1 ? (step === 1 ? 0 : shown.length - 1) : Math.min(shown.length - 1, Math.max(0, at + step));
  manager.laneFocus = shown[next].tab.id;
}

/** Tabs whose agent finished or stopped while nobody looked, newest first. */
export interface Finished extends AgentLabel {
  tab: Tab;
  title: string;
  summary: string;
  since: number;
}

export function finished(): Finished[] {
  const out: Finished[] = [];
  for (const id of orderedTabIds()) {
    const tab = layout.tabs[id];
    const s = tab ? sessionOf(tab) : null;
    if (!tab || !s) continue;
    const stopped = s.info?.agent && s.status === "done" && tabIsUnread(tab);
    if (!s.finished && !stopped) continue;
    const history = s.info?.history ?? [];
    out.push({
      tab,
      title: tabTitle(tab),
      ...labelOf(tab),
      summary: turnSummary(sessionEvents(tab)) ?? (s.finished ? "Agent exited" : ""),
      since: currentSince(history, manager.now),
    });
  }
  return out.sort((a, b) => b.since - a.since);
}

/** One Tab's agent events, oldest first. */
function sessionEvents(tab: Tab): HostAgentEvent[] {
  if (tab.sessionId === null) return [];
  return (feed.byHost[tab.host] ?? []).filter((e) => e.sessionId === tab.sessionId);
}

/**
 * What Manager calls a Tab's agent: its project and a few words on what it is at (`agentLabel`),
 * from the Session's facts, its title and the last prompt in its events.
 */
function labelOf(tab: Tab): AgentLabel {
  const s = sessionOf(tab);
  const remote = s?.info?.remote ?? false;
  return agentLabel({
    customTitle: tab.customTitle,
    git: remote ? null : (s?.info?.git ?? null),
    cwd: remote ? null : (s?.info?.cwd ?? tab.lastCwd),
    home: hostHome(tab.host),
    oscTitle: s?.info?.title ?? s?.title ?? null,
    agent: s?.info?.agent ?? null,
    lastPrompt: lastPrompt(sessionEvents(tab)),
  });
}

/** A feed row: the event, and what its Tab's agent is called (a gone Tab keeps a plain name). */
export interface FeedRow extends AgentLabel {
  event: HostAgentEvent;
  title: string;
}

export function feedRows(): FeedRow[] {
  const labels = new Map<string, AgentLabel & { title: string }>();
  return newestFirst()
    .slice(0, FEED_SHOWN)
    .map((event) => {
      const tabId = tabIdForSession(sessionKey(event.host, event.sessionId));
      const tab = tabId ? layout.tabs[tabId] : null;
      if (!tab) return { event, title: "Closed Tab", project: "Closed Tab", description: null };
      let label = labels.get(tab.id);
      if (!label) {
        label = { title: tabTitle(tab), ...labelOf(tab) };
        labels.set(tab.id, label);
      }
      return { event, ...label };
    });
}

// --- Actions -----------------------------------------------------------------------------------

function report(what: string): (e: unknown) => void {
  return (e) => console.error(`manager: ${what} failed`, e);
}

/** Let go of a question held for a Tab's agent, so it asks in its Terminal. */
function release(tab: Tab, pendingId: number): Promise<void> {
  if (tab.sessionId === null) return Promise.resolve();
  return isLocal(tab.host) ? agentRelease(tab.sessionId, pendingId) : hostRelease(tab.host, tab.sessionId, pendingId);
}

/**
 * Open a Tab in the default view (Tabs mode). A question its agent is waiting on moves to its
 * Terminal, where the user is about to look.
 */
export function openTab(tab: Tab): void {
  const pending = sessionOf(tab)?.info?.pending;
  if (pending) void release(tab, pending.id).catch(report("letting go of a question"));
  setMode("tabs");
  activateTab(tab.id);
}

/**
 * Answer `lane`'s question with option `index` (0-based). The card gives way to a Sent row at
 * once; focus moves to the next card. There is no undo: the answer has reached the agent.
 */
export async function answer(lane: Lane, index: number): Promise<void> {
  const { tab, pending } = lane;
  if (!pending || tab.sessionId === null || index < 0 || index >= pending.options.length) return;
  const cards = waiting();
  const at = cards.findIndex((c) => c.tab.id === tab.id);
  const next = at >= 0 ? (cards[at + 1] ?? null) : null;
  // A new answer replaces older Sent rows.
  manager.sent = [{ tabId: tab.id, title: lane.title, choice: pending.options[index], pendingId: pending.id, resumed: false, fading: false }];
  manager.cardFocus = next?.tab.id ?? null;
  try {
    if (isLocal(tab.host)) await agentAnswer(tab.sessionId, pending.id, index);
    else await hostAnswer(tab.host, tab.sessionId, pending.id, index);
  } catch (e) {
    // Not sent (answered elsewhere, or the agent moved on): the card comes back if it still waits.
    manager.sent = manager.sent.filter((s) => s.pendingId !== pending.id);
    report("answering")(e);
  }
}

// --- Keeping it current ------------------------------------------------------------------------

$effect.root(() => {
  // Sent rows: "resuming" until the agent leaves Needs input, then fade after a moment.
  $effect(() => {
    for (const s of manager.sent) {
      if (s.resumed) continue;
      const tab = layout.tabs[s.tabId];
      const info = tab ? sessionOf(tab)?.info : null;
      if (info && info.status === "needs-input" && (info.pending === null || info.pending.id === s.pendingId)) continue;
      s.resumed = true;
      setTimeout(() => {
        s.fading = true;
      }, SENT_LINGER_MS);
      setTimeout(() => {
        manager.sent = manager.sent.filter((x) => x !== s);
      }, SENT_LINGER_MS + 400);
    }
  });

  // In Tabs mode, a question the Tab in view is asked belongs in its Terminal, which the user
  // is looking at: the Host stops holding it.
  const released = new Set<string>();
  $effect(() => {
    if (layout.mode !== "tabs") return;
    const tab = activeTab();
    const pending = tab ? sessionOf(tab)?.info?.pending : null;
    if (!tab || !pending) return;
    const key = `${tab.id}:${pending.id}`;
    if (released.has(key)) return;
    released.add(key);
    void release(tab, pending.id).catch(report("letting go of a question"));
  });
});

/** Run Manager's clock while it shows; returns the function that stops it. */
export function runClock(): () => void {
  manager.now = Date.now();
  const timer = setInterval(() => (manager.now = Date.now()), CLOCK_MS);
  return () => clearInterval(timer);
}
