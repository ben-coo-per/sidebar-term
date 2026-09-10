// Capture-phase keydown on window for the app's Hotkeys (docs/architecture.md "Interaction").
// Which combo drives which action comes from ./hotkeys.svelte.ts (user-rebindable on the
// Settings page). Only bound combos are intercepted (preventDefault + stopPropagation);
// everything else, including Cmd-C/V/A/Q, passes through untouched.

import {
  activateTab,
  activeTab,
  groupOf,
  jumpToGroup,
  layout,
  moveTab,
  newGroup,
  newTab,
  orderedTabIds,
  toggleSidebarVisible,
} from "./layout.svelte";
import { requestCloseTab } from "./sidebar/closeTabFlow";
import { actionFor, comboFromEvent, isMac, isModifierOnly, type ActionId } from "./hotkeys";
import { hotkeys } from "./hotkeys.svelte";
import { toggleSettings } from "./settings/visibility.svelte";

function moveActiveTab(direction: 1 | -1): void {
  const tab = activeTab();
  const group = tab && groupOf(tab);
  if (!tab || !group) return;
  const targetIndex = group.tabIds.indexOf(tab.id) + direction;
  if (targetIndex < 0 || targetIndex >= group.tabIds.length) return;
  moveTab(tab.id, group.id, targetIndex);
}

/** Activate the Tab `direction` steps from the active one in `order`, wrapping at the ends. */
function stepThrough(order: string[], direction: 1 | -1): void {
  if (order.length === 0) return;
  const idx = layout.activeTabId ? order.indexOf(layout.activeTabId) : -1;
  const nextIdx = idx === -1 ? 0 : (idx + direction + order.length) % order.length;
  activateTab(order[nextIdx]);
}

function stepTabInGroup(direction: 1 | -1): void {
  const tab = activeTab();
  const group = tab && groupOf(tab);
  if (group) stepThrough(group.tabIds, direction);
}

function run(action: ActionId): void {
  switch (action) {
    case "tab.new":
      void newTab();
      return;
    case "tab.close":
      // Asks for confirmation when the Tab's Foreground process isn't the shell.
      if (layout.activeTabId) void requestCloseTab(layout.activeTabId);
      return;
    case "tab.next":
      stepThrough(orderedTabIds(), 1);
      return;
    case "tab.prev":
      stepThrough(orderedTabIds(), -1);
      return;
    case "tab.nextInGroup":
      stepTabInGroup(1);
      return;
    case "tab.prevInGroup":
      stepTabInGroup(-1);
      return;
    case "tab.moveUp":
      moveActiveTab(-1);
      return;
    case "tab.moveDown":
      moveActiveTab(1);
      return;
    case "group.new":
      newGroup();
      return;
    case "sidebar.toggle":
      toggleSidebarVisible();
      return;
    case "settings.toggle":
      toggleSettings();
      return;
    default:
      // group.jump.N
      jumpToGroup(Number(action.slice("group.jump.".length)) - 1);
  }
}

function handleKeydown(e: KeyboardEvent): void {
  if (hotkeys.recording || isModifierOnly(e)) return;
  const action = actionFor(hotkeys.bindings, comboFromEvent(e, isMac()));
  if (!action) return;
  e.preventDefault();
  e.stopPropagation();
  run(action);
}

/** Install the global shortcut listener. Returns a cleanup function. */
export function initShortcuts(): () => void {
  window.addEventListener("keydown", handleKeydown, { capture: true });
  return () => window.removeEventListener("keydown", handleKeydown, { capture: true });
}
