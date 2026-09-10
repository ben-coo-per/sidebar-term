// The layout model: Groups, Tabs, order, active Tab, sidebar width, the Panel. Persistence via
// layout_load/layout_save (src/lib/ipc.ts), debounced ~500ms. See docs/architecture.md
// "Persistence" and "Naming", and CONTEXT.md for vocabulary.
//
// OWNER: sidebar agent. Session facts (SessionInfo, title, Agent status) live in
// src/lib/sessions.svelte.ts, which reads this module's Tabs to know what to update.

import { loadLayout as ipcLoadLayout, resetSessions, saveLayout as ipcSaveLayout } from "./ipc";
import { terminals } from "./terminal/manager";
import type { SessionId } from "./types";
import { isPanelViewId, PANEL_VIEWS, type PanelViewId } from "./panel/views";

export interface Group {
  id: string;
  name: string;
  collapsed: boolean;
  /** Tab ids, in display order. */
  tabIds: string[];
}

export interface Tab {
  id: string;
  sessionId: SessionId | null;
  groupId: string;
  /** A user rename that sticks; null means "use the automatic Title". */
  customTitle: string | null;
  /** Last known non-remote cwd, used to respawn this Tab's Session on relaunch. */
  lastCwd: string | null;
}

/** The Panel at the bottom of the sidebar. */
export interface PanelState {
  view: PanelViewId;
  /** Collapsed to its header. */
  collapsed: boolean;
  /** Expanded height in px, header included. */
  height: number;
}

interface LayoutState {
  groups: Group[];
  tabs: Record<string, Tab>;
  activeTabId: string | null;
  sidebarWidth: number;
  /** Cmd-B toggle. Not persisted: the sidebar is visible again on relaunch. */
  sidebarVisible: boolean;
  panel: PanelState;
  /** True once startup load + Session respawn has finished. Gates persistence. */
  ready: boolean;
}

const LAYOUT_VERSION = 1;
const DEFAULT_SIDEBAR_WIDTH = 240;
export const MIN_SIDEBAR_WIDTH = 180;
export const MAX_SIDEBAR_WIDTH = 420;
/** Below this sidebar width the Panel is hidden: its columns would not fit. */
export const PANEL_MIN_SIDEBAR_WIDTH = 220;
export const MIN_PANEL_HEIGHT = 96;
export const MAX_PANEL_HEIGHT = 640;
const DEFAULT_PANEL: PanelState = { view: PANEL_VIEWS[0].id, collapsed: false, height: 220 };
const SAVE_DEBOUNCE_MS = 500;
const DEFAULT_GROUP_NAME = "Tabs";

export const layout = $state<LayoutState>({
  groups: [],
  tabs: {},
  activeTabId: null,
  sidebarWidth: DEFAULT_SIDEBAR_WIDTH,
  sidebarVisible: true,
  panel: { ...DEFAULT_PANEL },
  ready: false,
});

/** SessionId -> Tab id, kept in step with `layout.tabs` for O(1) exit handling. */
const sessionToTab = new Map<SessionId, string>();

/** Group id -> the Tab last active in it, so jumping to a Group lands where you left off. */
const lastActiveInGroup = new Map<string, string>();

$effect.root(() => {
  $effect(() => {
    const tab = layout.activeTabId ? layout.tabs[layout.activeTabId] : null;
    if (tab) lastActiveInGroup.set(tab.groupId, tab.id);
  });
});

function newId(prefix: string): string {
  return `${prefix}_${crypto.randomUUID()}`;
}

function clampWidth(px: number): number {
  return Math.max(MIN_SIDEBAR_WIDTH, Math.min(MAX_SIDEBAR_WIDTH, Math.round(px)));
}

function clampPanelHeight(px: number): number {
  return Math.max(MIN_PANEL_HEIGHT, Math.min(MAX_PANEL_HEIGHT, Math.round(px)));
}

function allTabIdsInOrder(): string[] {
  return layout.groups.flatMap((g) => g.tabIds);
}

export function tabIdForSession(sessionId: SessionId): string | null {
  return sessionToTab.get(sessionId) ?? null;
}

// ---------------------------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------------------------

let saveTimer: ReturnType<typeof setTimeout> | null = null;

function scheduleSave(): void {
  if (!layout.ready) return;
  if (saveTimer !== null) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    saveTimer = null;
    void ipcSaveLayout(serialize());
  }, SAVE_DEBOUNCE_MS);
}

function serialize() {
  return {
    version: LAYOUT_VERSION,
    groups: layout.groups.map((g) => ({
      id: g.id,
      name: g.name,
      collapsed: g.collapsed,
      tabIds: [...g.tabIds],
    })),
    tabs: Object.values(layout.tabs).map((t) => ({
      id: t.id,
      groupId: t.groupId,
      customTitle: t.customTitle,
      lastCwd: t.lastCwd,
    })),
    activeTabId: layout.activeTabId,
    sidebarWidth: layout.sidebarWidth,
    panel: { ...layout.panel },
  };
}

interface PersistedTab {
  id: string;
  groupId: string;
  customTitle: string | null;
  lastCwd: string | null;
}

interface PersistedGroup {
  id: string;
  name: string;
  collapsed: boolean;
  tabIds: string[];
}

interface PersistedLayout {
  groups: PersistedGroup[];
  tabs: PersistedTab[];
  activeTabId: string | null;
  sidebarWidth: number;
  panel: PanelState;
}

/** Missing or bad fields fall back to the defaults (layouts saved before the Panel have none). */
function parsePanel(raw: unknown): PanelState {
  const p = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  return {
    view: isPanelViewId(p.view) ? p.view : DEFAULT_PANEL.view,
    collapsed: typeof p.collapsed === "boolean" ? p.collapsed : DEFAULT_PANEL.collapsed,
    height: typeof p.height === "number" ? clampPanelHeight(p.height) : DEFAULT_PANEL.height,
  };
}

/** Defensive parse of whatever `layout_load` returned: unknown JSON, possibly stale or hand-edited. */
function validateAndMigrate(raw: unknown): PersistedLayout | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  if (!Array.isArray(r.groups) || !Array.isArray(r.tabs)) return null;

  const tabs: PersistedTab[] = [];
  for (const t of r.tabs) {
    if (!t || typeof t !== "object") continue;
    const tt = t as Record<string, unknown>;
    if (typeof tt.id !== "string" || typeof tt.groupId !== "string") continue;
    tabs.push({
      id: tt.id,
      groupId: tt.groupId,
      customTitle: typeof tt.customTitle === "string" ? tt.customTitle : null,
      lastCwd: typeof tt.lastCwd === "string" ? tt.lastCwd : null,
    });
  }

  const groups: PersistedGroup[] = [];
  const groupIds = new Set<string>();
  for (const g of r.groups) {
    if (!g || typeof g !== "object") continue;
    const gg = g as Record<string, unknown>;
    if (typeof gg.id !== "string" || typeof gg.name !== "string") continue;
    if (groupIds.has(gg.id)) continue; // drop duplicate ids defensively
    const tabIds = Array.isArray(gg.tabIds) ? gg.tabIds.filter((x): x is string => typeof x === "string") : [];
    groups.push({ id: gg.id, name: gg.name, collapsed: Boolean(gg.collapsed), tabIds });
    groupIds.add(gg.id);
  }
  if (groups.length === 0) return null;

  // Keep only Tabs whose Group actually exists.
  const validTabs = tabs.filter((t) => groupIds.has(t.groupId));
  const validTabIds = new Set(validTabs.map((t) => t.id));

  // Every Group's tabIds may only reference valid, still-existing Tabs, each exactly once.
  const claimed = new Set<string>();
  for (const g of groups) {
    g.tabIds = g.tabIds.filter((id) => {
      if (!validTabIds.has(id) || claimed.has(id)) return false;
      claimed.add(id);
      return true;
    });
  }
  // Any valid Tab not referenced by its Group (corrupt/truncated save) gets appended back.
  for (const t of validTabs) {
    if (!claimed.has(t.id)) {
      const g = groups.find((g) => g.id === t.groupId);
      if (g) {
        g.tabIds.push(t.id);
        claimed.add(t.id);
      }
    }
  }

  const finalTabs = validTabs.filter((t) => claimed.has(t.id));
  const finalTabIds = new Set(finalTabs.map((t) => t.id));
  const activeTabId = typeof r.activeTabId === "string" && finalTabIds.has(r.activeTabId) ? r.activeTabId : null;
  const sidebarWidth = typeof r.sidebarWidth === "number" ? clampWidth(r.sidebarWidth) : DEFAULT_SIDEBAR_WIDTH;

  return { groups, tabs: finalTabs, activeTabId, sidebarWidth, panel: parsePanel(r.panel) };
}

/**
 * Startup: load the persisted layout (if any) and respawn a Session per Tab at its last cwd.
 * With nothing persisted, creates the default state: one Group "Tabs" with one Tab.
 */
export async function initLayout(): Promise<void> {
  // A webview reload keeps the Rust process: drop the previous page's Sessions first.
  await resetSessions().catch(() => {});
  const raw = await ipcLoadLayout().catch(() => null);
  const parsed = validateAndMigrate(raw);
  if (parsed) layout.panel = parsed.panel;

  if (parsed && parsed.tabs.length > 0) {
    layout.groups = parsed.groups.map((g) => ({ ...g, tabIds: [] }));
    layout.tabs = {};
    layout.sidebarWidth = parsed.sidebarWidth;

    for (const t of parsed.tabs) {
      let sessionId: SessionId | null = null;
      try {
        sessionId = await terminals.create({ cwd: t.lastCwd });
      } catch {
        sessionId = null; // respawn failed (e.g. bad cwd); keep the Tab, session-less
      }
      const tab: Tab = { id: t.id, sessionId, groupId: t.groupId, customTitle: t.customTitle, lastCwd: t.lastCwd };
      layout.tabs[t.id] = tab;
      const group = layout.groups.find((g) => g.id === t.groupId);
      group?.tabIds.push(t.id);
      if (sessionId !== null) sessionToTab.set(sessionId, t.id);
    }
    layout.activeTabId = parsed.activeTabId ?? allTabIdsInOrder()[0] ?? null;
  } else if (parsed && parsed.groups.length > 0) {
    // Groups with no Tabs at all: keep them (e.g. a deliberately empty custom Group).
    layout.groups = parsed.groups;
    layout.tabs = {};
    layout.sidebarWidth = parsed.sidebarWidth;
    layout.activeTabId = null;
  } else {
    const group: Group = { id: newId("group"), name: DEFAULT_GROUP_NAME, collapsed: false, tabIds: [] };
    layout.groups = [group];
    const sessionId = await terminals.create({});
    const tab: Tab = { id: newId("tab"), sessionId, groupId: group.id, customTitle: null, lastCwd: null };
    layout.tabs[tab.id] = tab;
    group.tabIds.push(tab.id);
    sessionToTab.set(sessionId, tab.id);
    layout.activeTabId = tab.id;
  }

  layout.ready = true;
}

terminals.on("exit", (sessionId) => {
  const tabId = sessionToTab.get(sessionId);
  if (tabId) removeTab(tabId);
});

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
// Actions
// ---------------------------------------------------------------------------------------------

/** New Tab in the active Tab's Group, right after it, spawned at the active Tab's cwd. */
export async function newTab(opts?: { cwd?: string | null; groupId?: string }): Promise<string> {
  const active = activeTab();
  const groupId = opts?.groupId ?? active?.groupId ?? layout.groups[0]?.id;
  if (!groupId) throw new Error("layout: no Group to add a Tab to");
  const cwd = opts?.cwd ?? active?.lastCwd ?? null;

  const sessionId = await terminals.create({ cwd });
  const id = newId("tab");
  const tab: Tab = { id, sessionId, groupId, customTitle: null, lastCwd: cwd };
  layout.tabs[id] = tab;
  sessionToTab.set(sessionId, id);

  const group = layout.groups.find((g) => g.id === groupId)!;
  const anchor = active && active.groupId === groupId ? group.tabIds.indexOf(active.id) : group.tabIds.length - 1;
  group.tabIds.splice(anchor + 1, 0, id);
  layout.activeTabId = id;
  scheduleSave();
  return id;
}

function computeNeighborTabId(tabId: string): string | null {
  const order = allTabIdsInOrder();
  const idx = order.indexOf(tabId);
  if (idx === -1) return order[0] ?? null;
  return order[idx + 1] ?? order[idx - 1] ?? null;
}

function removeTab(tabId: string): void {
  const tab = layout.tabs[tabId];
  if (!tab) return;
  const nextActive = layout.activeTabId === tabId ? computeNeighborTabId(tabId) : null;

  delete layout.tabs[tabId];
  if (tab.sessionId !== null) sessionToTab.delete(tab.sessionId);
  const group = layout.groups.find((g) => g.id === tab.groupId);
  if (group) group.tabIds = group.tabIds.filter((id) => id !== tabId);
  if (layout.activeTabId === tabId) layout.activeTabId = nextActive;
  scheduleSave();
}

/** Close a Tab: kills its Session, which removes the Tab once `terminals` confirms exit. */
export function closeTab(tabId: string): void {
  const tab = layout.tabs[tabId];
  if (!tab) return;
  if (tab.sessionId !== null) {
    void terminals.close(tab.sessionId);
  } else {
    removeTab(tabId);
  }
}

/** Rename a Tab; an empty title clears the rename and reverts to the automatic Title. */
export function renameTab(tabId: string, title: string): void {
  const tab = layout.tabs[tabId];
  if (!tab) return;
  const trimmed = title.trim();
  tab.customTitle = trimmed === "" ? null : trimmed;
  scheduleSave();
}

export function setTabLastCwd(tabId: string, cwd: string): void {
  const tab = layout.tabs[tabId];
  if (!tab || tab.lastCwd === cwd) return;
  tab.lastCwd = cwd;
  scheduleSave();
}

export function activateTab(tabId: string): void {
  if (!layout.tabs[tabId] || layout.activeTabId === tabId) return;
  layout.activeTabId = tabId;
  scheduleSave();
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

export function newGroup(name = "New Group"): string {
  const id = newId("group");
  layout.groups.push({ id, name, collapsed: false, tabIds: [] });
  scheduleSave();
  return id;
}

export function renameGroup(groupId: string, name: string): void {
  const group = layout.groups.find((g) => g.id === groupId);
  if (!group) return;
  const trimmed = name.trim();
  if (trimmed === "") return; // Groups have no automatic name to fall back to.
  group.name = trimmed;
  scheduleSave();
}

export function setGroupCollapsed(groupId: string, collapsed: boolean): void {
  const group = layout.groups.find((g) => g.id === groupId);
  if (!group || group.collapsed === collapsed) return;
  group.collapsed = collapsed;
  scheduleSave();
}

export function toggleGroupCollapsed(groupId: string): void {
  const group = layout.groups.find((g) => g.id === groupId);
  if (group) setGroupCollapsed(groupId, !group.collapsed);
}

/** Never deletes the last Group (a no-op guard; the caller should disable the affordance too). */
export function deleteGroup(groupId: string): void {
  if (layout.groups.length <= 1) return;
  const group = layout.groups.find((g) => g.id === groupId);
  if (!group) return;

  const tabIds = [...group.tabIds];
  layout.groups = layout.groups.filter((g) => g.id !== groupId);
  if (layout.activeTabId && tabIds.includes(layout.activeTabId)) {
    layout.activeTabId = allTabIdsInOrder()[0] ?? null;
  }
  for (const tabId of tabIds) {
    const tab = layout.tabs[tabId];
    if (!tab) continue;
    if (tab.sessionId !== null) void terminals.close(tab.sessionId);
    else removeTab(tabId);
  }
  scheduleSave();
}

/** Peel a Tab out into a brand new Group of its own (context menu: "New Group from Tab"). */
export function newGroupFromTab(tabId: string, name = "New Group"): string | null {
  const tab = layout.tabs[tabId];
  if (!tab) return null;
  const oldGroup = layout.groups.find((g) => g.id === tab.groupId);
  if (oldGroup) oldGroup.tabIds = oldGroup.tabIds.filter((id) => id !== tabId);
  const groupId = newId("group");
  layout.groups.push({ id: groupId, name, collapsed: false, tabIds: [tabId] });
  tab.groupId = groupId;
  scheduleSave();
  return groupId;
}

/** Move a Tab within or across Groups, inserting at `targetIndex` (default: end of Group). */
export function moveTab(tabId: string, targetGroupId: string, targetIndex?: number): void {
  const tab = layout.tabs[tabId];
  const targetGroup = layout.groups.find((g) => g.id === targetGroupId);
  if (!tab || !targetGroup) return;

  const sourceGroup = layout.groups.find((g) => g.id === tab.groupId);
  const sameGroup = sourceGroup === targetGroup;
  let insertAt = targetIndex ?? targetGroup.tabIds.length;

  if (sourceGroup) {
    const removedAt = sourceGroup.tabIds.indexOf(tabId);
    sourceGroup.tabIds = sourceGroup.tabIds.filter((id) => id !== tabId);
    if (sameGroup && removedAt !== -1 && removedAt < insertAt) insertAt -= 1;
  }
  insertAt = Math.max(0, Math.min(insertAt, targetGroup.tabIds.length));
  targetGroup.tabIds.splice(insertAt, 0, tabId);
  tab.groupId = targetGroupId;
  scheduleSave();
}

/** Reorder Groups, moving `groupId` to `targetIndex`. */
export function moveGroup(groupId: string, targetIndex: number): void {
  const idx = layout.groups.findIndex((g) => g.id === groupId);
  if (idx === -1) return;
  const [group] = layout.groups.splice(idx, 1);
  layout.groups.splice(Math.max(0, Math.min(targetIndex, layout.groups.length)), 0, group);
  scheduleSave();
}

export function setSidebarWidth(px: number): void {
  layout.sidebarWidth = clampWidth(px);
  scheduleSave();
}

export function toggleSidebarVisible(): void {
  layout.sidebarVisible = !layout.sidebarVisible;
}

/** Show `view` in the Panel, expanding it if collapsed. */
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
