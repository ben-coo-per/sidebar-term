// The Remote protocol between a phone and the Mac (src-tauri/src/remote/server.rs), and the
// sidebar snapshot the Mac webview publishes for phones (src/lib/remote/sidebar.ts). Pure:
// message types and the binary output framing, no sockets.

import type { AgentKind, GitInfo, SessionId } from "../types";
import type { AgentStatus } from "../agentStatus";

// --- The sidebar as a phone shows it ------------------------------------------------------------

/** A Tab as the phone lists it: everything the Mac's TabRow shows, already derived. */
export interface SidebarTab {
  id: string;
  /** null while the Tab has no Session (its shell failed to spawn): not attachable. */
  sessionId: SessionId | null;
  title: string;
  agent: AgentKind | null;
  status: AgentStatus | null;
  /** An agent finished while the Tab was in the background on the Mac. */
  finished: boolean;
  /** The Badge: repo, worktree and branch (the Mac's `GitInfo`, whole, so Badge.svelte renders it). */
  git: GitInfo | null;
  remote: boolean;
}

export interface SidebarGroup {
  id: string;
  name: string;
  tabs: SidebarTab[];
}

export interface SidebarSnapshot {
  groups: SidebarGroup[];
  /** The Tab in view on the Mac. */
  activeTabId: string | null;
}

// --- Messages -----------------------------------------------------------------------------------

/** Text frames the phone sends. */
export type ClientMessage =
  | { t: "auth"; token: string }
  | { t: "attach"; sessionId: SessionId }
  | { t: "detach"; sessionId: SessionId }
  | { t: "input"; sessionId: SessionId; data: string }
  | { t: "ping" };

/** Text frames the Mac sends. Output comes as binary frames (see `decodeOutputFrame`). */
export type ServerMessage =
  | { t: "hello"; device: string; sidebar: SidebarSnapshot | null }
  | { t: "sidebar"; sidebar: SidebarSnapshot }
  | { t: "attached"; sessionId: SessionId; cols: number; rows: number }
  | { t: "resized"; sessionId: SessionId; cols: number; rows: number }
  | { t: "exit"; sessionId: SessionId }
  | { t: "error"; message: string }
  | { t: "pong" };

/** WebSocket close codes the Mac uses. */
export const CLOSE_UNAUTHORIZED = 4401;
export const CLOSE_AUTH_TIMEOUT = 4408;
export const CLOSE_FELL_BEHIND = 4429;
export const CLOSE_GOING_AWAY = 1001;

/** Parse a text frame; null when it is not a message we know. */
export function parseServerMessage(text: string): ServerMessage | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== "object" || typeof (parsed as { t?: unknown }).t !== "string") return null;
  return parsed as ServerMessage;
}

/** A binary frame: a big-endian u32 Session id, then the output bytes. */
export function encodeOutputFrame(sessionId: SessionId, bytes: Uint8Array): Uint8Array {
  const frame = new Uint8Array(4 + bytes.length);
  new DataView(frame.buffer).setUint32(0, sessionId);
  frame.set(bytes, 4);
  return frame;
}

export function decodeOutputFrame(frame: ArrayBufferLike | Uint8Array): { sessionId: SessionId; bytes: Uint8Array } | null {
  const view = frame instanceof Uint8Array ? frame : new Uint8Array(frame);
  if (view.length < 4) return null;
  const sessionId = new DataView(view.buffer, view.byteOffset, view.byteLength).getUint32(0);
  return { sessionId, bytes: view.subarray(4) };
}
