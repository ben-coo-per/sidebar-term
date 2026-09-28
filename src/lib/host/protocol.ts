// The Host protocol between a client (the phone's page, later the Mac app for a remote Host)
// and a Host (src-tauri/core/src/remote/server.rs). See docs/architecture.md "Host protocol".
// Pure: message types, the binary output framing and the reply correlation, no sockets.
//
// The Host sends its layout (`LayoutSnapshot`) and each Session's facts (`SessionInfo`, Agent
// status included) whole; a client derives what a row shows from them (src/lib/mobile/rows.ts
// on the phone, src/lib/sessions.svelte.ts on the Mac).

import type { AgentEvent, HostInfo, LayoutSnapshot, LinkedHost, PathExists, SessionId, SessionInfo, ActivitySession, Tab, Group } from "../types";

// --- Messages -----------------------------------------------------------------------------------

/** A layout command's client-chosen correlation id, echoed by its reply. */
export type CommandId = number;

/** The commands a client sends, each answered by `ok` or `error` carrying its `id`. */
export type CommandMessage =
  /** `take`: the client's user is at it, and the Host sizes the pty though other clients show the Session (ADR 0007). */
  | { t: "resize"; id: CommandId; sessionId: SessionId; cols: number; rows: number; take?: boolean }
  | { t: "tab_new"; id: CommandId; groupId?: string; afterTabId?: string; cwd?: string; cols?: number; rows?: number }
  | { t: "tab_close"; id: CommandId; tabId: string }
  | { t: "tab_rename"; id: CommandId; tabId: string; title: string }
  | { t: "tab_move"; id: CommandId; tabId: string; groupId: string; index?: number }
  | { t: "tab_activate"; id: CommandId; tabId: string }
  | { t: "group_new"; id: CommandId; name?: string; tabId?: string }
  | { t: "group_rename"; id: CommandId; groupId: string; name: string }
  | { t: "group_move"; id: CommandId; groupId: string; index: number }
  | { t: "group_delete"; id: CommandId; groupId: string }
  | { t: "group_set_collapsed"; id: CommandId; groupId: string; collapsed: boolean }
  /** Whether an absolute path exists on the Host (Handoff asks before choosing where a Tab lands). */
  | { t: "path_exists"; id: CommandId; path: string }
  /** Answer the question a Session's agent is waiting on with option `option` (0-based). */
  | { t: "answer"; id: CommandId; sessionId: SessionId; pendingId: number; option: number }
  /** Stop holding that question: the agent asks it in its Terminal instead. */
  | { t: "release"; id: CommandId; sessionId: SessionId; pendingId: number };

/** A command as a client hands it to its connection, which puts the `id` on it. */
export type Command = CommandMessage extends infer M ? (M extends CommandMessage ? Omit<M, "id"> : never) : never;

/** What a command's `ok` carries in `result`: `tab_new` the Tab, `group_new` the Group, `path_exists` its answer, else nothing. */
export type CommandResult = Tab | Group | PathExists | undefined;

/** Text frames a client sends. */
export type ClientMessage =
  /** `links`: send this Host's linked Tabs too, and the Hosts they point at (the phone, which reaches those itself). */
  | { t: "auth"; token: string; links?: boolean }
  | { t: "attach"; sessionId: SessionId }
  | { t: "detach"; sessionId: SessionId }
  | { t: "input"; sessionId: SessionId; data: string }
  | { t: "ping" }
  | CommandMessage;

/** Text frames the Host sends. Output comes as binary frames (see `decodeOutputFrame`). */
export type ServerMessage =
  | { t: "hello"; host: HostInfo; device: string; layout: LayoutSnapshot; sessions: SessionInfo[]; agentEvents?: AgentEvent[]; hosts?: LinkedHost[] }
  /** The Hosts this one's linked Tabs point at changed (to a client that asked for `links`). */
  | { t: "hosts"; hosts: LinkedHost[] }
  /** An agent did something. */
  | { t: "agent_event"; event: AgentEvent }
  | { t: "layout"; layout: LayoutSnapshot }
  | { t: "session"; session: SessionInfo }
  | { t: "activity"; sessions: ActivitySession[] }
  | { t: "attached"; sessionId: SessionId; cols: number; rows: number }
  | { t: "resized"; sessionId: SessionId; cols: number; rows: number }
  | { t: "exit"; sessionId: SessionId }
  | { t: "ok"; id: CommandId; result?: CommandResult }
  | { t: "error"; id: CommandId; message: string }
  /** Not a reply: the message could not be read, or `input` failed. */
  | { t: "error"; id?: undefined; message: string }
  | { t: "pong" };

/** WebSocket close codes the Host uses. */
export const CLOSE_UNAUTHORIZED = 4401;
export const CLOSE_AUTH_TIMEOUT = 4408;
export const CLOSE_FELL_BEHIND = 4429;
export const CLOSE_GOING_AWAY = 1001;

/** `POST /api/upload` (multipart, `Authorization: Bearer <token>`) answers with the file's path on the Host. */
export const UPLOAD_PATH = "/api/upload";
export interface UploadResponse {
  path: string;
}

/**
 * `POST /api/conversation` (multipart, bearer): a Claude Code conversation handed off to the
 * Host. Text fields `cwd` (the checkout it resumes from there) and `sessionId`, a `transcript`
 * file part, and one `memory` file part per memory file (its file name is the path under
 * `memory/`). The Host places them under its own Claude config dir and answers with the
 * transcript's path there (`UploadResponse`).
 */
export const CONVERSATION_PATH = "/api/conversation";

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

/** Whether a message answers a command (carries an `id`). */
export function isReply(msg: ServerMessage): msg is Extract<ServerMessage, { id: CommandId }> {
  return (msg.t === "ok" || msg.t === "error") && typeof msg.id === "number";
}

/**
 * Pairs each command with its reply: `send` hands out the id to put on the wire, `settle`
 * resolves or rejects the matching promise, `fail` rejects everything pending (the connection
 * dropped). Pure: the caller does the sending.
 */
export class Replies {
  private next: CommandId = 1;
  private pending = new Map<CommandId, { resolve: (r: CommandResult) => void; reject: (e: Error) => void }>();

  /** A fresh id, and the promise its reply settles. */
  open(): { id: CommandId; reply: Promise<CommandResult> } {
    const id = this.next++;
    const reply = new Promise<CommandResult>((resolve, reject) => this.pending.set(id, { resolve, reject }));
    return { id, reply };
  }

  /** Settle the command `msg` answers; false when nothing was waiting for it. */
  settle(msg: Extract<ServerMessage, { id: CommandId }>): boolean {
    const p = this.pending.get(msg.id);
    if (!p) return false;
    this.pending.delete(msg.id);
    if (msg.t === "ok") p.resolve(msg.result);
    else p.reject(new Error(msg.message));
    return true;
  }

  /** Reject every command still waiting. */
  fail(reason: string): void {
    for (const p of this.pending.values()) p.reject(new Error(reason));
    this.pending.clear();
  }

  get size(): number {
    return this.pending.size;
  }
}

// --- Output frames -------------------------------------------------------------------------------

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
