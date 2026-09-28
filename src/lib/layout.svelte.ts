// The layout as the webview sees it: a mirror of this Mac's layout (the local Host's snapshot:
// Groups, Tabs, order, the active Tab: src-tauri/core/src/layout/), changed only through its
// `tab_*` / `group_*` commands, plus what is this client's own: the sidebar width and
// visibility, the Panel, the user's unread marks, the window's mode (Tabs or Manager) and
// Manager's zoom. Those persist in the `sidebar` section of
// settings.json (debounced ~500 ms), never in layout.json.
//
// One set of Groups (ADR 0003). A Tab in them is local (its Session is this Mac's) or linked:
// it places a paired Host's Tab, whose Session, custom Title and cwd stay that Host's. Each
// paired Host's snapshot (its `hello` and `layout` messages, from src/lib/host/hosts.svelte.ts)
// is kept here only to fill in its linked Tabs and to keep the links in line with it: a Tab the
// Host has and this Mac has not linked (made from a phone, or there before pairing) is linked
// into a Group named after the Host and marked Unread; a link whose Tab is gone there goes. The
// Host's own Groups are not shown. Moving a Tab between Groups is this Mac's alone; closing,
// renaming and activating a linked Tab go to its Host. See docs/architecture.md "Hosts",
// ADR 0003, and CONTEXT.md.
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
  linksReconcile,
  onLayout,
  resetSessions,
  tabActivate,
  tabClose,
  tabLink,
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
import { hostCommands, hostName, hostState, hostTransport } from "./host/hosts.svelte";
import { hostTabOrder, linksOutOfLine } from "./host/links";
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

/**
 * A Tab as this client shows it. For a linked Tab, `sessionId`, `customTitle` and `lastCwd` are
 * its Host's Tab's (null until that Host's snapshot is in).
 */
export interface Tab extends HostTab {
  /** Where its Session runs: this Mac (`LOCAL_HOST`) for a local Tab, the link's Host otherwise. */
  host: HostId;
  /** The Tab's id on its Host: its own for a local Tab, the link's for a linked one. */
  hostTabId: string;
  /** Marked unread by the user; cleared when the user next goes to the Tab. */
  unread: boolean;
}

/** A Group: this Mac's, holding local and linked Tabs alike. */
export type Group = HostGroup;

interface LayoutState {
  /** This Mac's Groups, in sidebar order. */
  groups: Group[];
  /** Every Tab, local and linked, by this Mac's id. */
  tabs: Record<string, Tab>;
  /** The Tab in view: whose Terminal shows. */
  activeTabId: string | null;
  sidebarWidth: number;
  /** Cmd-B toggle. Not persisted: the sidebar is visible again on relaunch. */
  sidebarVisible: boolean;
  panel: PanelState;
  /** Tabs (the sidebar and one Terminal) or Manager (every agent's lane). Per window, persisted. */
  mode: WindowMode;
  managerZoom: ManagerZoom;
  /** The Tab whose Terminal Manager shows (the selected one), if any. Not persisted. */
  managerTabId: string | null;
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
  tabs: {},
  activeTabId: null,
  sidebarWidth: DEFAULT_SIDEBAR_WIDTH,
  sidebarVisible: true,
  panel: { ...DEFAULT_PANEL },
  mode: "tabs",
  managerZoom: DEFAULT_MANAGER_ZOOM,
  managerTabId: null,
  ready: false,
});

/** This Mac's layout as last shown. */
let local: LayoutSnapshot | null = null;
/** Each paired Host's layout as last heard, for its linked Tabs; kept while it is offline. */
const hostLayouts = new Map<HostId, LayoutSnapshot>();
/**
 * Hosts with a `tab_new` from this Mac in flight, until their snapshot names the new Tab (its
 * reply can come first): that Tab is not a stray, so their links wait.
 */
const creating = new Map<HostId, number>();
/** How long a Host's links wait for its snapshot to name a Tab this Mac made there. */
const CREATE_WAIT_MS = 5000;
/** Waiting for a Host's snapshot to name a Tab: resolved by `applyHostSnapshot`. */
const namedWaiters = new Map<HostId, { tabId: string; resolve: () => void }[]>();

/** Resolves once `host`'s snapshot has Tab `tabId`, or after `CREATE_WAIT_MS`. */
function whenHostNames(host: HostId, tabId: string | null): Promise<void> {
  if (tabId === null || hostLayouts.get(host)?.tabs[tabId]) return Promise.resolve();
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, CREATE_WAIT_MS);
    const list = namedWaiters.get(host) ?? [];
    list.push({ tabId, resolve: () => (clearTimeout(timer), resolve()) });
    namedWaiters.set(host, list);
  });
}
/**
 * Host Tab ids this Mac asked their Host to close, until its snapshot drops them: its reply can
 * come before that snapshot, and a Tab unlinked here meanwhile must not come back as a stray.
 */
const closing = new Map<HostId, Set<string>>();

function closingOn(host: HostId): Set<string> {
  let ids = closing.get(host);
  if (!ids) closing.set(host, (ids = new Set()));
  return ids;
}

/** Ask `host` to close its Tab `hostTabId`. */
function closeOnHost(host: HostId, hostTabId: string): Promise<void> {
  closingOn(host).add(hostTabId);
  return hostCommands(host)
    .tabClose(hostTabId)
    .catch((e) => {
      closing.get(host)?.delete(hostTabId);
      throw e;
    });
}

/** Tab ids the user marked unread. Persisted with the sidebar settings. */
const unreadMarks = new Set<string>();
/** Marks put on Tabs this Mac's layout has not named yet (a stray just linked): not pruned. */
const freshMarks = new Set<string>();

/** SessionKey -> Tab id, kept in step with `layout.tabs` for O(1) lookups. */
const sessionToTab = new Map<SessionKey, string>();

/** Group id -> the Tab last active in it, so jumping to a Group lands where you left off. */
const lastActiveInGroup = new Map<string, string>();

/** A Tab this client made or wants in view once the layout names it. */
let pendingView: string | null = null;

$effect.root(() => {
  $effect(() => {
    const tab = layout.activeTabId ? layout.tabs[layout.activeTabId] : null;
    if (tab) lastActiveInGroup.set(tab.groupId, tab.id);
  });
});

export function tabIdForSession(key: SessionKey): string | null {
  return sessionToTab.get(key) ?? null;
}

function findGroup(groupId: string): Group | null {
  return layout.groups.find((g) => g.id === groupId) ?? null;
}

function transportFor(host: HostId): SessionTransport {
  return isLocal(host) ? localTransport : hostTransport(host);
}

function hostOnline(host: HostId): boolean {
  return hostState(host)?.status === "online";
}

// ---------------------------------------------------------------------------------------------
// The mirror
// ---------------------------------------------------------------------------------------------

/**
 * Build the Tabs shown from this Mac's layout and each Host's: a linked Tab takes its Session,
 * Title and cwd from its Host's Tab. Every Tab's Session gets a Terminal. Unread marks for Tabs
 * that are gone are dropped (and saved with the next change).
 */
function rebuild(): void {
  if (!local) return;
  const tabs: Record<string, Tab> = {};
  sessionToTab.clear();
  for (const t of Object.values(local.tabs)) {
    const host = t.link?.hostId ?? LOCAL_HOST;
    const there = t.link ? (hostLayouts.get(host)?.tabs[t.link.tabId] ?? null) : t;
    const tab: Tab = {
      ...t,
      sessionId: there?.sessionId ?? null,
      customTitle: there?.customTitle ?? null,
      lastCwd: there?.lastCwd ?? null,
      host,
      hostTabId: t.link?.tabId ?? t.id,
      unread: unreadMarks.has(t.id),
    };
    tabs[t.id] = tab;
    freshMarks.delete(t.id);
    if (tab.sessionId !== null) {
      const key = sessionKey(host, tab.sessionId);
      sessionToTab.set(key, t.id);
      terminals.attach(key, transportFor(host));
    }
  }
  let pruned = false;
  for (const id of unreadMarks) {
    if (!tabs[id] && !freshMarks.has(id)) {
      unreadMarks.delete(id);
      pruned = true;
    }
  }
  layout.groups = local.groups.map((g) => ({ ...g, tabIds: [...g.tabIds] }));
  layout.tabs = tabs;
  if (pruned) scheduleSave();
}

/**
 * Take this Mac's snapshot. `initial` accepts it at the revision shown (the first read);
 * otherwise events only move forward. The Tab in view follows the layout's active Tab, which
 * going to a Tab keeps in step, unless this client is waiting to show a Tab it just made.
 */
function applyLocalSnapshot(snap: LayoutSnapshot, opts: { initial?: boolean } = {}): void {
  if (local && (opts.initial ? snap.revision < local.revision : snap.revision <= local.revision)) return;
  local = snap;
  rebuild();
  if (pendingView !== null) {
    if (layout.tabs[pendingView]) {
      layout.activeTabId = pendingView;
      pendingView = null;
    }
  } else {
    layout.activeTabId = snap.activeTabId && layout.tabs[snap.activeTabId] ? snap.activeTabId : null;
  }
  if (layout.ready) for (const host of hostLayouts.keys()) void reconcile(host);
}

/**
 * Take a paired Host's snapshot: its linked Tabs show what it says, and its links follow its
 * Tabs. `fresh` accepts any revision (a `hello`: the Host may have restarted); otherwise events
 * only move forward. It never moves the view.
 */
export function applyHostSnapshot(host: HostId, snap: LayoutSnapshot, opts: { fresh?: boolean } = {}): void {
  const shown = hostLayouts.get(host);
  if (shown && !opts.fresh && snap.revision <= shown.revision) return;
  hostLayouts.set(host, snap);
  const ids = closing.get(host);
  if (ids) for (const id of ids) if (!snap.tabs[id]) ids.delete(id);
  const waiters = namedWaiters.get(host);
  if (waiters) {
    for (const w of waiters) if (snap.tabs[w.tabId]) w.resolve();
    namedWaiters.set(host, waiters.filter((w) => !snap.tabs[w.tabId]));
  }
  rebuild();
  void reconcile(host);
}

/**
 * Bring `host`'s links in line with the Tabs it has, when they differ: its Tabs with no link are
 * linked into a Group named after it and marked Unread; links whose Tab is gone go. Waits while
 * this Mac is making a Tab there (that Tab is linked where it was asked for, then this runs).
 */
async function reconcile(host: HostId): Promise<void> {
  const snap = hostLayouts.get(host);
  if (!layout.ready || !local || !snap || (creating.get(host) ?? 0) > 0) return;
  const there = hostTabOrder(snap, closing.get(host));
  if (!linksOutOfLine(local, host, there)) return;
  try {
    for (const id of await linksReconcile(host, there, hostName(host))) markUnread(id);
  } catch (e) {
    report("linking a Host's Tabs")(e);
  }
}

function markUnread(tabId: string): void {
  unreadMarks.add(tabId);
  const tab = layout.tabs[tabId];
  if (tab) tab.unread = true;
  else freshMarks.add(tabId);
  scheduleSave();
}

/** A paired Host was removed: its linked Tabs go (a view on one falls to its neighbour). */
export function dropHost(host: HostId): void {
  if (isLocal(host)) return;
  hostLayouts.delete(host);
  creating.delete(host);
  for (const w of namedWaiters.get(host) ?? []) w.resolve();
  namedWaiters.delete(host);
  closing.delete(host);
  rebuild();
  void linksReconcile(host, [], "").catch(report("dropping a Host's Tabs"));
}

/** The paired Hosts are read: links to a Host that is not among them any more go. */
export function dropUnknownHosts(known: HostId[]): void {
  const keep = new Set(known);
  const stale = new Set(Object.values(layout.tabs).flatMap((t) => (t.link && !keep.has(t.link.hostId) ? [t.link.hostId] : [])));
  for (const host of stale) dropHost(host);
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
 * it, and read this client's sidebar settings. The Host has already spawned a Session per local
 * Tab. Paired Hosts join afterwards (src/lib/host/hosts.svelte.ts `initHosts`) and fill in the
 * linked Tabs.
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
  await onLayout((snap) => applyLocalSnapshot(snap));
  const first = await layoutGet();
  applyLocalSnapshot(first, { initial: true });
  layout.ready = true;
}

// ---------------------------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------------------------

export function activeTab(): Tab | null {
  return layout.activeTabId ? (layout.tabs[layout.activeTabId] ?? null) : null;
}

/** The Tab whose Terminal Manager shows, or null while it shows none. */
export function managerTab(): Tab | null {
  return layout.managerTabId ? (layout.tabs[layout.managerTabId] ?? null) : null;
}

/** The Tab whose Terminal shows now: the active Tab in Tabs mode, Manager's in Manager. */
export function tabInView(): Tab | null {
  return layout.mode === "manager" ? managerTab() : activeTab();
}

/** The Host of the Tab in view; the local Host while nothing is. */
export function activeHost(): HostId {
  return activeTab()?.host ?? LOCAL_HOST;
}

export function groupOf(tab: Tab): Group | null {
  return findGroup(tab.groupId);
}

/** Tab ids in top-to-bottom sidebar order, including collapsed Groups. */
export function orderedTabIds(): string[] {
  return layout.groups.flatMap((g) => g.tabIds);
}

/** A paired Host's own Groups, as it last said (Handoff lets the user pick one there). */
export function hostGroups(host: HostId): HostGroup[] {
  return hostLayouts.get(host)?.groups ?? [];
}

// ---------------------------------------------------------------------------------------------
// Actions. This Mac's layout changes by its commands (over IPC); its snapshot updates the
// mirror. What only a linked Tab's Host can do (make, close, rename, activate its Tab) goes to
// that Host over its socket (src/lib/host/hosts.svelte.ts `hostCommands`).
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

function report(what: string): (e: unknown) => void {
  return (e) => console.error(`layout: ${what} failed`, e);
}

export interface NewTabOptions {
  /** Where its Session runs: default the Host of `after`, else of the Tab in view (a `groupId` alone: this Mac). */
  host?: HostId;
  /** This Mac's Group to put it in, at the end (unless `after` is given). */
  groupId?: string;
  /** The Tab to put it right after (default: the Tab in view, unless `groupId` is given). */
  after?: string;
  /** Where the Session starts: default the cwd of the Tab it goes after, when on the same Host. */
  cwd?: string | null;
  /** A paired Host's own Group for the Tab there (Handoff's choice); default its active Tab's. */
  hostGroupId?: string;
  /** false: leave the view where it is (Handoff moving a Tab that was not in view). */
  show?: boolean;
}

/**
 * New Tab: on the Host of the Tab in view (so ⌘T next to a Tab on a paired Host makes one
 * there, and next to a local Tab one here), right after it in its Group, at its cwd; `groupId`
 * alone puts a local Tab at the end of that Group. Shown as soon as the layout names it, unless
 * `show` is false. Resolves to its id in this Mac's layout.
 */
export async function newTab(opts: NewTabOptions = {}): Promise<string> {
  const anchor = opts.after ? (layout.tabs[opts.after] ?? null) : opts.groupId ? null : activeTab();
  const host = opts.host ?? (anchor?.host ?? LOCAL_HOST);
  return isLocal(host) ? newLocalTab(opts, anchor) : newTabOnHost(host, opts, anchor);
}

async function newLocalTab(opts: NewTabOptions, anchor: Tab | null): Promise<string> {
  // Not shown: this Mac makes its new Tab active, and its snapshot would pull the view along;
  // keep the view, and give the layout its active Tab back.
  const keep = opts.show === false ? layout.activeTabId : null;
  if (keep) pendingView = keep;
  const tab = await tabNew({
    cwd: opts.cwd ?? (anchor && isLocal(anchor.host) ? undefined : null),
    groupId: opts.groupId ?? anchor?.groupId,
    afterTabId: anchor?.id,
    ...terminals.grid(),
  });
  if (opts.show === false) {
    if (keep) void tabActivate(keep).catch(report("going back to a Tab"));
    return tab.id;
  }
  if (layout.tabs[tab.id]) activateTab(tab.id);
  else pendingView = tab.id;
  return tab.id;
}

/**
 * A Tab on paired Host `host`: made there (after the anchor's Tab when that is on the same Host,
 * at its cwd), then linked here after the anchor. Its links wait meanwhile, so the new Tab is
 * never taken for a stray.
 */
async function newTabOnHost(host: HostId, opts: NewTabOptions, anchor: Tab | null): Promise<string> {
  const sameHost = anchor?.host === host ? anchor : null;
  creating.set(host, (creating.get(host) ?? 0) + 1);
  let madeId: string | null = null;
  let linked: HostTab;
  try {
    const made = await hostCommands(host).tabNew({
      groupId: opts.hostGroupId,
      afterTabId: opts.hostGroupId ? undefined : sameHost?.hostTabId,
      cwd: opts.cwd ?? sameHost?.lastCwd ?? undefined,
      ...terminals.grid(),
    });
    madeId = made.id;
    linked = await tabLink({ hostId: host, tabId: made.id }, opts.groupId ?? anchor?.groupId ?? null, anchor?.id ?? null);
  } finally {
    void whenHostNames(host, madeId).then(() => {
      creating.set(host, (creating.get(host) ?? 1) - 1);
      void reconcile(host);
    });
  }
  if (opts.show !== false) {
    if (layout.tabs[linked.id]) activateTab(linked.id);
    else {
      // The Host made it its active Tab already; this Mac's layout follows.
      pendingView = linked.id;
      void tabActivate(linked.id).catch(report("going to a Tab"));
    }
  }
  return linked.id;
}

/**
 * Close a Tab: a local one here (its Session is killed), a linked one on its Host, then its link
 * (the Host's snapshot would drop it anyway). A Host that answers but no longer has the Tab gets
 * its link dropped too. The caller checks the Host is connected (closeTabFlow.ts).
 */
export function closeTab(tabId: string): void {
  const tab = layout.tabs[tabId];
  if (!tab) return;
  if (isLocal(tab.host)) {
    void tabClose(tabId).catch(report("closing a Tab"));
    return;
  }
  const unlink = () => void tabClose(tabId).catch(() => {}); // gone already: the Host's snapshot won
  void closeOnHost(tab.host, tab.hostTabId).then(unlink, (e) => {
    if (hostOnline(tab.host)) unlink();
    else report("closing a Tab")(e);
  });
}

/** Rename a Tab; an empty title clears the rename and reverts to the automatic Title. A linked Tab's Title is its Host's. */
export function renameTab(tabId: string, title: string): void {
  const tab = layout.tabs[tabId];
  if (!tab) return;
  const done = isLocal(tab.host) ? tabRename(tabId, title) : hostCommands(tab.host).tabRename(tab.hostTabId, title);
  void done.catch(report("renaming a Tab"));
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
 * Go to a Tab, clearing its unread mark. Shown at once, ahead of the snapshot, so the Terminal
 * switches without a round trip. This Mac's layout is told (what a local `tab_new` goes next
 * to), and for a linked Tab its Host too, so its own active Tab follows. Not used on relaunch,
 * so a mark put on the Tab in view survives a restart.
 */
export function activateTab(tabId: string): void {
  const tab = layout.tabs[tabId];
  if (!tab || layout.activeTabId === tabId) return;
  layout.activeTabId = tabId;
  setTabUnread(tabId, false);
  void tabActivate(tabId).catch(report("going to a Tab"));
  if (!isLocal(tab.host) && hostOnline(tab.host)) void hostCommands(tab.host).tabActivate(tab.hostTabId).catch(() => {});
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
  const group = findGroup(groupId);
  if (!group || name.trim() === "") return; // Groups have no automatic name to fall back to.
  void groupRename(groupId, name).catch(report("renaming a Group"));
}

export function setGroupCollapsed(groupId: string, collapsed: boolean): void {
  const group = findGroup(groupId);
  if (!group || group.collapsed === collapsed) return;
  group.collapsed = collapsed; // shown at once; the snapshot agrees
  void groupSetCollapsed(groupId, collapsed).catch(report("collapsing a Group"));
}

export function toggleGroupCollapsed(groupId: string): void {
  const group = findGroup(groupId);
  if (group) setGroupCollapsed(groupId, !group.collapsed);
}

/**
 * Delete a Group and close its Tabs: its linked Tabs on their Hosts first, then the Group here.
 * Never the last Group (the caller disables the affordance too). A linked Tab whose Host is not
 * connected only loses its link, and comes back into the Host's Group when the Host does.
 */
export async function deleteGroup(groupId: string): Promise<void> {
  const group = findGroup(groupId);
  if (!group || layout.groups.length <= 1) return;
  const closes = group.tabIds.flatMap((id) => {
    const t = layout.tabs[id];
    return t && !isLocal(t.host) ? [closeOnHost(t.host, t.hostTabId)] : [];
  });
  await Promise.allSettled(closes);
  await groupDelete(groupId).catch(report("deleting a Group"));
}

/** Peel a Tab out into a brand new Group of its own (context menu: "New Group from Tab"). */
export async function newGroupFromTab(tabId: string, name?: string): Promise<string | null> {
  if (!layout.tabs[tabId]) return null;
  const group = await groupNew(name, tabId);
  return group.id;
}

/**
 * Move a Tab within or across Groups, inserting at `targetIndex` (default: end of Group). Local
 * and linked Tabs alike: a move is this Mac's alone and never moves a Session (Handoff does).
 */
export function moveTab(tabId: string, targetGroupId: string, targetIndex?: number): void {
  if (!layout.tabs[tabId] || !findGroup(targetGroupId)) return;
  void tabMove(tabId, targetGroupId, targetIndex).catch(report("moving a Tab"));
}

/** Reorder the Groups, moving `groupId` to `targetIndex` among them. */
export function moveGroup(groupId: string, targetIndex: number): void {
  if (!findGroup(groupId)) return;
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

/** Show Tabs (the sidebar and one Terminal) or Manager (every agent's lane) in this window. */
export function setMode(mode: WindowMode): void {
  if (layout.mode === mode) return;
  layout.mode = mode;
  scheduleSave();
}

/**
 * Show a Tab's Terminal in Manager (null: none), clearing its unread mark as going to it does.
 * The active Tab stays: Tabs mode comes back to where the user left it.
 */
export function showInManager(tabId: string | null): void {
  const tab = tabId === null ? null : layout.tabs[tabId];
  layout.managerTabId = tab ? tab.id : null;
  if (tab) setTabUnread(tab.id, false);
}

export function toggleMode(): void {
  setMode(layout.mode === "manager" ? "tabs" : "manager");
}

export function setManagerZoom(zoom: ManagerZoom): void {
  if (layout.managerZoom === zoom) return;
  layout.managerZoom = zoom;
  scheduleSave();
}

/** The Session key of a Tab, or null while it has none. */
export function tabSessionKey(tab: Tab | null): SessionKey | null {
  return tab && tab.sessionId !== null ? sessionKey(tab.host, tab.sessionId) : null;
}

/** The local Session id of a Tab, or null for a linked Tab or one without a Session. */
export function localSessionId(tab: Tab | null): SessionId | null {
  return tab && isLocal(tab.host) ? tab.sessionId : null;
}
