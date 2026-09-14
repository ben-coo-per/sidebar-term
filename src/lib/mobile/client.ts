// The phone's connection to the Mac: one WebSocket to /ws, authenticated with the pairing token
// as its first message, reconnecting with backoff when it drops and re-attaching what was
// attached. Protocol: ./protocol.ts (mirrors src-tauri/src/remote/server.rs). No DOM beyond
// WebSocket; the store (./store.svelte.ts) owns what is shown.

import type { SessionId } from "../types";
import {
  CLOSE_GOING_AWAY,
  CLOSE_UNAUTHORIZED,
  decodeOutputFrame,
  parseServerMessage,
  type ClientMessage,
  type SidebarSnapshot,
} from "./protocol";

export type ConnectionStatus = "connecting" | "online" | "offline";

export interface ClientEvents {
  /** `detail` says why we are offline, for the banner. */
  status: (status: ConnectionStatus, detail: string | null) => void;
  hello: (device: string, sidebar: SidebarSnapshot | null) => void;
  sidebar: (sidebar: SidebarSnapshot) => void;
  attached: (sessionId: SessionId, cols: number, rows: number) => void;
  resized: (sessionId: SessionId, cols: number, rows: number) => void;
  output: (sessionId: SessionId, bytes: Uint8Array) => void;
  exit: (sessionId: SessionId) => void;
  error: (message: string) => void;
  /** The Mac no longer knows our token: pair again. */
  unauthorized: () => void;
}

const BACKOFF_MS = [1000, 2000, 4000, 8000, 15000];

export class RemoteClient {
  private ws: WebSocket | null = null;
  private wanted = new Set<SessionId>();
  private attempts = 0;
  private retry: ReturnType<typeof setTimeout> | null = null;
  private closed = false;
  private listeners: { [K in keyof ClientEvents]: Set<ClientEvents[K]> } = {
    status: new Set(),
    hello: new Set(),
    sidebar: new Set(),
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
      if (ev.code === CLOSE_UNAUTHORIZED) {
        this.emit("status", "offline", "This phone is no longer paired.");
        this.emit("unauthorized");
        return;
      }
      if (this.closed) return;
      const detail = ev.code === CLOSE_GOING_AWAY ? "Remote was turned off on the Mac." : null;
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
  }

  private send(msg: ClientMessage): boolean {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return false;
    this.ws.send(JSON.stringify(msg));
    return true;
  }

  /** Attach to a Session; the Mac replies `attached` then replays its recent output. */
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

  private receive(data: unknown) {
    if (data instanceof ArrayBuffer) {
      const frame = decodeOutputFrame(data);
      if (frame) this.emit("output", frame.sessionId, frame.bytes);
      return;
    }
    if (typeof data !== "string") return;
    const msg = parseServerMessage(data);
    if (!msg) return;
    switch (msg.t) {
      case "hello":
        this.attempts = 0;
        this.emit("status", "online", null);
        this.emit("hello", msg.device, msg.sidebar);
        // Back after a drop: pick up where we were.
        for (const id of this.wanted) this.send({ t: "attach", sessionId: id });
        break;
      case "sidebar":
        this.emit("sidebar", msg.sidebar);
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
