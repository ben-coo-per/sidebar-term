// Typed wrappers over the Tauri commands in src-tauri/src/lib.rs.
// CONTRACT: owned by the tech lead. Outside Tauri (plain `vite dev` in a browser) every call
// is routed to ./mock.ts so the UI can be developed without the Rust build.

import { invoke, Channel, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  EVENT_ACTIVITY,
  EVENT_AGENT_EVENT,
  EVENT_CAFFEINATE,
  EVENT_LAYOUT,
  EVENT_MEMORY_GUARD,
  EVENT_MENU_SETTINGS,
  EVENT_REMOTE,
  EVENT_SESSION_EXIT,
  EVENT_SESSION_INFO,
  EVENT_SESSION_RESIZED,
  EVENT_USAGE,
  type ActivitySnapshot,
  type AgentEvent,
  type AgentKind,
  type ClaudeConversation,
  type ConversationFiles,
  type Group,
  type HandoffProbe,
  type LayoutSnapshot,
  type LinkedHost,
  type Pairing,
  type RemoteSnapshot,
  type GuardSnapshot,
  type ResumeEntry,
  type SessionExit,
  type SessionId,
  type SessionInfo,
  type SessionResized,
  type Tab,
  type TabLink,
  type UsageSnapshot,
} from "./types";
import * as mock from "./mock";

export const inTauri: boolean = isTauri();

/**
 * Take a Session's output from here on: raw pty bytes, in order, to feed straight to
 * `terminal.write(bytes)`, starting with what the Session printed before (its shell's prompt).
 * Rejects for a Session that is gone.
 */
export async function attachSession(sessionId: SessionId, onData: (bytes: Uint8Array) => void): Promise<void> {
  if (!inTauri) return mock.attachSession(sessionId, onData);
  const channel = new Channel<ArrayBuffer | number[]>();
  channel.onmessage = (msg) => {
    onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg));
  };
  return invoke("session_attach", { sessionId, onData: channel });
}

export function writeSession(sessionId: SessionId, data: string): Promise<void> {
  if (!inTauri) return mock.writeSession(sessionId, data);
  return invoke("session_write", { sessionId, data });
}

export function resizeSession(sessionId: SessionId, cols: number, rows: number): Promise<void> {
  if (!inTauri) return mock.resizeSession(sessionId, cols, rows);
  return invoke("session_resize", { sessionId, cols, rows });
}

export function pauseSession(sessionId: SessionId): Promise<void> {
  if (!inTauri) return Promise.resolve();
  return invoke("session_pause", { sessionId });
}

export function resumeSession(sessionId: SessionId): Promise<void> {
  if (!inTauri) return Promise.resolve();
  return invoke("session_resume", { sessionId });
}

/**
 * Called once at startup: the Sessions a previous page's Terminals were attached to are replaced
 * (a reload would otherwise leave those shells with nowhere to send output); the app's first page
 * finds nothing attached and keeps what the Host spawned at launch.
 */
export function resetSessions(): Promise<void> {
  if (!inTauri) return Promise.resolve();
  return invoke("session_reset");
}

// --- The layout: the Host's, mirrored here (docs/architecture.md "Split of responsibility") ----

/** The whole layout, for the first read; every change after that arrives on `onLayout`. */
export function layoutGet(): Promise<LayoutSnapshot> {
  if (!inTauri) return mock.layoutGet();
  return invoke("layout_get");
}

export function onLayout(cb: (snapshot: LayoutSnapshot) => void): Promise<UnlistenFn> {
  if (!inTauri) return mock.onLayout(cb);
  return listen<LayoutSnapshot>(EVENT_LAYOUT, (e) => cb(e.payload));
}

export interface TabNewOptions {
  /** The Group to add to; the active Tab's, else the first, when omitted. */
  groupId?: string | null;
  /** The Tab to go right after; the active Tab when it is in the Group, else the end, when omitted. */
  afterTabId?: string | null;
  /** Where the Session starts; the active Tab's last cwd, else home, when omitted. */
  cwd?: string | null;
  /** The pty's size, so the Terminal's first fit is a no-op. */
  cols?: number;
  rows?: number;
}

/** A new Tab with its Session, active. Resolves to the Tab. */
export function tabNew(opts: TabNewOptions = {}): Promise<Tab> {
  if (!inTauri) return mock.tabNew(opts);
  return invoke("tab_new", {
    groupId: opts.groupId ?? null,
    afterTabId: opts.afterTabId ?? null,
    cwd: opts.cwd ?? null,
    cols: opts.cols ?? null,
    rows: opts.rows ?? null,
  });
}

/** Close a Tab and kill its Session, no questions asked (src/lib/sidebar/closeTabFlow.ts asks). */
export function tabClose(tabId: string): Promise<void> {
  if (!inTauri) return mock.tabClose(tabId);
  return invoke("tab_close", { tabId });
}

/** Rename a Tab; an empty title restores the automatic Title. */
export function tabRename(tabId: string, title: string): Promise<void> {
  if (!inTauri) return mock.tabRename(tabId, title);
  return invoke("tab_rename", { tabId, title });
}

/** Move a Tab to `index` in a Group (default: its end). */
export function tabMove(tabId: string, groupId: string, index?: number): Promise<void> {
  if (!inTauri) return mock.tabMove(tabId, groupId, index);
  return invoke("tab_move", { tabId, groupId, index: index ?? null });
}

export function tabActivate(tabId: string): Promise<void> {
  if (!inTauri) return mock.tabActivate(tabId);
  return invoke("tab_activate", { tabId });
}

/**
 * Link a paired Host's Tab into this Mac's layout (ADR 0003), right after `afterTabId`, else as
 * a new Tab would go; not made active. An existing link to the same Tab is handed back as it is.
 */
export function tabLink(link: TabLink, groupId?: string | null, afterTabId?: string | null): Promise<Tab> {
  if (!inTauri) return mock.tabLink(link, groupId, afterTabId);
  return invoke("tab_link", { hostId: link.hostId, tabId: link.tabId, groupId: groupId ?? null, afterTabId: afterTabId ?? null });
}

/**
 * A paired Host's links follow the Tabs it has (`tabIds`, in its order): a link whose Tab is
 * gone goes, and a Tab with no link is linked at the end of the Group named `groupName` (made
 * if missing). Resolves to the ids of the Tabs linked now.
 */
export function linksReconcile(hostId: string, tabIds: string[], groupName: string): Promise<string[]> {
  if (!inTauri) return mock.linksReconcile(hostId, tabIds, groupName);
  return invoke("links_reconcile", { hostId, tabIds, groupName });
}

/** A new Group at the end ("New Group" unless named); with `tabId`, that Tab moves into it. */
export function groupNew(name?: string | null, tabId?: string | null): Promise<Group> {
  if (!inTauri) return mock.groupNew(name, tabId);
  return invoke("group_new", { name: name ?? null, tabId: tabId ?? null });
}

export function groupRename(groupId: string, name: string): Promise<void> {
  if (!inTauri) return mock.groupRename(groupId, name);
  return invoke("group_rename", { groupId, name });
}

export function groupMove(groupId: string, index: number): Promise<void> {
  if (!inTauri) return mock.groupMove(groupId, index);
  return invoke("group_move", { groupId, index });
}

/** Delete a Group and close every Tab in it. Rejects for the last Group. */
export function groupDelete(groupId: string): Promise<void> {
  if (!inTauri) return mock.groupDelete(groupId);
  return invoke("group_delete", { groupId });
}

export function groupSetCollapsed(groupId: string, collapsed: boolean): Promise<void> {
  if (!inTauri) return mock.groupSetCollapsed(groupId, collapsed);
  return invoke("group_set_collapsed", { groupId, collapsed });
}

/** Fresh probe of one Session, bypassing the monitor tick. null if the Session is gone. */
export function sessionInfo(sessionId: SessionId): Promise<SessionInfo | null> {
  if (!inTauri) return mock.sessionInfo(sessionId);
  return invoke("session_info", { sessionId });
}

export function onSessionInfo(cb: (info: SessionInfo) => void): Promise<UnlistenFn> {
  if (!inTauri) return mock.onSessionInfo(cb);
  return listen<SessionInfo>(EVENT_SESSION_INFO, (e) => cb(e.payload));
}

export function onSessionExit(cb: (exit: SessionExit) => void): Promise<UnlistenFn> {
  if (!inTauri) return mock.onSessionExit(cb);
  return listen<SessionExit>(EVENT_SESSION_EXIT, (e) => cb(e.payload));
}

/** A local Session's pty took another size: this webview's own resize, or a phone's that took the size. */
export function onSessionResized(cb: (resized: SessionResized) => void): Promise<UnlistenFn> {
  if (!inTauri) return mock.onSessionResized(cb);
  return listen<SessionResized>(EVENT_SESSION_RESIZED, (e) => cb(e.payload));
}

/** The local Host's last agent events, oldest first; later ones arrive through `onAgentEvent`. */
export function agentEvents(): Promise<AgentEvent[]> {
  if (!inTauri) return mock.agentEvents();
  return invoke("agent_events");
}

export function onAgentEvent(cb: (event: AgentEvent) => void): Promise<UnlistenFn> {
  if (!inTauri) return mock.onAgentEvent(cb);
  return listen<AgentEvent>(EVENT_AGENT_EVENT, (e) => cb(e.payload));
}

/** Answer the question a local Session's agent is waiting on with option `option` (0-based). */
export function agentAnswer(sessionId: SessionId, pendingId: number, option: number): Promise<void> {
  if (!inTauri) return mock.agentAnswer(sessionId, pendingId, option);
  return invoke("agent_answer", { sessionId, pendingId, option });
}

/** Stop holding that question: the agent asks it in its Terminal instead. */
export function agentRelease(sessionId: SessionId, pendingId: number): Promise<void> {
  if (!inTauri) return mock.agentRelease(sessionId, pendingId);
  return invoke("agent_release", { sessionId, pendingId });
}

/** Start or stop sampling Activity; while on, `onActivity` fires every ~2 s. Sampling runs `ps`. */
export function watchActivity(on: boolean): Promise<void> {
  if (!inTauri) return mock.watchActivity(on);
  return invoke("activity_watch", { on });
}

export function onActivity(cb: (snapshot: ActivitySnapshot) => void): Promise<UnlistenFn> {
  if (!inTauri) return mock.onActivity(cb);
  return listen<ActivitySnapshot>(EVENT_ACTIVITY, (e) => cb(e.payload));
}

/** Memory Guard's state. */
export function guardState(): Promise<GuardSnapshot> {
  if (!inTauri) return mock.guardState();
  return invoke("guard_state");
}

/**
 * Turn Memory Guard on or off and set its limit (percent of physical memory, 50..95). Off thaws
 * every frozen Tab. Resolves to the new state.
 */
export function setGuard(on: boolean, limitPercent: number): Promise<GuardSnapshot> {
  if (!inTauri) return mock.setGuard(on, limitPercent);
  return invoke("guard_set", { on, limitPercent });
}

/** The Session whose Tab is in view: Memory Guard never freezes it, and thaws it if frozen. */
export function guardVisible(sessionId: SessionId | null): Promise<void> {
  if (!inTauri) return mock.guardVisible(sessionId);
  return invoke("guard_visible", { sessionId });
}

/** Freeze a Session by hand (Memory Guard need not be on). Rejects for the Session in view. */
export function guardFreeze(sessionId: SessionId): Promise<GuardSnapshot> {
  if (!inTauri) return mock.guardFreeze(sessionId);
  return invoke("guard_freeze", { sessionId });
}

/** Thaw a frozen Session without going to its Tab. */
export function guardThaw(sessionId: SessionId): Promise<GuardSnapshot> {
  if (!inTauri) return mock.guardThaw(sessionId);
  return invoke("guard_thaw", { sessionId });
}

/** Memory Guard froze or thawed a Tab, or was turned on or off. */
export function onGuard(cb: (snapshot: GuardSnapshot) => void): Promise<UnlistenFn> {
  if (!inTauri) return mock.onGuard(cb);
  return listen<GuardSnapshot>(EVENT_MEMORY_GUARD, (e) => cb(e.payload));
}

/**
 * Start (or change the agents of) or stop reading Usage. While on, `onUsage` fires right away and
 * then whenever a number changes. Reading Claude Code's usage calls api.anthropic.com every 10
 * minutes (its last answer is kept across launches and shown until then), backing off on a 429.
 */
export function watchUsage(on: boolean, agents: AgentKind[]): Promise<void> {
  if (!inTauri) return mock.watchUsage(on, agents);
  return invoke("usage_watch", { on, agents });
}

export function onUsage(cb: (snapshot: UsageSnapshot) => void): Promise<UnlistenFn> {
  if (!inTauri) return mock.onUsage(cb);
  return listen<UsageSnapshot>(EVENT_USAGE, (e) => cb(e.payload));
}

/** Whether Caffeinate is keeping this Mac awake (a background `caffeinate` run). */
export function caffeinateState(): Promise<boolean> {
  if (!inTauri) return mock.caffeinateState();
  return invoke("caffeinate_state");
}

/** Turn Caffeinate on or off; resolves to whether it is on now. */
export function setCaffeinate(on: boolean): Promise<boolean> {
  if (!inTauri) return mock.setCaffeinate(on);
  return invoke("caffeinate_set", { on });
}

/** Caffeinate turned off on its own. Never fires outside Tauri. */
export function onCaffeinate(cb: (on: boolean) => void): Promise<UnlistenFn> {
  if (!inTauri) return Promise.resolve(() => {});
  return listen<boolean>(EVENT_CAFFEINATE, (e) => cb(e.payload));
}

/** Where Remote stands, after re-reading Tailscale's state (runs its CLI: not for a hot path). */
export function remoteState(): Promise<RemoteSnapshot> {
  if (!inTauri) return mock.remoteState();
  return invoke("remote_state");
}

/** Turn Remote on or off; resolves to the state now, or rejects with why it could not start. */
export function setRemote(on: boolean): Promise<RemoteSnapshot> {
  if (!inTauri) return mock.setRemote(on);
  return invoke("remote_set", { on });
}

/** Start a pairing: the code (and QR link) a phone presents once to be let in. */
export function remotePairBegin(): Promise<Pairing> {
  if (!inTauri) return mock.remotePairBegin();
  return invoke("remote_pair_begin");
}

export function remotePairCancel(): Promise<void> {
  if (!inTauri) return mock.remotePairCancel();
  return invoke("remote_pair_cancel");
}

/** Forget a paired phone; its token stops working at its next connection. */
export function remoteRevoke(id: string): Promise<void> {
  if (!inTauri) return mock.remoteRevoke(id);
  return invoke("remote_revoke", { id });
}

/** Tell Remote the paired Hosts (no tokens): what a phone that shows the linked Tabs reaches them by. */
export function remoteHostsSet(hosts: LinkedHost[]): Promise<void> {
  if (!inTauri) return Promise.resolve();
  return invoke("remote_hosts_set", { hosts });
}

/** Remote's state changed: turned on or off, a phone connected or paired, a pairing expired. */
export function onRemote(cb: (snapshot: RemoteSnapshot) => void): Promise<UnlistenFn> {
  if (!inTauri) return mock.onRemote(cb);
  return listen<RemoteSnapshot>(EVENT_REMOTE, (e) => cb(e.payload));
}

/** The app menu's "Settings…" item. Never fires outside Tauri (the browser has no app menu). */
export function onMenuSettings(cb: () => void): Promise<UnlistenFn> {
  if (!inTauri) return Promise.resolve(() => {});
  return listen(EVENT_MENU_SETTINGS, () => cb());
}

/**
 * For each path printed in the Session (relative, `~/...` or absolute), the absolute path of the
 * file or directory it names, or null when there is none (always null in a remote Session).
 */
export function resolvePaths(sessionId: SessionId, candidates: string[]): Promise<(string | null)[]> {
  if (!inTauri) return mock.resolvePaths(sessionId, candidates);
  return invoke("path_resolve", { sessionId, candidates });
}

/** Open a file or directory (an absolute path from `resolvePaths`) in its default app. */
export function openPath(path: string): Promise<void> {
  if (!inTauri) return mock.openPath(path);
  return invoke("path_open", { path });
}

/**
 * What Handoff needs to know about a local Session before moving its Tab: fresh facts, its
 * Resume entry, the Claude Code conversation running in it, its checkout's git status (runs
 * `git`). null for a Session that is gone.
 */
export function handoffProbe(sessionId: SessionId): Promise<HandoffProbe | null> {
  if (!inTauri) return mock.handoffProbe(sessionId);
  return invoke("handoff_probe", { sessionId });
}

/** A conversation's files, once its transcript has stopped changing: call after the Session is killed. */
export function handoffConversationRead(conversation: ClaudeConversation): Promise<ConversationFiles> {
  if (!inTauri) return mock.handoffConversationRead(conversation);
  return invoke("handoff_conversation_read", { conversation });
}

/** Delete this Mac's copy of a conversation's transcript, once the Host has it. */
export function handoffConversationForget(conversation: ClaudeConversation): Promise<void> {
  if (!inTauri) return mock.handoffConversationForget(conversation);
  return invoke("handoff_conversation_forget", { conversation });
}

/** Real paths of the files in the drop just received (read off the macOS drag pasteboard). */
export function dropPaths(): Promise<string[]> {
  if (!inTauri) return Promise.resolve([]);
  return invoke("drop_paths");
}

/** Save a dropped file that has no path (e.g. a file promise) to a temp dir; resolves to its path. */
export async function saveDroppedFile(file: File): Promise<string> {
  if (!inTauri) return file.name;
  const bytes = new Uint8Array(await file.arrayBuffer());
  return invoke("drop_save", bytes, { headers: { "x-file-name": encodeURIComponent(file.name) } });
}

/** What earlier runs left running and was not yet resumed or dismissed: the Resume banner's rows. */
export function resumeLeftover(): Promise<ResumeEntry[]> {
  if (!inTauri) return mock.resumeLeftover();
  return invoke("resume_leftover");
}

/** Drop leftover Resume entries by key (resumed, dismissed, or their Tab is gone). */
export function resumeForget(keys: string[]): Promise<void> {
  if (!inTauri) return mock.resumeForget(keys);
  return invoke("resume_forget", { keys });
}

/** The persisted app settings blob (Hotkeys, Usage agents, the sidebar's presentation), or null on first run. Shape is owned by src/lib/settings/store.ts. */
export function loadSettings(): Promise<unknown | null> {
  if (!inTauri) return mock.loadSettings();
  return invoke("settings_load");
}

export function saveSettings(settings: unknown): Promise<void> {
  if (!inTauri) return mock.saveSettings(settings);
  return invoke("settings_save", { settings });
}
