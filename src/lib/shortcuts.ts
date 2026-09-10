// Capture-phase keydown on window for the app's global shortcuts (docs/architecture.md
// "Interaction"). Only the combinations below are intercepted (preventDefault + stopPropagation);
// everything else, including Cmd-C/V/A/Q, passes through untouched.

import {
  activateTab,
  layout,
  moveTab,
  newGroup,
  newTab,
  toggleSidebarVisible,
  visibleTabIds,
  orderedTabIds,
  groupOf,
} from "./layout.svelte";
import { requestCloseTab } from "./sidebar/closeTabFlow";

function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac/.test(navigator.platform ?? navigator.userAgent);
}

/** Cmd on macOS, Ctrl elsewhere (v1 targets macOS only, but this keeps dev-in-browser sane). */
function primary(e: KeyboardEvent): boolean {
  return isMac() ? e.metaKey : e.ctrlKey;
}

function moveActiveTab(direction: 1 | -1): void {
  const id = layout.activeTabId;
  if (!id) return;
  const tab = layout.tabs[id];
  const group = tab && groupOf(tab);
  if (!tab || !group) return;
  const idx = group.tabIds.indexOf(id);
  const targetIndex = idx + direction;
  if (targetIndex < 0 || targetIndex >= group.tabIds.length) return;
  moveTab(id, group.id, targetIndex);
}

function stepTab(direction: 1 | -1): void {
  const order = orderedTabIds();
  if (order.length === 0) return;
  const id = layout.activeTabId;
  const idx = id ? order.indexOf(id) : -1;
  const nextIdx = idx === -1 ? 0 : (idx + direction + order.length) % order.length;
  activateTab(order[nextIdx]);
}

function jumpToVisibleTab(n: number): void {
  const id = visibleTabIds()[n - 1];
  if (id) activateTab(id);
}

function handleKeydown(e: KeyboardEvent): void {
  if (!primary(e)) return;

  // Cmd-T: new Tab
  if (!e.shiftKey && !e.altKey && e.key.toLowerCase() === "t") {
    e.preventDefault();
    e.stopPropagation();
    void newTab();
    return;
  }

  // Cmd-Shift-N: new Group
  if (e.shiftKey && !e.altKey && e.key.toLowerCase() === "n") {
    e.preventDefault();
    e.stopPropagation();
    newGroup();
    return;
  }

  // Cmd-W: close Tab (with confirmation when its Foreground process isn't the shell)
  if (!e.shiftKey && !e.altKey && e.key.toLowerCase() === "w") {
    e.preventDefault();
    e.stopPropagation();
    if (layout.activeTabId) void requestCloseTab(layout.activeTabId);
    return;
  }

  // Cmd-1..9: jump to the Nth visible Tab
  if (!e.shiftKey && !e.altKey && e.key >= "1" && e.key <= "9") {
    e.preventDefault();
    e.stopPropagation();
    jumpToVisibleTab(Number(e.key));
    return;
  }

  // Cmd-Shift-[ / Cmd-Shift-]: previous / next Tab
  if (e.shiftKey && !e.altKey && (e.key === "[" || e.key === "{")) {
    e.preventDefault();
    e.stopPropagation();
    stepTab(-1);
    return;
  }
  if (e.shiftKey && !e.altKey && (e.key === "]" || e.key === "}")) {
    e.preventDefault();
    e.stopPropagation();
    stepTab(1);
    return;
  }

  // Cmd-Opt-Up / Cmd-Opt-Down: move the active Tab within its Group
  if (e.altKey && !e.shiftKey && e.key === "ArrowUp") {
    e.preventDefault();
    e.stopPropagation();
    moveActiveTab(-1);
    return;
  }
  if (e.altKey && !e.shiftKey && e.key === "ArrowDown") {
    e.preventDefault();
    e.stopPropagation();
    moveActiveTab(1);
    return;
  }

  // Cmd-B: toggle sidebar
  if (!e.shiftKey && !e.altKey && e.key.toLowerCase() === "b") {
    e.preventDefault();
    e.stopPropagation();
    toggleSidebarVisible();
    return;
  }
}

/** Install the global shortcut listener. Returns a cleanup function. */
export function initShortcuts(): () => void {
  window.addEventListener("keydown", handleKeydown, { capture: true });
  return () => window.removeEventListener("keydown", handleKeydown, { capture: true });
}
