// Shared HTML5 drag-and-drop state for Tabs (within/across Groups, onto collapsed Group
// headers) and Groups. dataTransfer.getData() only reliably reads during `drop` in most
// browsers, so hover/insertion state is tracked here instead and dataTransfer is a fallback.
//
// A drag never crosses Hosts: a Session cannot change machines, so a Tab dragged over another
// Host's rows shows no drop target and drops nowhere (Handoff, #30, is the real thing), and a
// Group reorders only among its own Host's Groups.

import { groupsOf, layout, moveGroup, moveTab } from "../layout.svelte";
import type { HostId } from "../host/ids";

const TAB_MIME = "application/x-sidebar-tab";
const GROUP_MIME = "application/x-sidebar-group";

export const dnd = $state<{
  draggingTabId: string | null;
  draggingGroupId: string | null;
  overTabId: string | null;
  overPosition: "before" | "after" | null;
  overGroupId: string | null;
}>({
  draggingTabId: null,
  draggingGroupId: null,
  overTabId: null,
  overPosition: null,
  overGroupId: null,
});

function hostOfTab(tabId: string): HostId | null {
  return layout.tabs[tabId]?.host ?? null;
}

function hostOfGroup(groupId: string): HostId | null {
  for (const g of layout.groups) if (g.id === groupId) return g.host;
  for (const s of layout.sections) for (const g of s.groups) if (g.id === groupId) return s.host;
  return null;
}

/** The Host of what is being dragged, if anything is. */
function draggingHost(): HostId | null {
  if (dnd.draggingTabId) return hostOfTab(dnd.draggingTabId);
  if (dnd.draggingGroupId) return hostOfGroup(dnd.draggingGroupId);
  return null;
}

/** Refuse the drop here: no indicator, and the browser shows "not allowed". */
function refuse(e: DragEvent): void {
  if (e.dataTransfer) e.dataTransfer.dropEffect = "none";
  dnd.overTabId = null;
  dnd.overPosition = null;
  dnd.overGroupId = null;
}

export function startTabDrag(e: DragEvent, tabId: string): void {
  dnd.draggingTabId = tabId;
  e.dataTransfer?.setData(TAB_MIME, tabId);
  if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
}

export function startGroupDrag(e: DragEvent, groupId: string): void {
  dnd.draggingGroupId = groupId;
  e.dataTransfer?.setData(GROUP_MIME, groupId);
  if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
}

export function endDrag(): void {
  dnd.draggingTabId = null;
  dnd.draggingGroupId = null;
  dnd.overTabId = null;
  dnd.overPosition = null;
  dnd.overGroupId = null;
}

export function overTabRow(e: DragEvent, tabId: string): void {
  if (!dnd.draggingTabId) return;
  if (draggingHost() !== hostOfTab(tabId)) {
    refuse(e);
    return;
  }
  e.preventDefault();
  if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
  dnd.overTabId = tabId;
  dnd.overPosition = e.clientY < rect.top + rect.height / 2 ? "before" : "after";
  dnd.overGroupId = null;
}

export function dropOnTabRow(e: DragEvent, groupId: string, tabId: string): void {
  e.preventDefault();
  const draggedId = e.dataTransfer?.getData(TAB_MIME) || dnd.draggingTabId;
  if (draggedId && hostOfTab(draggedId) === hostOfTab(tabId)) {
    const group = groupsOf(hostOfTab(tabId) ?? "").find((g) => g.id === groupId);
    if (group) {
      const idx = group.tabIds.indexOf(tabId);
      const insertAt = dnd.overPosition === "after" ? idx + 1 : idx;
      moveTab(draggedId, groupId, insertAt);
    }
  }
  endDrag();
}

export function overGroupHeader(e: DragEvent, groupId: string): void {
  if (!dnd.draggingTabId && !dnd.draggingGroupId) return;
  if (draggingHost() !== hostOfGroup(groupId)) {
    refuse(e);
    return;
  }
  e.preventDefault();
  if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
  dnd.overGroupId = groupId;
  dnd.overTabId = null;
}

export function dropOnGroupHeader(e: DragEvent, groupId: string): void {
  e.preventDefault();
  const tabId = e.dataTransfer?.getData(TAB_MIME) || dnd.draggingTabId;
  const draggedGroupId = e.dataTransfer?.getData(GROUP_MIME) || dnd.draggingGroupId;
  const host = hostOfGroup(groupId);
  if (tabId) {
    if (hostOfTab(tabId) === host) moveTab(tabId, groupId);
  } else if (draggedGroupId && draggedGroupId !== groupId && hostOfGroup(draggedGroupId) === host && host !== null) {
    const targetIndex = groupsOf(host).findIndex((g) => g.id === groupId);
    if (targetIndex !== -1) moveGroup(draggedGroupId, targetIndex);
  }
  endDrag();
}
