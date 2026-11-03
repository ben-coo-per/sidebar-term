import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteClient, type ConnectionStatus } from "./client";

/** A WebSocket the test drives: what the client sent, and the Host's side by hand. */
class FakeSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSED = 3;
  static all: FakeSocket[] = [];
  readyState = FakeSocket.CONNECTING;
  binaryType = "blob";
  sent: { t: string }[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((ev: { data: unknown }) => void) | null = null;
  onclose: ((ev: { code: number }) => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(readonly url: string) {
    FakeSocket.all.push(this);
  }
  send(data: string) {
    this.sent.push(JSON.parse(data));
  }
  close() {
    this.readyState = FakeSocket.CLOSED;
  }
  /** The Host accepts and says hello. */
  open() {
    this.readyState = FakeSocket.OPEN;
    this.onopen?.();
    this.message({
      t: "hello",
      host: { name: "dell", version: "0", home: "/home/u" },
      device: "Mac app",
      layout: { groups: [], tabs: [] },
      sessions: [],
    });
  }
  message(msg: object) {
    this.onmessage?.({ data: JSON.stringify(msg) });
  }
  pings() {
    return this.sent.filter((m) => m.t === "ping").length;
  }
}

let statuses: ConnectionStatus[];
let slows: boolean[];

function connected(): { client: RemoteClient; ws: FakeSocket } {
  const client = new RemoteClient("https://dell.example.ts.net", "tok");
  client.on("status", (s) => statuses.push(s));
  client.on("slow", (s) => slows.push(s));
  client.connect();
  const ws = FakeSocket.all.at(-1)!;
  ws.open();
  return { client, ws };
}

beforeEach(() => {
  vi.useFakeTimers();
  FakeSocket.all = [];
  statuses = [];
  slows = [];
  vi.stubGlobal("WebSocket", FakeSocket);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("RemoteClient liveness", () => {
  it("keeps a connection the Host answers", () => {
    const { client, ws } = connected();
    for (let i = 0; i < 5; i++) {
      vi.advanceTimersByTime(5000);
      ws.message({ t: "pong" });
    }
    expect(ws.pings()).toBe(5);
    expect(statuses.at(-1)).toBe("online");
    client.close();
  });

  it("calls a Host that answers late slow, not offline, and well again once it answers", () => {
    const { client, ws } = connected();
    vi.advanceTimersByTime(5000); // ping
    vi.advanceTimersByTime(4000); // no pong yet
    expect(statuses.at(-1)).toBe("online");
    expect(slows).toEqual([true]);
    vi.advanceTimersByTime(10000);
    ws.message({ t: "pong" }); // 14 s late: busy, not gone
    expect(slows).toEqual([true, false]);
    expect(statuses.at(-1)).toBe("online");
    expect(FakeSocket.all).toHaveLength(1);
    client.close();
  });

  it("drops a connection that stops answering altogether, and connects again", () => {
    const { client, ws } = connected();
    vi.advanceTimersByTime(5000); // ping
    vi.advanceTimersByTime(19999);
    expect(statuses.at(-1)).toBe("online");
    vi.advanceTimersByTime(1); // 20 s without a word
    expect(statuses.at(-1)).toBe("offline");
    expect(ws.onclose).toBeNull();
    vi.advanceTimersByTime(1000); // backoff
    expect(FakeSocket.all).toHaveLength(2);
    client.close();
  });

  it("checks an open connection at once when asked to reconnect (the Mac woke)", () => {
    const { client, ws } = connected();
    // Asleep for a minute: no timer ran, and nothing was heard.
    vi.setSystemTime(Date.now() + 60_000);
    client.reconnectNow();
    expect(ws.pings()).toBe(1);
    expect(FakeSocket.all).toHaveLength(1);
    vi.advanceTimersByTime(4000);
    expect(statuses.at(-1)).toBe("offline");
    expect(slows).toEqual([]);
    client.close();
  });

  it("gives a Host just heard from its time when asked to reconnect (the window took focus)", () => {
    const { client, ws } = connected();
    client.reconnectNow();
    vi.advanceTimersByTime(4000);
    expect(statuses.at(-1)).toBe("online");
    expect(slows).toEqual([true]);
    ws.message({ t: "pong" });
    expect(slows).toEqual([true, false]);
    client.close();
  });

  it("passes on the Host's memory pressure, from hello and as it changes", () => {
    const client = new RemoteClient("https://dell.example.ts.net", "tok");
    const seen: (string | null)[] = [];
    client.on("pressure", (p) => seen.push(p?.level ?? null));
    client.connect();
    const ws = FakeSocket.all.at(-1)!;
    ws.open(); // a hello without pressure: a Host that does not say
    const pressure = { level: "throttled", mem: 9, high: 9, max: 10, swap: 2, swapMax: 2, some: 44, full: 42, top: [] };
    ws.message({ t: "pressure", pressure });
    expect(seen).toEqual([null, "throttled"]);
    client.close();
  });

  it("takes any message as an answer", () => {
    const { client, ws } = connected();
    client.reconnectNow();
    ws.message({ t: "activity", sessions: [] });
    vi.advanceTimersByTime(4000);
    expect(statuses.at(-1)).toBe("online");
    client.close();
  });
});
