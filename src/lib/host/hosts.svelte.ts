// The paired Hosts, as the Mac app drives them: one connection per Host (./client.ts, the Host
// protocol over its socket, reconnecting with backoff), its connection state for its Tabs' chips
// and the Settings page, its name and home from `hello`, each Session's CPU and
// memory from its `activity` messages, and the token that lets this Mac in (settings.json,
// section `hosts`: ./settings.ts). What a Host says about its layout fills in its linked Tabs in
// this Mac's layout (src/lib/layout.svelte.ts `applyHostSnapshot`, ADR 0003); its Sessions' facts
// go into the same mirror as the local Host's (src/lib/sessions.svelte.ts `applySessionInfo`); its Sessions' Terminals are the Terminal
// manager's, reached through the transport made here. See docs/architecture.md "Hosts".

import { loadSection, saveSection } from "../settings/store";
import type { ActivitySession, ConversationFiles, HostInfo, PathExists, SessionId } from "../types";
import type { SessionTransport, TerminalSink } from "../terminal/manager";
import { applyHostSnapshot, dropHost, dropUnknownHosts, type LayoutCommands } from "../layout.svelte";
import { applySessionInfo, forgetSession, setHostHome } from "../sessions.svelte";
import type { ConnectionStatus, HostClient } from "./client";
import { openHostClient, pairHost } from "./connect";
import { sessionKey, type HostId } from "./ids";
import { cleanPath, newHostId, normalizeHostUrl, parseHostsSection, type PairedHost } from "./settings";

/** The name a Host records for this Mac at pairing (its list of paired clients). */
const DEVICE_NAME = "Mac app";
const HOSTS_SECTION = "hosts";

export interface HostState extends PairedHost {
  status: ConnectionStatus;
  /** Why the Host is offline, when it said (Remote turned off there, the token refused). */
  detail: string | null;
  /** False once the Host refused the token: it must be removed and paired again. */
  paired: boolean;
  /** From the Host's last `hello`; null before the first. */
  info: HostInfo | null;
  /** This Mac's name as the Host knows it. */
  device: string | null;
  /** Each Session's CPU and memory, from the Host's last `activity` message. */
  activity: Record<SessionId, ActivitySession>;
}

export const hosts = $state<{ list: HostState[]; ready: boolean }>({ list: [], ready: false });

interface Connection {
  client: HostClient;
  /** The Terminal attached to each Session, for its output, replays and exit. */
  sinks: Map<SessionId, TerminalSink>;
  offs: (() => void)[];
}

const connections = new Map<HostId, Connection>();

export function hostState(id: HostId): HostState | null {
  return hosts.list.find((h) => h.id === id) ?? null;
}

/** A paired Host's name for the sidebar: from `hello`, else as last heard, else its URL's host. */
export function hostName(id: HostId): string {
  const h = hostState(id);
  if (!h) return id;
  return h.info?.name ?? h.name ?? new URL(h.url).host;
}

/** A Session's CPU and memory on its Host, if the Host samples Activity. */
export function hostActivity(host: HostId, sessionId: SessionId | null): ActivitySession | undefined {
  if (sessionId === null) return undefined;
  return hostState(host)?.activity[sessionId];
}

function persist(): void {
  saveSection(
    HOSTS_SECTION,
    hosts.list.map(
      ({ id, url, token, name, checkoutRoot, repoPaths }): PairedHost => ({ id, url, token, name, checkoutRoot, repoPaths: { ...repoPaths } }),
    ),
  );
}

/** Set a Host's Checkout root (Settings): where its repos are, `<root>/<repo>`. Empty clears it. */
export function setHostCheckoutRoot(id: HostId, root: string): void {
  const h = hostState(id);
  if (!h) return;
  h.checkoutRoot = cleanPath(root);
  persist();
}

/** Set (or, with an empty path, drop) where one repo is on a Host, overriding its Checkout root. */
export function setHostRepoPath(id: HostId, repo: string, path: string): void {
  const h = hostState(id);
  const name = repo.trim();
  if (!h || name === "") return;
  const clean = cleanPath(path);
  if (clean) h.repoPaths = { ...h.repoPaths, [name]: clean };
  else {
    const { [name]: _dropped, ...rest } = h.repoPaths;
    h.repoPaths = rest;
  }
  persist();
}

function connect(state: HostState): void {
  disconnect(state.id);
  const client = openHostClient(state.url, state.token);
  const conn: Connection = { client, sinks: new Map(), offs: [] };
  connections.set(state.id, conn);
  const id = state.id;
  conn.offs.push(
    client.on("status", (status, detail) => {
      state.status = status;
      state.detail = detail;
    }),
    client.on("hello", (info, device, layout, sessions) => {
      state.info = info;
      state.device = device;
      if (state.name !== info.name) {
        state.name = info.name;
        persist();
      }
      setHostHome(id, info.home);
      // The Host may have restarted since: Sessions it no longer has are over for their Terminals.
      const live = new Set<SessionId>(sessions.map((s) => s.sessionId));
      for (const t of Object.values(layout.tabs)) if (t.sessionId !== null) live.add(t.sessionId);
      for (const [sid, sink] of conn.sinks) {
        if (!live.has(sid)) {
          conn.sinks.delete(sid);
          sink.exit(null);
        }
      }
      state.activity = {};
      applyHostSnapshot(id, layout, { fresh: true });
      for (const s of sessions) applySessionInfo(id, s);
    }),
    client.on("layout", (layout) => applyHostSnapshot(id, layout)),
    client.on("session", (session) => applySessionInfo(id, session)),
    client.on("activity", (sessions) => {
      const next: Record<SessionId, ActivitySession> = {};
      for (const s of sessions) next[s.sessionId] = s;
      state.activity = next;
    }),
    client.on("attached", (sid, cols, rows) => conn.sinks.get(sid)?.replay(cols, rows)),
    client.on("output", (sid, bytes) => conn.sinks.get(sid)?.data(bytes)),
    client.on("exit", (sid) => {
      const sink = conn.sinks.get(sid);
      conn.sinks.delete(sid);
      sink?.exit(null);
      forgetSession(sessionKey(id, sid));
    }),
    client.on("error", (message) => console.warn(`host ${hostName(id)}:`, message)),
    client.on("unauthorized", () => {
      state.paired = false;
    }),
  );
  client.connect();
}

function disconnect(id: HostId): void {
  const conn = connections.get(id);
  if (!conn) return;
  connections.delete(id);
  for (const off of conn.offs) off();
  conn.client.close();
  for (const sink of conn.sinks.values()) sink.exit(null);
  conn.sinks.clear();
}

/**
 * Read the paired Hosts and connect to each; returns the function that stops. Call after the
 * local Host's layout is in: each Host's Tabs are linked into it.
 */
export function initHosts(): () => void {
  void loadSection(HOSTS_SECTION)
    .catch(() => undefined)
    .then((raw) => {
      for (const paired of parseHostsSection(raw)) {
        const state: HostState = {
          ...paired,
          status: "offline",
          detail: null,
          paired: true,
          info: null,
          device: null,
          activity: {},
        };
        hosts.list.push(state);
        connect(hosts.list[hosts.list.length - 1]);
      }
      dropUnknownHosts(hosts.list.map((h) => h.id));
      hosts.ready = true;
    });
  // Back in the foreground (the window was hidden, the Mac slept): do not wait out a backoff.
  const onVisible = () => {
    if (document.visibilityState === "visible") for (const c of connections.values()) c.client.reconnectNow();
  };
  document.addEventListener("visibilitychange", onVisible);
  return () => {
    document.removeEventListener("visibilitychange", onVisible);
    for (const id of [...connections.keys()]) disconnect(id);
  };
}

/**
 * Pair with the Host at `url` by presenting `code` (shown by its Settings page, or printed by
 * `sidebar-termd --pair`), keep its token, and connect. Rejects with why not: a bad URL, the
 * Host unreachable, a wrong or expired code.
 */
export async function addHost(url: string, code: string): Promise<void> {
  const base = normalizeHostUrl(url);
  if (!base) throw new Error("Enter the Host's address, such as https://dell.tail1234.ts.net or http://127.0.0.1:47611.");
  const cleaned = code.replace(/[^A-Za-z0-9]/g, "").toUpperCase();
  if (cleaned.length !== 8) throw new Error("The pairing code is 8 characters.");
  if (hosts.list.some((h) => h.url === base && h.paired)) throw new Error("This Host is already paired.");
  let paired;
  try {
    paired = await pairHost(base, cleaned, DEVICE_NAME);
  } catch (e) {
    if (e instanceof TypeError) throw new Error(`Could not reach ${base}: ${e.message}`);
    throw e;
  }
  // Pairing again with a Host that refused the old token replaces it.
  const stale = hosts.list.find((h) => h.url === base);
  if (stale) removeHost(stale.id);
  const state: HostState = {
    id: newHostId(),
    url: base,
    token: paired.token,
    name: null,
    checkoutRoot: null,
    repoPaths: {},
    status: "offline",
    detail: null,
    paired: true,
    info: null,
    device: paired.device,
    activity: {},
  };
  hosts.list.push(state);
  persist();
  connect(hosts.list[hosts.list.length - 1]);
}

/** Forget a Host: close its connection, drop its linked Tabs, and its token. The Host still lists this Mac until removed there. */
export function removeHost(id: HostId): void {
  disconnect(id);
  dropHost(id);
  hosts.list = hosts.list.filter((h) => h.id !== id);
  persist();
}

/** Try the Host now instead of waiting out its backoff. */
export function reconnectHost(id: HostId): void {
  connections.get(id)?.client.reconnectNow();
}

// --- What the layout and the Terminals need of a Host ------------------------------------------

function clientOf(id: HostId): HostClient | null {
  return connections.get(id)?.client ?? null;
}

const notConnected = () => Promise.reject(new Error("Not connected to the Host."));

/** The layout commands, over the Host's socket (the Host protocol's, one to one with the IPC's). */
export function hostCommands(id: HostId): LayoutCommands {
  const send = (msg: Parameters<HostClient["command"]>[0]) => clientOf(id)?.command(msg) ?? notConnected();
  return {
    tabNew: async (opts) => {
      const tab = await send({
        t: "tab_new",
        groupId: opts.groupId ?? undefined,
        afterTabId: opts.afterTabId ?? undefined,
        cwd: opts.cwd ?? undefined,
        cols: opts.cols,
        rows: opts.rows,
      });
      if (!tab || !("groupId" in tab)) throw new Error("The Host answered tab_new without the Tab.");
      return tab;
    },
    tabClose: async (tabId) => void (await send({ t: "tab_close", tabId })),
    tabRename: async (tabId, title) => void (await send({ t: "tab_rename", tabId, title })),
    tabMove: async (tabId, groupId, index) => void (await send({ t: "tab_move", tabId, groupId, index })),
    tabActivate: async (tabId) => void (await send({ t: "tab_activate", tabId })),
    groupNew: async (name, tabId) => {
      const group = await send({ t: "group_new", name: name ?? undefined, tabId: tabId ?? undefined });
      if (!group || !("tabIds" in group)) throw new Error("The Host answered group_new without the Group.");
      return group;
    },
    groupRename: async (groupId, name) => void (await send({ t: "group_rename", groupId, name })),
    groupMove: async (groupId, index) => void (await send({ t: "group_move", groupId, index })),
    groupDelete: async (groupId) => void (await send({ t: "group_delete", groupId })),
    groupSetCollapsed: async (groupId, collapsed) => void (await send({ t: "group_set_collapsed", groupId, collapsed })),
  };
}

// --- What Handoff needs of a Host (src/lib/handoff/handoff.svelte.ts) -------------------------

/** Whether an absolute path exists on the Host (`path_exists`). Rejects when not connected. */
export async function hostPathExists(id: HostId, path: string): Promise<PathExists> {
  const answer = await (clientOf(id)?.command({ t: "path_exists", path }) ?? notConnected());
  if (!answer || !("exists" in answer)) throw new Error("The Host answered path_exists without an answer.");
  return answer;
}

/** Type into a Session on the Host (what a Terminal would send). */
export function hostInput(id: HostId, sessionId: SessionId, data: string): void {
  const client = clientOf(id);
  if (!client) throw new Error("Not connected to the Host.");
  client.input(sessionId, data);
}

/** Hand a Claude Code conversation to the Host; resolves with the transcript's path there. */
export function hostPutConversation(id: HostId, cwd: string, sessionId: string, files: ConversationFiles): Promise<string> {
  return clientOf(id)?.putConversation(cwd, sessionId, files) ?? notConnected();
}

/**
 * A Host's Sessions for the Terminal manager: attach (the Host replays its recent output on
 * every attach, so the sink is told to clear first), input, resize (the Mac sends it; the Host
 * refuses while another client shows the Session, which the manager ignores), and dropped
 * files uploaded to the Host. No flow control: the Host's ring and drop-behind rules stand in.
 */
export function hostTransport(id: HostId): SessionTransport {
  return {
    async attach(sid, sink) {
      const conn = connections.get(id);
      if (!conn) throw new Error("Not connected to the Host.");
      conn.sinks.set(sid, sink);
      conn.client.attach(sid);
    },
    detach(sid) {
      const conn = connections.get(id);
      if (!conn) return;
      conn.sinks.delete(sid);
      conn.client.detach(sid);
    },
    async write(sid, data) {
      clientOf(id)?.input(sid, data);
    },
    async resize(sid, cols, rows) {
      await (clientOf(id)?.command({ t: "resize", sessionId: sid, cols, rows }) ?? notConnected());
    },
    async pause() {},
    async resume() {},
    async dropPaths(_sid, files) {
      const client = clientOf(id);
      if (!client) return [];
      const paths = await Promise.all(
        files.map((file) =>
          client.upload(file).catch((err) => {
            console.error(`host ${hostName(id)}: could not upload`, file.name, err);
            return null;
          }),
        ),
      );
      return paths.filter((p): p is string => !!p);
    },
    opensFiles: false,
  };
}
