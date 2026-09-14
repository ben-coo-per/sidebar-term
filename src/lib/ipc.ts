// Typed wrappers over the Tauri commands in src-tauri/src/lib.rs.
// CONTRACT: owned by the tech lead. Outside Tauri (plain `vite dev` in a browser) every call
// is routed to ./mock.ts so the UI can be developed without the Rust build.

import { invoke, Channel, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  EVENT_ACTIVITY,
  EVENT_CAFFEINATE,
  EVENT_MEMORY_GUARD,
  EVENT_MENU_SETTINGS,
  EVENT_SESSION_EXIT,
  EVENT_SESSION_INFO,
  EVENT_USAGE,
  type ActivitySnapshot,
  type AgentKind,
  type GuardSnapshot,
  type ResumeEntry,
  type SessionExit,
  type SessionId,
  type SessionInfo,
  type UsageSnapshot,
} from "./types";
import * as mock from "./mock";

export const inTauri: boolean = isTauri();

export interface SpawnOptions {
  cwd?: string | null;
  cols: number;
  rows: number;
  /** Key for this Session in Resume entries: its Tab id. A Session without one is never resumed. */
  resumeKey?: string | null;
  /** Raw pty output bytes, in order. Feed straight to `terminal.write(bytes)`. */
  onData: (bytes: Uint8Array) => void;
}

export async function spawnSession(opts: SpawnOptions): Promise<SessionId> {
  if (!inTauri) return mock.spawnSession(opts);
  const onData = new Channel<ArrayBuffer | number[]>();
  onData.onmessage = (msg) => {
    opts.onData(msg instanceof ArrayBuffer ? new Uint8Array(msg) : Uint8Array.from(msg));
  };
  return invoke<SessionId>("session_spawn", {
    cwd: opts.cwd ?? null,
    cols: opts.cols,
    rows: opts.rows,
    resumeKey: opts.resumeKey ?? null,
    onData,
  });
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

export function killSession(sessionId: SessionId): Promise<void> {
  if (!inTauri) return mock.killSession(sessionId);
  return invoke("session_kill", { sessionId });
}

/** Kill every Session. Called once at startup so a webview reload leaves no orphaned shells. */
export function resetSessions(): Promise<void> {
  if (!inTauri) return Promise.resolve();
  return invoke("session_reset");
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

/** Memory Guard froze or thawed a Tab, or was turned on or off. */
export function onGuard(cb: (snapshot: GuardSnapshot) => void): Promise<UnlistenFn> {
  if (!inTauri) return mock.onGuard(cb);
  return listen<GuardSnapshot>(EVENT_MEMORY_GUARD, (e) => cb(e.payload));
}

/**
 * Start (or change the agents of) or stop reading Usage. While on, `onUsage` fires right away and
 * then whenever a number changes. Reading Claude Code's usage calls api.anthropic.com every minute.
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

/** The persisted sidebar layout blob, or null on first run. Shape is owned by src/lib/layout. */
export function loadLayout(): Promise<unknown | null> {
  if (!inTauri) return mock.loadLayout();
  return invoke("layout_load");
}

export function saveLayout(layout: unknown): Promise<void> {
  if (!inTauri) return mock.saveLayout(layout);
  return invoke("layout_save", { layout });
}

/** The persisted app settings blob (Hotkeys, Usage agents), or null on first run. Shape is owned by src/lib/settings/store.ts. */
export function loadSettings(): Promise<unknown | null> {
  if (!inTauri) return mock.loadSettings();
  return invoke("settings_load");
}

export function saveSettings(settings: unknown): Promise<void> {
  if (!inTauri) return mock.saveSettings(settings);
  return invoke("settings_save", { settings });
}
