// Shared HTML5 drag-and-drop state for Tabs (within/across Groups, onto collapsed Group
// headers) and Groups. dataTransfer.getData() only reliably reads during `drop` in most
// browsers, so hover/insertion state is tracked here instead and dataTransfer is a fallback.

import { layout, moveGroup, moveTab } from "../layout.svelte";

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
  if (draggedId) {
    const group = layout.groups.find((g) => g.id === groupId);
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
  e.preventDefault();
  if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
  dnd.overGroupId = groupId;
  dnd.overTabId = null;
}

export function dropOnGroupHeader(e: DragEvent, groupId: string): void {
  e.preventDefault();
  const tabId = e.dataTransfer?.getData(TAB_MIME) || dnd.draggingTabId;
  const draggedGroupId = e.dataTransfer?.getData(GROUP_MIME) || dnd.draggingGroupId;
  if (tabId) {
    moveTab(tabId, groupId);
  } else if (draggedGroupId && draggedGroupId !== groupId) {
    const targetIndex = layout.groups.findIndex((g) => g.id === groupId);
    if (targetIndex !== -1) moveGroup(draggedGroupId, targetIndex);
  }
  endDrag();
}
