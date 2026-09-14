// The sidebar snapshot a phone shows, built from the Mac's layout and Session facts. Pure: the
// caller passes what it reads (so the reactive publisher in ./remote.svelte.ts tracks it) and
// this decides the shape (src/lib/mobile/protocol.ts). Rust relays it opaque (ADR 0001).

import type { SidebarSnapshot, SidebarTab } from "../mobile/protocol";
import type { SessionState } from "../sessions.svelte";
import type { SessionId } from "../types";

export interface SidebarSource {
  groups: { id: string; name: string; tabIds: string[] }[];
  tabs: Record<string, { id: string; sessionId: SessionId | null }>;
  activeTabId: string | null;
  titleOf(tabId: string): string;
  stateOf(sessionId: SessionId | null): SessionState | null;
}

export function buildSidebarSnapshot(src: SidebarSource): SidebarSnapshot {
  const groups = src.groups.map((g) => ({
    id: g.id,
    name: g.name,
    tabs: g.tabIds.flatMap((tabId): SidebarTab[] => {
      const tab = src.tabs[tabId];
      if (!tab) return [];
      const s = src.stateOf(tab.sessionId);
      return [
        {
          id: tab.id,
          sessionId: tab.sessionId,
          title: src.titleOf(tab.id),
          agent: s?.info?.agent ?? null,
          status: s?.status ?? null,
          finished: s?.finished ?? false,
          git: s?.info?.git ?? null,
          remote: s?.info?.remote ?? false,
        },
      ];
    }),
  }));
  return { groups, activeTabId: src.activeTabId };
}
