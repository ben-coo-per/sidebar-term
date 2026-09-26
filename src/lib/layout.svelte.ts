// The layout as the webview sees it: a mirror of the Host's snapshot (Groups, Tabs, order, the
// active Tab: src-tauri/core/src/layout/, read with `layout_get`, followed on the `layout` event,
// changed through the `tab_*` / `group_*` commands), plus what is this client's own: the sidebar
// width and visibility, the Panel, the user's unread marks. Those persist in the `sidebar`
// section of settings.json (debounced ~500 ms), never in the Host's layout.json. See
// docs/architecture.md "Split of responsibility" and "Persistence", ADR 0002, and CONTEXT.md.
//
// OWNER: sidebar agent. Session facts (SessionInfo, title, Agent status) live in
// src/lib/sessions.svelte.ts, which reads this module's Tabs to know what to update.

import {
  groupDelete,
  groupMove,
  groupNew,
  groupRename,
  groupSetCollapsed,
  layoutGet,
  onLayout,
  resetSessions,
  tabActivate,
  tabClose,
  tabMove,
  tabNew,
  tabRename,
} from "./ipc";
import { loadSection, saveSection } from "./settings/store";
import { terminals } from "./terminal/manager";
import type { Group, LayoutSnapshot, SessionId, Tab as HostTab } from "./types";
import type { PanelViewId } from "./panel/views";
import {
  clampPanelHeight,
  clampWidth,
  DEFAULT_PANEL,
  DEFAULT_SIDEBAR_WIDTH,
  parseSidebarSection,
  type PanelState,
  type SidebarSettings,
} from "./sidebar/settings";

export type { Group, PanelState };
export { MAX_PANEL_HEIGHT, MAX_SIDEBAR_WIDTH, MIN_PANEL_HEIGHT, MIN_SIDEBAR_WIDTH } from "./sidebar/settings";

/** A Tab as this client shows it: the Host's, plus the user's unread mark. */
export interface Tab extends HostTab {
  /** Marked unread by the user; cleared when the user next goes to the Tab. */
  unread: boolean;
}

interface LayoutState {
  groups: Group[];
  tabs: Record<string, Tab>;
  activeTabId: string | null;
  sidebarWidth: number;
  /** Cmd-B toggle. Not persisted: the sidebar is visible again on relaunch. */
  sidebarVisible: boolean;
  panel: PanelState;
  /** True once the first snapshot and the sidebar settings are in. Gates persistence. */
  ready: boolean;
}

/** Below this sidebar width the Panel is hidden: its columns would not fit. */
export const PANEL_MIN_SIDEBAR_WIDTH = 220;
const SAVE_DEBOUNCE_MS = 500;
/** The settings.json section this client's presentation state lives in. */
const SIDEBAR_SECTION = "sidebar";

export const layout = $state<LayoutState>({
  groups: [],
  tabs: {},
  activeTabId: null,
  sidebarWidth: DEFAULT_SIDEBAR_WIDTH,
  sidebarVisible: true,
  panel: { ...DEFAULT_PANEL },
  ready: false,
});

/** The Host's revision of the snapshot shown; an older one arriving late is ignored. */
let revision = -1;

/** Tab ids the user marked unread. Persisted with the sidebar settings. */
const unreadMarks = new Set<string>();

/** SessionId -> Tab id, kept in step with `layout.tabs` for O(1) lookups. */
const sessionToTab = new Map<SessionId, string>();

/** Group id -> the Tab last active in it, so jumping to a Group lands where you left off. */
const lastActiveInGroup = new Map<string, string>();

$effect.root(() => {
  $effect(() => {
    const tab = layout.activeTabId ? layout.tabs[layout.activeTabId] : null;
    if (tab) lastActiveInGroup.set(tab.groupId, tab.id);
  });
});

function allTabIdsInOrder(): string[] {
  return layout.groups.flatMap((g) => g.tabIds);
}

export function tabIdForSession(sessionId: SessionId): string | null {
  return sessionToTab.get(sessionId) ?? null;
}

// ---------------------------------------------------------------------------------------------
// The mirror
// ---------------------------------------------------------------------------------------------

/**
 * Take the Host's snapshot: replace Groups, Tabs and the active Tab, keep this client's marks,
 * and give every Tab's Session a Terminal. `initial` accepts the snapshot at the revision shown
 * (the first read); events only move forward.
 */
function applySnapshot(snap: LayoutSnapshot, initial = false): void {
  if (initial ? snap.revision < revision : snap.revision <= revision) return;
  revision = snap.revision;

  const tabs: Record<string, Tab> = {};
  sessionToTab.clear();
  for (const id of Object.keys(snap.tabs)) {
    const t = snap.tabs[id];
    tabs[id] = { ...t, unread: unreadMarks.has(id) };
    if (t.sessionId !== null) {
      sessionToTab.set(t.sessionId, id);
      terminals.attach(t.sessionId);
    }
  }
  // Marks for Tabs that are gone are dropped (and saved with the next change).
  let pruned = false;
  for (const id of unreadMarks) {
    if (!tabs[id]) {
      unreadMarks.delete(id);
      pruned = true;
    }
  }
  layout.groups = snap.groups.map((g) => ({ ...g, tabIds: [...g.tabIds] }));
  layout.tabs = tabs;
  layout.activeTabId = snap.activeTabId;
  if (pruned) scheduleSave();
}

// ---------------------------------------------------------------------------------------------
// This client's own state: the `sidebar` settings section
// ---------------------------------------------------------------------------------------------

let saveTimer: ReturnType<typeof setTimeout> | null = null;

function scheduleSave(): void {
  if (!layout.ready) return;
  if (saveTimer !== null) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    saveTimer = null;
    saveSection(SIDEBAR_SECTION, serializeSidebar());
  }, SAVE_DEBOUNCE_MS);
}

function serializeSidebar(): SidebarSettings {
  return { width: layout.sidebarWidth, panel: { ...layout.panel }, unread: [...unreadMarks] };
}

/**
 * Startup: replace what a previous page left attached, read the Host's layout and follow it,
 * and read this client's sidebar settings. The Host has already spawned a Session per Tab.
 */
export async function initLayout(): Promise<void> {
  await resetSessions().catch(() => {});
  const saved = parseSidebarSection(await loadSection(SIDEBAR_SECTION).catch(() => undefined));
  layout.sidebarWidth = saved.width;
  layout.panel = saved.panel;
  for (const id of saved.unread) unreadMarks.add(id);

  // Listen before the first read: nothing between the two is missed, and an older snapshot
  // arriving after a newer one is ignored by its revision.
  await onLayout((snap) => applySnapshot(snap));
  const first = await layoutGet();
  applySnapshot(first, true);
  layout.ready = true;
}

// ---------------------------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------------------------

export function activeTab(): Tab | null {
  return layout.activeTabId ? (layout.tabs[layout.activeTabId] ?? null) : null;
}

export function groupOf(tab: Tab): Group | null {
  return layout.groups.find((g) => g.id === tab.groupId) ?? null;
}

/** Tab ids in top-to-bottom sidebar order, including collapsed Groups. */
export function orderedTabIds(): string[] {
  return allTabIdsInOrder();
}

// ---------------------------------------------------------------------------------------------
// Actions on the Host's layout: each is one command; the snapshot it emits updates the mirror
// ---------------------------------------------------------------------------------------------

function report(what: string): (e: unknown) => void {
  return (e) => console.error(`layout: ${what} failed`, e);
}

/**
 * New Tab in the active Tab's Group, right after it, spawned at the active Tab's cwd (the Host's
 * rule; `groupId` puts it at the end of that Group instead). Resolves to the new Tab's id.
 */
export async function newTab(opts?: { cwd?: string | null; groupId?: string }): Promise<string> {
  const tab = await tabNew({ cwd: opts?.cwd, groupId: opts?.groupId, ...terminals.grid() });
  return tab.id;
}

/** Close a Tab: the Host takes it out at once and kills its Session. */
export function closeTab(tabId: string): void {
  if (!layout.tabs[tabId]) return;
  void tabClose(tabId).catch(report("closing a Tab"));
}

/** Rename a Tab; an empty title clears the rename and reverts to the automatic Title. */
export function renameTab(tabId: string, title: string): void {
  if (!layout.tabs[tabId]) return;
  void tabRename(tabId, title).catch(report("renaming a Tab"));
}

/** Mark or unmark a Tab as unread. The mark stays until the user next goes to the Tab. */
export function setTabUnread(tabId: string, unread: boolean): void {
  const tab = layout.tabs[tabId];
  if (!tab || tab.unread === unread) return;
  tab.unread = unread;
  if (unread) unreadMarks.add(tabId);
  else unreadMarks.delete(tabId);
  scheduleSave();
}

/**
 * Go to a Tab, clearing its unread mark. Shown at once, ahead of the Host's snapshot, so the
 * Terminal switches without a round trip. Not used on relaunch, so a mark put on the Tab in view
 * survives a restart.
 */
export function activateTab(tabId: string): void {
  if (!layout.tabs[tabId] || layout.activeTabId === tabId) return;
  layout.activeTabId = tabId;
  setTabUnread(tabId, false);
  void tabActivate(tabId).catch(report("going to a Tab"));
}

/**
 * Go to the Group at `index` (0-based, sidebar order): activate the Tab last active in it, else
 * its first Tab, expanding the Group so the active Tab shows. A no-op for an empty Group.
 */
export function jumpToGroup(index: number): void {
  const group = layout.groups[index];
  if (!group || group.tabIds.length === 0) return;
  const remembered = lastActiveInGroup.get(group.id);
  setGroupCollapsed(group.id, false);
  activateTab(remembered && group.tabIds.includes(remembered) ? remembered : group.tabIds[0]);
}

/** A new Group at the end; resolves to its id. */
export async function newGroup(name?: string): Promise<string> {
  const group = await groupNew(name);
  return group.id;
}

export function renameGroup(groupId: string, name: string): void {
  if (name.trim() === "") return; // Groups have no automatic name to fall back to.
  void groupRename(groupId, name).catch(report("renaming a Group"));
}

export function setGroupCollapsed(groupId: string, collapsed: boolean): void {
  const group = layout.groups.find((g) => g.id === groupId);
  if (!group || group.collapsed === collapsed) return;
  group.collapsed = collapsed; // shown at once; the snapshot agrees
  void groupSetCollapsed(groupId, collapsed).catch(report("collapsing a Group"));
}

export function toggleGroupCollapsed(groupId: string): void {
  const group = layout.groups.find((g) => g.id === groupId);
  if (group) setGroupCollapsed(groupId, !group.collapsed);
}

/** Delete a Group and close its Tabs. Never the last Group (the caller disables the affordance too). */
export function deleteGroup(groupId: string): void {
  if (layout.groups.length <= 1) return;
  void groupDelete(groupId).catch(report("deleting a Group"));
}

/** Peel a Tab out into a brand new Group of its own (context menu: "New Group from Tab"). */
export async function newGroupFromTab(tabId: string, name?: string): Promise<string | null> {
  if (!layout.tabs[tabId]) return null;
  const group = await groupNew(name, tabId);
  return group.id;
}

/** Move a Tab within or across Groups, inserting at `targetIndex` (default: end of Group). */
export function moveTab(tabId: string, targetGroupId: string, targetIndex?: number): void {
  if (!layout.tabs[tabId] || !layout.groups.some((g) => g.id === targetGroupId)) return;
  void tabMove(tabId, targetGroupId, targetIndex).catch(report("moving a Tab"));
}

/** Reorder Groups, moving `groupId` to `targetIndex`. */
export function moveGroup(groupId: string, targetIndex: number): void {
  if (!layout.groups.some((g) => g.id === groupId)) return;
  void groupMove(groupId, Math.max(0, targetIndex)).catch(report("moving a Group"));
}

// ---------------------------------------------------------------------------------------------
// Presentation: this client's alone
// ---------------------------------------------------------------------------------------------

export function setSidebarWidth(px: number): void {
  layout.sidebarWidth = clampWidth(px);
  scheduleSave();
}

export function toggleSidebarVisible(): void {
  layout.sidebarVisible = !layout.sidebarVisible;
}

/** Open `view` in the Panel, closing whichever was open. */
export function setPanelView(view: PanelViewId): void {
  layout.panel.view = view;
  layout.panel.collapsed = false;
  scheduleSave();
}

export function togglePanelCollapsed(): void {
  layout.panel.collapsed = !layout.panel.collapsed;
  scheduleSave();
}

export function setPanelHeight(px: number): void {
  layout.panel.height = clampPanelHeight(px);
  scheduleSave();
}
