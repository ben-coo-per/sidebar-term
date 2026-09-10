// One app-wide custom context menu, opened by a Tab row or a Group header. A single
// <ContextMenu/> mounted in Sidebar.svelte renders whatever is currently open.

import type { MenuItem } from "./ContextMenu.svelte";

export const contextMenuBox = $state<{ current: { x: number; y: number; items: MenuItem[] } | null }>({
  current: null,
});

export function openContextMenu(e: MouseEvent, items: MenuItem[]): void {
  e.preventDefault();
  e.stopPropagation();
  contextMenuBox.current = { x: e.clientX, y: e.clientY, items };
}

export function closeContextMenu(): void {
  contextMenuBox.current = null;
}
