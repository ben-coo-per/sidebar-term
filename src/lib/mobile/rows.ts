// What a Tab's row on the phone shows, derived from the layout and Session facts of the Hosts the
// phone reaches, as the Mac's sidebar derives its own (src/lib/sessions.svelte.ts): the Title (a
// rename, else the automatic Title), the agent and its status, the Badge facts, and the Host a
// linked Tab's Session runs on. The Groups are those of the Host the page came from; a linked Tab
// (ADR 0003) takes everything else from the Host it points at, which the phone reaches itself
// (ADR 0004). Pure.

import { agentLabel, computeAutomaticTitle, type AgentLabel } from "../agentStatus";
import { LOCAL_HOST, type HostId } from "../host/ids";
import { lastPrompt } from "../manager/model";
import type { AgentEvent, AgentKind, AgentStatus, GitInfo, Group, LayoutSnapshot, SessionId, SessionInfo, Tab } from "../types";

export type Facts = Record<SessionId, SessionInfo>;

/** What the phone knows of one Host: the one its page came from (`LOCAL_HOST`), or one its linked Tabs point at. */
export interface HostView {
  id: HostId;
  /** The Host's name: the list's heading for the page's own Host, a row's chip for another. */
  name: string;
  /** The phone holds a token for it. */
  paired: boolean;
  /** Connected now: its Sessions can be attached. */
  online: boolean;
  /** The Host's home directory, for the `~` in automatic Titles. */
  home: string | null;
  /** Its layout as last heard; null before the first. */
  layout: LayoutSnapshot | null;
  sessions: Facts;
  /** Its agent events, oldest first. */
  events: AgentEvent[];
}

export type HostViews = Record<HostId, HostView>;

export interface TabRow {
  /** Unique among the rows: the Tab's id in the layout that places it; `<host>/<tab>` for a Tab of another Host not placed yet. */
  id: string;
  /** The Host its Session runs on. */
  host: HostId;
  /** That Host's name, for the row's chip; null on the Host the page came from. */
  hostName: string | null;
  /** The Host is connected: the row can be opened. */
  reachable: boolean;
  /** null while the Tab has no Session (its shell failed to spawn, or its Host was never heard): not attachable. */
  sessionId: SessionId | null;
  title: string;
  agent: AgentKind | null;
  /** Running / Needs input / Done from the Host; null when not an Agent session. */
  status: AgentStatus | null;
  git: GitInfo | null;
  remote: boolean;
  /** What Manager calls its agent; null when not an Agent session. */
  label: AgentLabel | null;
  /** The Session's facts, whole; null before its Host said any. */
  info: SessionInfo | null;
}

export interface GroupRows {
  id: string;
  name: string;
  tabs: TabRow[];
}

/** A Host a linked Tab points at and the phone holds no token for, with how many Tabs wait behind it. */
export interface Unpaired {
  host: HostId;
  name: string;
  tabs: number;
}

/** One Session's agent events, oldest first. */
export function sessionEvents(host: HostView, sessionId: SessionId | null): AgentEvent[] {
  return sessionId === null ? [] : host.events.filter((e) => e.sessionId === sessionId);
}

/** The row of `tab`, a Tab of `host`'s own layout, under the id `id`. */
export function tabRow(tab: Tab, host: HostView, id: string = tab.id): TabRow {
  const info = tab.sessionId !== null ? (host.sessions[tab.sessionId] ?? null) : null;
  const agent = info?.agent ?? null;
  const git = info?.remote ? null : (info?.git ?? null);
  const prompt = agent ? lastPrompt(sessionEvents(host, tab.sessionId)) : null;
  const title =
    tab.customTitle ||
    computeAutomaticTitle({
      agent,
      git,
      lastPrompt: prompt,
      oscTitle: info?.title ?? null,
      foreground: info?.foreground ?? null,
      shellIsForeground: info?.shellIsForeground ?? true,
      cwd: info?.cwd ?? tab.lastCwd,
      home: host.home,
    });
  const label = agent
    ? agentLabel({
        customTitle: tab.customTitle,
        git,
        cwd: info?.remote ? null : (info?.cwd ?? tab.lastCwd),
        home: host.home,
        oscTitle: info?.title ?? null,
        agent,
        lastPrompt: prompt,
      })
    : null;
  return {
    id,
    host: host.id,
    hostName: host.id === LOCAL_HOST ? null : host.name,
    reachable: host.online,
    sessionId: tab.sessionId,
    title,
    agent,
    status: info?.status ?? null,
    git: info?.git ?? null,
    remote: info?.remote ?? false,
    label,
    info,
  };
}

/**
 * The row of a linked Tab of the page's Host: the Tab it points at, as its own Host describes
 * it. A placeholder while that Host was never heard; nothing (null) when the phone is not paired
 * with it, or when it no longer has the Tab (the link goes once the Mac hears so).
 */
function linkedRow(tab: Tab, hosts: HostViews): TabRow | null {
  const link = tab.link;
  const host = link ? hosts[link.hostId] : undefined;
  if (!link || !host || !host.paired) return null;
  if (!host.layout) {
    return {
      id: tab.id,
      host: host.id,
      hostName: host.name,
      reachable: false,
      sessionId: null,
      title: tab.customTitle ?? "…",
      agent: null,
      status: null,
      git: null,
      remote: false,
      label: null,
      info: null,
    };
  }
  const there = host.layout.tabs[link.tabId];
  return there ? tabRow(there, host, tab.id) : null;
}

/** The id of the row of a Tab of `host` that no layout of the page's Host places. */
export function strayId(host: HostId, tabId: string): string {
  return `${host}/${tabId}`;
}

/**
 * Every Group of the page's Host with its Tabs' rows, in sidebar order, linked Tabs included.
 * A Tab another Host has that no link places (made there since the Mac last looked) goes to the
 * end of the Group named after its Host, as the Mac will link it; made here when there is none.
 */
export function groupRows(hosts: HostViews): GroupRows[] {
  const home = hosts[LOCAL_HOST];
  if (!home?.layout) return [];
  const layout = home.layout;
  const linked = new Set<string>();
  const groups: GroupRows[] = layout.groups.map((g: Group) => ({
    id: g.id,
    name: g.name,
    tabs: g.tabIds.flatMap((id) => {
      const tab = layout.tabs[id];
      if (!tab) return [];
      if (!tab.link) return [tabRow(tab, home)];
      linked.add(strayId(tab.link.hostId, tab.link.tabId));
      const row = linkedRow(tab, hosts);
      return row ? [row] : [];
    }),
  }));
  for (const host of Object.values(hosts)) {
    if (host.id === LOCAL_HOST || !host.paired || !host.layout) continue;
    const there = host.layout;
    const strays = there.groups
      .flatMap((g) => g.tabIds)
      .filter((id) => there.tabs[id] && !linked.has(strayId(host.id, id)))
      .map((id) => tabRow(there.tabs[id], host, strayId(host.id, id)));
    if (strays.length === 0) continue;
    const named = groups.find((g) => g.name === host.name);
    if (named) named.tabs.push(...strays);
    else groups.push({ id: `host:${host.id}`, name: host.name, tabs: strays });
  }
  return groups;
}

/** The Hosts linked Tabs point at that the phone is not paired with, in the order first met. */
export function unpairedHosts(hosts: HostViews): Unpaired[] {
  const home = hosts[LOCAL_HOST];
  const out = new Map<HostId, Unpaired>();
  for (const host of Object.values(hosts)) {
    if (host.id !== LOCAL_HOST && !host.paired) out.set(host.id, { host: host.id, name: host.name, tabs: 0 });
  }
  for (const tab of Object.values(home?.layout?.tabs ?? {})) {
    const waiting = tab.link ? out.get(tab.link.hostId) : undefined;
    if (waiting) waiting.tabs += 1;
  }
  return [...out.values()];
}

/** The row with id `id`, or null when no layout has its Tab any more. */
export function findRow(hosts: HostViews, id: string): TabRow | null {
  for (const group of groupRows(hosts)) {
    const row = group.tabs.find((t) => t.id === id);
    if (row) return row;
  }
  return null;
}
