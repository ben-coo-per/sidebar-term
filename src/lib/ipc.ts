// Typed wrappers over the Tauri commands in src-tauri/src/lib.rs.
// CONTRACT: owned by the tech lead. Outside Tauri (plain `vite dev` in a browser) every call
// is routed to ./mock.ts so the UI can be developed without the Rust build.

import { invoke, Channel, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  EVENT_ACTIVITY,
  EVENT_SESSION_EXIT,
  EVENT_SESSION_INFO,
  type ActivitySnapshot,
  type SessionExit,
  type SessionId,
  type SessionInfo,
} from "./types";
import * as mock from "./mock";

export const inTauri: boolean = isTauri();

export interface SpawnOptions {
  cwd?: string | null;
  cols: number;
  rows: number;
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

/** The persisted sidebar layout blob, or null on first run. Shape is owned by src/lib/layout. */
export function loadLayout(): Promise<unknown | null> {
  if (!inTauri) return mock.loadLayout();
  return invoke("layout_load");
}

export function saveLayout(layout: unknown): Promise<void> {
  if (!inTauri) return mock.saveLayout(layout);
  return invoke("layout_save", { layout });
}
