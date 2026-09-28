// What a Tab's row on the phone shows, derived from the Host's layout and Session facts as the
// Mac's sidebar derives its own (src/lib/sessions.svelte.ts): the Title (a rename, else the
// automatic Title), the agent and its status, the Badge facts. Pure.

import { computeAutomaticTitle } from "../agentStatus";
import type { AgentKind, AgentStatus, GitInfo, Group, LayoutSnapshot, SessionId, SessionInfo, Tab } from "../types";

export interface TabRow {
  id: string;
  /** null while the Tab has no Session (its shell failed to spawn): not attachable. */
  sessionId: SessionId | null;
  title: string;
  agent: AgentKind | null;
  /** Running / Needs input / Done from the Host; null when not an Agent session. */
  status: AgentStatus | null;
  git: GitInfo | null;
  remote: boolean;
}

export interface GroupRows {
  id: string;
  name: string;
  tabs: TabRow[];
}

export type Facts = Record<SessionId, SessionInfo>;

/** One Tab's row. `home` is the Host's, for the `~` in automatic Titles. */
export function tabRow(tab: Tab, facts: Facts, home: string | null): TabRow {
  const info = tab.sessionId !== null ? (facts[tab.sessionId] ?? null) : null;
  const title =
    tab.customTitle ||
    computeAutomaticTitle({
      agent: info?.agent ?? null,
      git: info?.remote ? null : (info?.git ?? null),
      oscTitle: info?.title ?? null,
      foreground: info?.foreground ?? null,
      shellIsForeground: info?.shellIsForeground ?? true,
      cwd: info?.cwd ?? tab.lastCwd,
      home,
    });
  return {
    id: tab.id,
    sessionId: tab.sessionId,
    title,
    agent: info?.agent ?? null,
    status: info?.status ?? null,
    git: info?.git ?? null,
    remote: info?.remote ?? false,
  };
}

/** Every Group with its Tabs' rows, in sidebar order. */
export function groupRows(layout: LayoutSnapshot, facts: Facts, home: string | null): GroupRows[] {
  return layout.groups.map((g: Group) => ({
    id: g.id,
    name: g.name,
    tabs: g.tabIds.flatMap((id) => {
      const tab = layout.tabs[id];
      return tab ? [tabRow(tab, facts, home)] : [];
    }),
  }));
}

/** The row of Tab `tabId`, or null when the layout no longer has it. */
export function findRow(layout: LayoutSnapshot | null, facts: Facts, home: string | null, tabId: string): TabRow | null {
  const tab = layout?.tabs[tabId];
  return tab ? tabRow(tab, facts, home) : null;
}
