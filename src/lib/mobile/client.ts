// The phone's connection to a Host: one WebSocket to /ws, authenticated with the pairing token
// as its first message, reconnecting with backoff when it drops and re-attaching what was
// attached. Protocol: src/lib/host/protocol.ts (mirrors src-tauri/core/src/remote/server.rs).
// No DOM beyond WebSocket and fetch; the store (./store.svelte.ts) owns what is shown.

import type { ActivitySession, HostInfo, LayoutSnapshot, SessionId, SessionInfo } from "../types";
import {
  CLOSE_GOING_AWAY,
  CLOSE_UNAUTHORIZED,
  decodeOutputFrame,
  isReply,
  parseServerMessage,
  Replies,
  UPLOAD_PATH,
  type ClientMessage,
  type CommandMessage,
  type CommandResult,
  type UploadResponse,
} from "../host/protocol";

export type ConnectionStatus = "connecting" | "online" | "offline";

export interface ClientEvents {
  /** `detail` says why we are offline, for the banner. */
  status: (status: ConnectionStatus, detail: string | null) => void;
  hello: (host: HostInfo, device: string, layout: LayoutSnapshot, sessions: SessionInfo[]) => void;
  layout: (layout: LayoutSnapshot) => void;
  session: (session: SessionInfo) => void;
  activity: (sessions: ActivitySession[]) => void;
  attached: (sessionId: SessionId, cols: number, rows: number) => void;
  resized: (sessionId: SessionId, cols: number, rows: number) => void;
  output: (sessionId: SessionId, bytes: Uint8Array) => void;
  exit: (sessionId: SessionId) => void;
  /** An error that answers no command: a message the Host could not read, or `input` failed. */
  error: (message: string) => void;
  /** The Host no longer knows our token: pair again. */
  unauthorized: () => void;
}

const BACKOFF_MS = [1000, 2000, 4000, 8000, 15000];

export class RemoteClient {
  private ws: WebSocket | null = null;
  private wanted = new Set<SessionId>();
  private attempts = 0;
  private retry: ReturnType<typeof setTimeout> | null = null;
  private closed = false;
  private replies = new Replies();
  private listeners: { [K in keyof ClientEvents]: Set<ClientEvents[K]> } = {
    status: new Set(),
    hello: new Set(),
    layout: new Set(),
    session: new Set(),
    activity: new Set(),
    attached: new Set(),
    resized: new Set(),
    output: new Set(),
    exit: new Set(),
    error: new Set(),
    unauthorized: new Set(),
  };

  constructor(
    private readonly url: string,
    private readonly token: string,
  ) {}

  /** The WebSocket URL for the page we were loaded from. */
  static urlFor(location: { protocol: string; host: string }): string {
    return `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws`;
  }

  on<K extends keyof ClientEvents>(event: K, cb: ClientEvents[K]): () => void {
    this.listeners[event].add(cb);
    return () => void this.listeners[event].delete(cb);
  }

  private emit<K extends keyof ClientEvents>(event: K, ...args: Parameters<ClientEvents[K]>) {
    for (const cb of this.listeners[event]) {
      try {
        (cb as (...a: Parameters<ClientEvents[K]>) => void)(...args);
      } catch (err) {
        console.error(`[remote] "${event}" listener threw`, err);
      }
    }
  }

  connect(): void {
    if (this.closed || this.ws) return;
    if (this.retry !== null) {
      clearTimeout(this.retry);
      this.retry = null;
    }
    this.emit("status", "connecting", null);
    const ws = new WebSocket(this.url);
    ws.binaryType = "arraybuffer";
    this.ws = ws;
    ws.onopen = () => {
      this.send({ t: "auth", token: this.token });
    };
    ws.onmessage = (ev) => this.receive(ev.data);
    ws.onclose = (ev) => {
      if (this.ws !== ws) return;
      this.ws = null;
      this.replies.fail("The connection dropped.");
      if (ev.code === CLOSE_UNAUTHORIZED) {
        this.emit("status", "offline", "This phone is no longer paired.");
        this.emit("unauthorized");
        return;
      }
      if (this.closed) return;
      const detail = ev.code === CLOSE_GOING_AWAY ? "Remote was turned off on the Host." : null;
      this.emit("status", "offline", detail);
      this.scheduleReconnect();
    };
    ws.onerror = () => {
      /* onclose follows */
    };
  }

  /** Try now (the page came back to the foreground) instead of waiting out the backoff. */
  reconnectNow(): void {
    if (this.ws || this.closed) return;
    this.attempts = 0;
    this.connect();
  }

  private scheduleReconnect() {
    const delay = BACKOFF_MS[Math.min(this.attempts, BACKOFF_MS.length - 1)];
    this.attempts += 1;
    this.retry = setTimeout(() => {
      this.retry = null;
      this.connect();
    }, delay);
  }

  close(): void {
    this.closed = true;
    if (this.retry !== null) clearTimeout(this.retry);
    this.retry = null;
    const ws = this.ws;
    this.ws = null;
    ws?.close();
    this.replies.fail("The connection was closed.");
  }

  private send(msg: ClientMessage): boolean {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return false;
    this.ws.send(JSON.stringify(msg));
    return true;
  }

  /** Attach to a Session; the Host replies `attached` then replays its recent output. */
  attach(sessionId: SessionId): void {
    this.wanted.add(sessionId);
    this.send({ t: "attach", sessionId });
  }

  detach(sessionId: SessionId): void {
    this.wanted.delete(sessionId);
    this.send({ t: "detach", sessionId });
  }

  input(sessionId: SessionId, data: string): void {
    this.send({ t: "input", sessionId, data });
  }

  /**
   * A layout command (or `resize`), answered by the Host: resolves with the command's result
   * (`tab_new` the Tab, `group_new` the Group), rejects with the Host's message.
   */
  command(msg: Omit<CommandMessage, "id">): Promise<CommandResult> {
    const { id, reply } = this.replies.open();
    if (!this.send({ ...msg, id } as CommandMessage)) {
      this.replies.settle({ t: "error", id, message: "Not connected." });
    }
    return reply;
  }

  /** Upload a file to the Host; resolves with its path there, for attaching by path. */
  async upload(file: File): Promise<string> {
    const body = new FormData();
    body.append("file", file, file.name);
    const res = await fetch(UPLOAD_PATH, {
      method: "POST",
      headers: { authorization: `Bearer ${this.token}` },
      body,
    });
    const parsed = (await res.json().catch(() => ({}))) as Partial<UploadResponse> & { error?: string };
    if (!res.ok || !parsed.path) throw new Error(parsed.error ?? `Upload failed (${res.status}).`);
    return parsed.path;
  }

  private receive(data: unknown) {
    if (data instanceof ArrayBuffer) {
      const frame = decodeOutputFrame(data);
      if (frame) this.emit("output", frame.sessionId, frame.bytes);
      return;
    }
    if (typeof data !== "string") return;
    const msg = parseServerMessage(data);
    if (!msg) return;
    if (isReply(msg)) {
      this.replies.settle(msg);
      return;
    }
    switch (msg.t) {
      case "hello":
        this.attempts = 0;
        this.emit("status", "online", null);
        this.emit("hello", msg.host, msg.device, msg.layout, msg.sessions);
        // Back after a drop: pick up where we were.
        for (const id of this.wanted) this.send({ t: "attach", sessionId: id });
        break;
      case "layout":
        this.emit("layout", msg.layout);
        break;
      case "session":
        this.emit("session", msg.session);
        break;
      case "activity":
        this.emit("activity", msg.sessions);
        break;
      case "attached":
        this.emit("attached", msg.sessionId, msg.cols, msg.rows);
        break;
      case "resized":
        this.emit("resized", msg.sessionId, msg.cols, msg.rows);
        break;
      case "exit":
        this.wanted.delete(msg.sessionId);
        this.emit("exit", msg.sessionId);
        break;
      case "error":
        this.emit("error", msg.message);
        break;
      case "pong":
        break;
    }
  }
}
