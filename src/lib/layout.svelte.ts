// The layout as the webview sees it: a mirror of every Host's snapshot (Groups, Tabs, order,
// each Host's active Tab: src-tauri/core/src/layout/), changed only through the Hosts' `tab_*`
// / `group_*` commands, plus what is this client's own: the Tab in view, the sidebar width and
// visibility, the Panel, the user's unread marks, the window's mode (Tabs or Manager) and
// Manager's zoom. Those persist in the `sidebar` section of
// settings.json (debounced ~500 ms), never in a Host's layout.json.
//
// The local Host (this Mac's own, `LOCAL_HOST`) is read with `layout_get` and followed on the
// `layout` event; its Groups are `layout.groups`. Each paired Host is a section
// (`layout.sections`, in Settings order) fed by src/lib/host/hosts.svelte.ts from the Host's
// `hello` and `layout` messages. Every Host's Tabs share `layout.tabs` (ids are random per
// Host, so they never collide); a Tab knows its Host. See docs/architecture.md "Split of
// responsibility", "Persistence" and "Hosts", ADR 0002, and CONTEXT.md.
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
  type TabNewOptions,
} from "./ipc";
import { loadSection, saveSection } from "./settings/store";
import { localTransport, terminals, type SessionTransport } from "./terminal/manager";
import type { Group as HostGroup, LayoutSnapshot, SessionId, Tab as HostTab } from "./types";
import type { PanelViewId } from "./panel/views";
import { isLocal, LOCAL_HOST, sessionKey, type HostId, type SessionKey } from "./host/ids";
import { hostCommands, hostTransport } from "./host/hosts.svelte";
import {
  clampPanelHeight,
  clampWidth,
  DEFAULT_MANAGER_ZOOM,
  DEFAULT_PANEL,
  DEFAULT_SIDEBAR_WIDTH,
  parseSidebarSection,
  type ManagerZoom,
  type PanelState,
  type SidebarSettings,
  type WindowMode,
} from "./sidebar/settings";

export type { ManagerZoom, PanelState, WindowMode };
export { MAX_PANEL_HEIGHT, MAX_SIDEBAR_WIDTH, MIN_PANEL_HEIGHT, MIN_SIDEBAR_WIDTH } from "./sidebar/settings";

/** A Tab as this client shows it: its Host's, plus which Host, and the user's unread mark. */
export interface Tab extends HostTab {
  host: HostId;
  /** Marked unread by the user; cleared when the user next goes to the Tab. */
  unread: boolean;
}

/** A Group as this client shows it: its Host's, plus which Host. */
export interface Group extends HostGroup {
  host: HostId;
}

/** A paired Host's part of the sidebar: its Groups, and which Tab the Host itself calls active. */
export interface HostSection {
  host: HostId;
  groups: Group[];
  /** The Host's own active Tab; the Tab in view (`layout.activeTabId`) is this client's. */
  activeTabId: string | null;
}

interface LayoutState {
  /** The local Host's Groups, in sidebar order. */
  groups: Group[];
  /** Each paired Host's section, in Settings order, whether it is connected or not. */
  sections: HostSection[];
  /** Every Host's Tabs, by id. */
  tabs: Record<string, Tab>;
  /** The Tab in view, on any Host: whose Terminal shows. */
  activeTabId: string | null;
  sidebarWidth: number;
  /** Cmd-B toggle. Not persisted: the sidebar is visible again on relaunch. */
  sidebarVisible: boolean;
  panel: PanelState;
  /** Tabs (the sidebar and one Terminal) or Manager (every agent's lane). Per window, persisted. */
  mode: WindowMode;
  managerZoom: ManagerZoom;
  /** True once the local Host's first snapshot and the sidebar settings are in. Gates persistence. */
  ready: boolean;
}

/** Below this sidebar width the Panel is hidden: its columns would not fit. */
export const PANEL_MIN_SIDEBAR_WIDTH = 220;
const SAVE_DEBOUNCE_MS = 500;
/** The settings.json section this client's presentation state lives in. */
const SIDEBAR_SECTION = "sidebar";

export const layout = $state<LayoutState>({
  groups: [],
  sections: [],
  tabs: {},
  activeTabId: null,
  sidebarWidth: DEFAULT_SIDEBAR_WIDTH,
  sidebarVisible: true,
  panel: { ...DEFAULT_PANEL },
  mode: "tabs",
  managerZoom: DEFAULT_MANAGER_ZOOM,
  ready: false,
});

/** Each Host's revision of the snapshot shown; an older one arriving late is ignored. */
const revisions = new Map<HostId, number>();

/** Tab ids the user marked unread. Persisted with the sidebar settings. */
const unreadMarks = new Set<string>();
/** The Host each marked Tab was last seen on: a mark is only pruned once its Host's snapshot lacks the Tab. */
const markHosts = new Map<string, HostId>();

/** SessionKey -> Tab id, kept in step with `layout.tabs` for O(1) lookups. */
const sessionToTab = new Map<SessionKey, string>();

/** Group id -> the Tab last active in it, so jumping to a Group lands where you left off. */
const lastActiveInGroup = new Map<string, string>();

/** A Tab this client made (`tab_new`) and wants in view once its Host's snapshot names it. */
let pendingView: { host: HostId; tabId: string } | null = null;

$effect.root(() => {
  $effect(() => {
    const tab = layout.activeTabId ? layout.tabs[layout.activeTabId] : null;
    if (tab) lastActiveInGroup.set(tab.groupId, tab.id);
  });
});

export function tabIdForSession(key: SessionKey): string | null {
  return sessionToTab.get(key) ?? null;
}

/** The Groups of one Host, in its sidebar order. */
export function groupsOf(host: HostId): Group[] {
  if (isLocal(host)) return layout.groups;
  return layout.sections.find((s) => s.host === host)?.groups ?? [];
}

function sectionOf(host: HostId): HostSection | null {
  return layout.sections.find((s) => s.host === host) ?? null;
}

function findGroup(groupId: string): Group | null {
  for (const g of layout.groups) if (g.id === groupId) return g;
  for (const s of layout.sections) for (const g of s.groups) if (g.id === groupId) return g;
  return null;
}

function transportFor(host: HostId): SessionTransport {
  return isLocal(host) ? localTransport : hostTransport(host);
}

// ---------------------------------------------------------------------------------------------
// The mirror
// ---------------------------------------------------------------------------------------------

/**
 * Take a Host's snapshot: replace its Groups, Tabs and active Tab, keep this client's marks, and
 * give every Tab's Session a Terminal. `initial` accepts the snapshot at the revision shown (the
 * first read); `fresh` accepts any revision (a paired Host's `hello`: it may have restarted);
 * otherwise events only move forward.
 *
 * The Tab in view follows the Host's active Tab only while the view is on that Host (the Host's
 * word wins there: after a `tab_new` or a close, it picks what shows next), or while nothing is
 * in view yet, or when the snapshot brings the Tab this client just made. Another Host changing
 * its own active Tab never pulls the view away.
 */
export function applyHostSnapshot(host: HostId, snap: LayoutSnapshot, opts: { initial?: boolean; fresh?: boolean } = {}): void {
  const shown = revisions.get(host);
  if (shown !== undefined && !opts.fresh && (opts.initial ? snap.revision < shown : snap.revision <= shown)) return;
  revisions.set(host, snap.revision);

  const viewTab = layout.activeTabId ? (layout.tabs[layout.activeTabId] ?? null) : null;
  const viewHost = viewTab?.host ?? null;

  // This Host's Tabs, replaced whole; other Hosts' stay.
  const tabs: Record<string, Tab> = {};
  for (const [id, t] of Object.entries(layout.tabs)) if (t.host !== host) tabs[id] = t;
  for (const key of sessionToTab.keys()) if (key.startsWith(`${host}/`)) sessionToTab.delete(key);
  for (const id of Object.keys(snap.tabs)) {
    const t = snap.tabs[id];
    tabs[id] = { ...t, host, unread: unreadMarks.has(id) };
    if (unreadMarks.has(id)) markHosts.set(id, host);
    if (t.sessionId !== null) {
      const key = sessionKey(host, t.sessionId);
      sessionToTab.set(key, id);
      terminals.attach(key, transportFor(host));
    }
  }
  // Marks for Tabs that are gone from their Host are dropped (and saved with the next change).
  let pruned = false;
  for (const id of unreadMarks) {
    if (markHosts.get(id) === host && !tabs[id]) {
      unreadMarks.delete(id);
      markHosts.delete(id);
      pruned = true;
    }
  }
  const groups: Group[] = snap.groups.map((g) => ({ ...g, tabIds: [...g.tabIds], host }));
  if (isLocal(host)) {
    layout.groups = groups;
  } else {
    const section = sectionOf(host);
    if (section) {
      section.groups = groups;
      section.activeTabId = snap.activeTabId;
    } else {
      layout.sections.push({ host, groups, activeTabId: snap.activeTabId });
    }
  }
  layout.tabs = tabs;

  if (pendingView?.host === host && tabs[pendingView.tabId]) {
    layout.activeTabId = pendingView.tabId;
    pendingView = null;
  } else if (viewHost === host || (viewTab === null && isLocal(host))) {
    layout.activeTabId = snap.activeTabId && tabs[snap.activeTabId] ? snap.activeTabId : null;
  }
  if (pruned) scheduleSave();
}

/** A paired Host's section, shown (empty, until its first snapshot) as soon as it is paired. */
export function ensureHostSection(host: HostId): void {
  if (isLocal(host) || sectionOf(host)) return;
  layout.sections.push({ host, groups: [], activeTabId: null });
}

/** A paired Host was removed: its section and Tabs go; a view on it falls back to the local Host's active Tab. */
export function dropHost(host: HostId): void {
  if (isLocal(host)) return;
  revisions.delete(host);
  const viewTab = layout.activeTabId ? (layout.tabs[layout.activeTabId] ?? null) : null;
  const tabs: Record<string, Tab> = {};
  for (const [id, t] of Object.entries(layout.tabs)) if (t.host !== host) tabs[id] = t;
  for (const key of sessionToTab.keys()) if (key.startsWith(`${host}/`)) sessionToTab.delete(key);
  layout.tabs = tabs;
  layout.sections = layout.sections.filter((s) => s.host !== host);
  if (viewTab?.host === host) {
    const local = layout.groups.flatMap((g) => g.tabIds);
    layout.activeTabId = local[0] ?? null;
  }
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
  return {
    width: layout.sidebarWidth,
    panel: { ...layout.panel },
    unread: [...unreadMarks],
    mode: layout.mode,
    managerZoom: layout.managerZoom,
  };
}

/**
 * Startup: replace what a previous page left attached, read the local Host's layout and follow
 * it, and read this client's sidebar settings. The Host has already spawned a Session per Tab.
 * Paired Hosts join afterwards (src/lib/host/hosts.svelte.ts `initHosts`).
 */
export async function initLayout(): Promise<void> {
  await resetSessions().catch(() => {});
  const saved = parseSidebarSection(await loadSection(SIDEBAR_SECTION).catch(() => undefined));
  layout.sidebarWidth = saved.width;
  layout.panel = saved.panel;
  layout.mode = saved.mode;
  layout.managerZoom = saved.managerZoom;
  for (const id of saved.unread) unreadMarks.add(id);

  // Listen before the first read: nothing between the two is missed, and an older snapshot
  // arriving after a newer one is ignored by its revision.
  await onLayout((snap) => applyHostSnapshot(LOCAL_HOST, snap));
  const first = await layoutGet();
  applyHostSnapshot(LOCAL_HOST, first, { initial: true });
  layout.ready = true;
}

// ---------------------------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------------------------

export function activeTab(): Tab | null {
  return layout.activeTabId ? (layout.tabs[layout.activeTabId] ?? null) : null;
}

/** The Host of the Tab in view; the local Host while nothing is. */
export function activeHost(): HostId {
  return activeTab()?.host ?? LOCAL_HOST;
}

export function groupOf(tab: Tab): Group | null {
  return groupsOf(tab.host).find((g) => g.id === tab.groupId) ?? null;
}

/** Tab ids in top-to-bottom sidebar order, every Host, including collapsed Groups. */
export function orderedTabIds(): string[] {
  return [...layout.groups, ...layout.sections.flatMap((s) => s.groups)].flatMap((g) => g.tabIds);
}

// ---------------------------------------------------------------------------------------------
// Actions on a Host's layout: each is one command to that Host; the snapshot it emits updates
// the mirror. The local Host's go over IPC, a paired Host's over its socket.
// ---------------------------------------------------------------------------------------------

/** The layout commands, as every Host answers them (the IPC table, one to one with the Host protocol's). */
export interface LayoutCommands {
  tabNew(opts: TabNewOptions): Promise<HostTab>;
  tabClose(tabId: string): Promise<void>;
  tabRename(tabId: string, title: string): Promise<void>;
  tabMove(tabId: string, groupId: string, index?: number): Promise<void>;
  tabActivate(tabId: string): Promise<void>;
  groupNew(name?: string | null, tabId?: string | null): Promise<HostGroup>;
  groupRename(groupId: string, name: string): Promise<void>;
  groupMove(groupId: string, index: number): Promise<void>;
  groupDelete(groupId: string): Promise<void>;
  groupSetCollapsed(groupId: string, collapsed: boolean): Promise<void>;
}

const localCommands: LayoutCommands = {
  tabNew,
  tabClose,
  tabRename,
  tabMove,
  tabActivate,
  groupNew,
  groupRename,
  groupMove,
  groupDelete,
  groupSetCollapsed,
};

function commandsFor(host: HostId): LayoutCommands {
  return isLocal(host) ? localCommands : hostCommands(host);
}

function report(what: string): (e: unknown) => void {
  return (e) => console.error(`layout: ${what} failed`, e);
}

/**
 * New Tab, on `host` (default: the Group's Host, else the Host of the Tab in view) in the active
 * Tab's Group there, right after it, spawned at that Tab's cwd (the Host's rule; `groupId` puts
 * it at the end of that Group instead). Shown as soon as its Host names it, unless `show` is
 * false (Handoff moving a Tab that was not in view). Resolves to its id.
 */
export async function newTab(opts?: { cwd?: string | null; groupId?: string; host?: HostId; show?: boolean }): Promise<string> {
  const host = opts?.host ?? (opts?.groupId ? (findGroup(opts.groupId)?.host ?? activeHost()) : activeHost());
  // Not shown, while the view is on a Tab of the same Host: the Host makes its new Tab active
  // there, and its snapshot would pull the view along; keep the view, and give the Host its
  // active Tab back.
  const view = layout.activeTabId ? (layout.tabs[layout.activeTabId] ?? null) : null;
  const keep = opts?.show === false && view?.host === host ? view.id : null;
  if (keep) pendingView = { host, tabId: keep };
  const tab = await commandsFor(host).tabNew({ cwd: opts?.cwd, groupId: opts?.groupId, ...terminals.grid() });
  if (opts?.show === false) {
    if (keep) void commandsFor(host).tabActivate(keep).catch(report("going back to a Tab"));
    return tab.id;
  }
  if (layout.tabs[tab.id]) activateTab(tab.id);
  else pendingView = { host, tabId: tab.id };
  return tab.id;
}

/** Close a Tab: its Host takes it out at once and kills its Session. */
export function closeTab(tabId: string): void {
  const tab = layout.tabs[tabId];
  if (!tab) return;
  void commandsFor(tab.host).tabClose(tabId).catch(report("closing a Tab"));
}

/** Rename a Tab; an empty title clears the rename and reverts to the automatic Title. */
export function renameTab(tabId: string, title: string): void {
  const tab = layout.tabs[tabId];
  if (!tab) return;
  void commandsFor(tab.host).tabRename(tabId, title).catch(report("renaming a Tab"));
}

/** Mark or unmark a Tab as unread. The mark stays until the user next goes to the Tab. */
export function setTabUnread(tabId: string, unread: boolean): void {
  const tab = layout.tabs[tabId];
  if (!tab || tab.unread === unread) return;
  tab.unread = unread;
  if (unread) {
    unreadMarks.add(tabId);
    markHosts.set(tabId, tab.host);
  } else {
    unreadMarks.delete(tabId);
    markHosts.delete(tabId);
  }
  scheduleSave();
}

/**
 * Go to a Tab, clearing its unread mark. Shown at once, ahead of the Host's snapshot, so the
 * Terminal switches without a round trip; the Tab's Host is told, so its own active Tab (what
 * `tab_new` there goes next to) follows. Not used on relaunch, so a mark put on the Tab in view
 * survives a restart.
 */
export function activateTab(tabId: string): void {
  const tab = layout.tabs[tabId];
  if (!tab || layout.activeTabId === tabId) return;
  layout.activeTabId = tabId;
  setTabUnread(tabId, false);
  void commandsFor(tab.host).tabActivate(tabId).catch(report("going to a Tab"));
}

/**
 * Go to the local Host's Group at `index` (0-based, sidebar order): activate the Tab last active
 * in it, else its first Tab, expanding the Group so the active Tab shows. A no-op for an empty
 * Group. Go-to-Group numbers count the local Host's Groups only (docs/architecture.md "Hosts").
 */
export function jumpToGroup(index: number): void {
  const group = layout.groups[index];
  if (!group || group.tabIds.length === 0) return;
  const remembered = lastActiveInGroup.get(group.id);
  setGroupCollapsed(group.id, false);
  activateTab(remembered && group.tabIds.includes(remembered) ? remembered : group.tabIds[0]);
}

/** A new Group at the end of `host`'s (default: the Host of the Tab in view); resolves to its id. */
export async function newGroup(name?: string, host?: HostId): Promise<string> {
  const group = await commandsFor(host ?? activeHost()).groupNew(name);
  return group.id;
}

export function renameGroup(groupId: string, name: string): void {
  const group = findGroup(groupId);
  if (!group || name.trim() === "") return; // Groups have no automatic name to fall back to.
  void commandsFor(group.host).groupRename(groupId, name).catch(report("renaming a Group"));
}

export function setGroupCollapsed(groupId: string, collapsed: boolean): void {
  const group = findGroup(groupId);
  if (!group || group.collapsed === collapsed) return;
  group.collapsed = collapsed; // shown at once; the snapshot agrees
  void commandsFor(group.host).groupSetCollapsed(groupId, collapsed).catch(report("collapsing a Group"));
}

export function toggleGroupCollapsed(groupId: string): void {
  const group = findGroup(groupId);
  if (group) setGroupCollapsed(groupId, !group.collapsed);
}

/** Delete a Group and close its Tabs. Never a Host's last Group (the caller disables the affordance too). */
export function deleteGroup(groupId: string): void {
  const group = findGroup(groupId);
  if (!group || groupsOf(group.host).length <= 1) return;
  void commandsFor(group.host).groupDelete(groupId).catch(report("deleting a Group"));
}

/** Peel a Tab out into a brand new Group of its own, on its Host (context menu: "New Group from Tab"). */
export async function newGroupFromTab(tabId: string, name?: string): Promise<string | null> {
  const tab = layout.tabs[tabId];
  if (!tab) return null;
  const group = await commandsFor(tab.host).groupNew(name, tabId);
  return group.id;
}

/**
 * Move a Tab within or across Groups of its Host, inserting at `targetIndex` (default: end of
 * Group). A Session cannot change machines, so a Group on another Host is refused (Handoff, #30,
 * is the real thing).
 */
export function moveTab(tabId: string, targetGroupId: string, targetIndex?: number): void {
  const tab = layout.tabs[tabId];
  const target = findGroup(targetGroupId);
  if (!tab || !target) return;
  if (target.host !== tab.host) {
    console.warn("layout: a Tab cannot move to another Host's Group");
    return;
  }
  void commandsFor(tab.host).tabMove(tabId, targetGroupId, targetIndex).catch(report("moving a Tab"));
}

/** Reorder a Host's Groups, moving `groupId` to `targetIndex` among them. */
export function moveGroup(groupId: string, targetIndex: number): void {
  const group = findGroup(groupId);
  if (!group) return;
  void commandsFor(group.host).groupMove(groupId, Math.max(0, targetIndex)).catch(report("moving a Group"));
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

/** Show Tabs (the sidebar and one Terminal) or Manager (every agent's lane) in this window. */
export function setMode(mode: WindowMode): void {
  if (layout.mode === mode) return;
  layout.mode = mode;
  scheduleSave();
}

export function toggleMode(): void {
  setMode(layout.mode === "manager" ? "tabs" : "manager");
}

export function setManagerZoom(zoom: ManagerZoom): void {
  if (layout.managerZoom === zoom) return;
  layout.managerZoom = zoom;
  scheduleSave();
}

/** The Session key of a Tab, or null while it has no Session. */
export function tabSessionKey(tab: Tab | null): SessionKey | null {
  return tab && tab.sessionId !== null ? sessionKey(tab.host, tab.sessionId) : null;
}

/** The local Session id of a Tab, or null for a paired Host's Tab or one without a Session. */
export function localSessionId(tab: Tab | null): SessionId | null {
  return tab && isLocal(tab.host) ? tab.sessionId : null;
}
