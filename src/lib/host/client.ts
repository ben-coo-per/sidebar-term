// A client's connection to a Host: one WebSocket to the Host's `/ws`, authenticated with the
// pairing token as its first message, reconnecting with backoff when it drops and re-attaching
// what was attached. Protocol: ./protocol.ts (mirrors src-tauri/core/src/remote/server.rs).
// Both clients of a Host use it: the phone's page (src/lib/mobile/store.svelte.ts, against the
// origin it was loaded from) and the Mac app for each paired Host (./hosts.svelte.ts, against
// the Host's URL). No DOM beyond WebSocket and fetch; the stores own what is shown.

import type { ActivitySession, AgentEvent, ConversationFiles, HostInfo, LayoutSnapshot, SessionId, SessionInfo } from "../types";
import {
  CLOSE_GOING_AWAY,
  CLOSE_UNAUTHORIZED,
  CONVERSATION_PATH,
  decodeOutputFrame,
  isReply,
  parseServerMessage,
  Replies,
  UPLOAD_PATH,
  type ClientMessage,
  type Command,
  type CommandMessage,
  type CommandResult,
  type UploadResponse,
} from "./protocol";
import { webSocketUrl } from "./settings";

export type ConnectionStatus = "connecting" | "online" | "offline";

export interface ClientEvents {
  /** `detail` says why we are offline, when the Host said. */
  status: (status: ConnectionStatus, detail: string | null) => void;
  /** `agentEvents`: the Host's recent agent events, oldest first; null from a Host older than Manager, which keeps none. */
  hello: (host: HostInfo, device: string, layout: LayoutSnapshot, sessions: SessionInfo[], agentEvents: AgentEvent[] | null) => void;
  layout: (layout: LayoutSnapshot) => void;
  session: (session: SessionInfo) => void;
  activity: (sessions: ActivitySession[]) => void;
  /** An agent did something. */
  agentEvent: (event: AgentEvent) => void;
  /** Attached; a replay of the Session's recent output follows, at this grid. */
  attached: (sessionId: SessionId, cols: number, rows: number) => void;
  resized: (sessionId: SessionId, cols: number, rows: number) => void;
  output: (sessionId: SessionId, bytes: Uint8Array) => void;
  exit: (sessionId: SessionId) => void;
  /** An error that answers no command: a message the Host could not read, or `input` failed. */
  error: (message: string) => void;
  /** The Host no longer knows our token: pair again. */
  unauthorized: () => void;
}

/** What a store needs of a connection; `RemoteClient` is the real one, src/lib/mock.ts fakes one. */
export interface HostClient {
  on<K extends keyof ClientEvents>(event: K, cb: ClientEvents[K]): () => void;
  connect(): void;
  /** Try now instead of waiting out the backoff. */
  reconnectNow(): void;
  close(): void;
  attach(sessionId: SessionId): void;
  detach(sessionId: SessionId): void;
  input(sessionId: SessionId, data: string): void;
  /** A layout command or `resize`: resolves with the reply's result, rejects with the Host's message. */
  command(msg: Command): Promise<CommandResult>;
  /** Upload a file to the Host; resolves with its path there. */
  upload(file: File): Promise<string>;
  /**
   * Hand a Claude Code conversation to the Host, to resume from the checkout at `cwd` there;
   * resolves with the transcript's path on the Host (`POST /api/conversation`).
   */
  putConversation(cwd: string, sessionId: string, files: ConversationFiles): Promise<string>;
}

/** What `POST /api/pair` answers. */
export interface Paired {
  token: string;
  /** This client's name as the Host recorded it. */
  device: string;
}

/**
 * Present a pairing code to the Host at `base` (`https://dell.tail1234.ts.net`) with this
 * client's name; resolves with the token to connect with, rejects with the Host's reason.
 */
export async function pairWithHost(base: string, code: string, name: string): Promise<Paired> {
  const res = await fetch(`${base}/api/pair`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ code, name }),
  });
  const body = (await res.json().catch(() => ({}))) as Partial<Paired> & { error?: string };
  if (!res.ok || !body.token) throw new Error(body.error ?? `Pairing failed (${res.status}).`);
  return { token: body.token, device: body.device ?? name };
}

const BACKOFF_MS = [1000, 2000, 4000, 8000, 15000];

export class RemoteClient implements HostClient {
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
    agentEvent: new Set(),
    attached: new Set(),
    resized: new Set(),
    output: new Set(),
    exit: new Set(),
    error: new Set(),
    unauthorized: new Set(),
  };

  /** `base` is the Host's origin: `https://dell.tail1234.ts.net`, or `location.origin` on the phone. */
  constructor(
    private readonly base: string,
    private readonly token: string,
  ) {}

  on<K extends keyof ClientEvents>(event: K, cb: ClientEvents[K]): () => void {
    this.listeners[event].add(cb);
    return () => void this.listeners[event].delete(cb);
  }

  private emit<K extends keyof ClientEvents>(event: K, ...args: Parameters<ClientEvents[K]>) {
    for (const cb of this.listeners[event]) {
      try {
        (cb as (...a: Parameters<ClientEvents[K]>) => void)(...args);
      } catch (err) {
        console.error(`[host] "${event}" listener threw`, err);
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
    let ws: WebSocket;
    try {
      ws = new WebSocket(webSocketUrl(this.base));
    } catch (e) {
      // A URL the browser refuses outright (a bad scheme): nothing will ever connect.
      this.emit("status", "offline", e instanceof Error ? e.message : String(e));
      return;
    }
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
        this.emit("status", "offline", "No longer paired with this Host.");
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

  command(msg: Command): Promise<CommandResult> {
    const { id, reply } = this.replies.open();
    if (!this.send({ ...msg, id } as CommandMessage)) {
      this.replies.settle({ t: "error", id, message: "Not connected." });
    }
    return reply;
  }

  async upload(file: File): Promise<string> {
    const body = new FormData();
    body.append("file", file, file.name);
    return this.post(UPLOAD_PATH, body, "Upload");
  }

  async putConversation(cwd: string, sessionId: string, files: ConversationFiles): Promise<string> {
    const body = new FormData();
    body.append("cwd", cwd);
    body.append("sessionId", sessionId);
    body.append("transcript", new Blob([files.transcript], { type: "application/x-ndjson" }), `${sessionId}.jsonl`);
    for (const f of files.memory) body.append("memory", new Blob([f.content], { type: "text/plain" }), f.name);
    return this.post(CONVERSATION_PATH, body, "Sending the conversation");
  }

  /** A multipart POST with the token; resolves with the `path` the Host answers. */
  private async post(path: string, body: FormData, what: string): Promise<string> {
    const res = await fetch(`${this.base}${path}`, {
      method: "POST",
      headers: { authorization: `Bearer ${this.token}` },
      body,
    });
    const parsed = (await res.json().catch(() => ({}))) as Partial<UploadResponse> & { error?: string };
    if (!res.ok || !parsed.path) throw new Error(parsed.error ?? `${what} failed (${res.status}).`);
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
        this.emit("hello", msg.host, msg.device, msg.layout, msg.sessions, msg.agentEvents ?? null);
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
      case "agent_event":
        this.emit("agentEvent", msg.event);
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
