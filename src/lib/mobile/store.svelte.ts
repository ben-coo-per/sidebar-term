// What the phone shows: paired or not, the Hosts it reaches and what each last said (its layout,
// Session facts and agent events, mirrored as the Mac's sidebar mirrors its Hosts'), which view
// is up (Manager or Tabs) and which Tab is open.
//
// The page's own Host (`LOCAL_HOST`: the origin the page was loaded from) gives the Groups, its
// own Tabs, and its linked Tabs with the Hosts they point at (ADR 0003). The phone reaches each
// of those Hosts itself, over a connection and a pairing of its own (ADR 0004): it stays in
// reach while the Mac sleeps, and each Host lists and can remove the phone. What the page's Host
// last said is kept, so the list is there before it answers, and when it does not.
//
// Tokens live in localStorage (an installed home-screen page keeps it indefinitely).

import { pairWithHost, RemoteClient, type ConnectionStatus } from "../host/client";
import { LOCAL_HOST, type HostId } from "../host/ids";
import { normalizeHostUrl } from "../host/settings";
import type { AgentEvent, HostInfo, LayoutSnapshot, LinkedHost, SessionId, SessionInfo } from "../types";
import { lanes as sortedLanes, type Lane, type Lanes } from "./lanes";
import { findRow, groupRows, unpairedHosts, type Facts, type GroupRows, type HostViews, type TabRow, type Unpaired } from "./rows";

const TOKEN_KEY = "sidebar-term:remote-token";
const NAME_KEY = "sidebar-term:remote-name";
/** Tokens for the Hosts linked Tabs point at, by the Host's URL. */
const HOST_TOKENS_KEY = "sidebar-term:host-tokens";
/** What the page's Host last said: its layout, facts, and the Hosts its linked Tabs point at. */
const LAST_HEARD_KEY = "sidebar-term:last-heard";
const VIEW_KEY = "sidebar-term:view";

/** Agent events kept per Host, as the Host keeps them. */
const EVENTS_PER_HOST = 500;
/** How often relative times redraw. */
const CLOCK_MS = 30_000;
/** How long a Sent row stays once its agent is back at work. */
const SENT_LINGER_MS = 4_000;

export type Phase = "loading" | "pair" | "connected";
export type View = "manager" | "tabs";

/** One Host as the phone holds it. */
export interface HostState {
  id: HostId;
  /** Its Remote server; the page's origin for the page's own Host. */
  url: string;
  /** From its `hello`, else as the page's Host named it; null when neither did. */
  name: string | null;
  /** The phone holds a token for it. */
  paired: boolean;
  status: ConnectionStatus;
  /** Why it is offline, when it said. */
  detail: string | null;
  info: HostInfo | null;
  /** Its layout, whole; an older revision than the one held is ignored. */
  layout: LayoutSnapshot | null;
  /** Its latest facts about each of its Sessions. */
  sessions: Facts;
  /** Its agent events, oldest first. */
  events: AgentEvent[];
}

/** An answer given from a card, shown in the card's place until its agent picks up again. */
export interface Sent {
  rowId: string;
  title: string;
  choice: string;
  pendingId: number;
}

export const mobile = $state<{
  phase: Phase;
  /** This phone's name as its Hosts know it. */
  device: string | null;
  hosts: Record<HostId, HostState>;
  view: View;
  /** The Tab whose Session is on screen, as it was when opened; null on the lists. */
  openTab: TabRow | null;
  /** The Host the pairing screen pairs with: another than the page's own, chosen from the list; null otherwise. */
  pairWith: HostId | null;
  /** Code from the QR link (`#pair=CODE`), prefilled on the pairing screen. */
  pairCode: string;
  pairError: string | null;
  pairing: boolean;
  sent: Sent[];
  /** The clock relative times are read against. */
  now: number;
}>({
  phase: "loading",
  device: null,
  hosts: {},
  view: "manager",
  openTab: null,
  pairWith: null,
  pairCode: "",
  pairError: null,
  pairing: false,
  sent: [],
  now: Date.now(),
});

const clients = new Map<HostId, RemoteClient>();
/** The Hosts the page's Host's linked Tabs point at, as it last said. */
let linked: LinkedHost[] = [];

/** The live connection to a Host, for the terminal screen; null while there is none. */
export function clientOf(host: HostId): RemoteClient | null {
  return clients.get(host) ?? null;
}

// --- localStorage ------------------------------------------------------------------------------

function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string | null): void {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* private mode: the pairing lasts this page */
  }
}

function readJson(key: string): unknown {
  const raw = readStorage(key);
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

function hostTokens(): Record<string, string> {
  const raw = readJson(HOST_TOKENS_KEY);
  const out: Record<string, string> = {};
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return out;
  for (const [url, token] of Object.entries(raw)) if (typeof token === "string" && token !== "") out[url] = token;
  return out;
}

function setHostToken(url: string, token: string | null): void {
  const tokens = hostTokens();
  if (token === null) delete tokens[url];
  else tokens[url] = token;
  writeStorage(HOST_TOKENS_KEY, Object.keys(tokens).length ? JSON.stringify(tokens) : null);
}

/** What the page's Host sent that says which Hosts there are: unknown JSON, from it or from storage. */
export function parseLinkedHosts(raw: unknown): LinkedHost[] {
  if (!Array.isArray(raw)) return [];
  const out: LinkedHost[] = [];
  const seen = new Set<string>([LOCAL_HOST]);
  for (const item of raw) {
    if (!item || typeof item !== "object") continue;
    const r = item as Record<string, unknown>;
    if (typeof r.id !== "string" || r.id === "" || seen.has(r.id)) continue;
    const url = typeof r.url === "string" ? normalizeHostUrl(r.url) : null;
    if (!url || url.startsWith("mock:")) continue;
    seen.add(r.id);
    out.push({ id: r.id, url, name: typeof r.name === "string" && r.name !== "" ? r.name : null });
  }
  return out;
}

interface LastHeard {
  info: HostInfo | null;
  layout: LayoutSnapshot | null;
  sessions: SessionInfo[];
  hosts: LinkedHost[];
}

function readLastHeard(): LastHeard | null {
  const raw = readJson(LAST_HEARD_KEY);
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Partial<LastHeard>;
  const layout = r.layout && Array.isArray(r.layout.groups) && r.layout.tabs && typeof r.layout.tabs === "object" ? r.layout : null;
  return {
    info: r.info && typeof r.info.name === "string" ? r.info : null,
    layout,
    sessions: Array.isArray(r.sessions) ? r.sessions.filter((s) => s && typeof s.sessionId === "number") : [],
    hosts: parseLinkedHosts(r.hosts),
  };
}

function keepLastHeard(): void {
  const home = mobile.hosts[LOCAL_HOST];
  if (!home?.layout) return;
  const heard: LastHeard = { info: home.info, layout: home.layout, sessions: Object.values(home.sessions), hosts: linked };
  writeStorage(LAST_HEARD_KEY, JSON.stringify(heard));
}

// --- Hosts -------------------------------------------------------------------------------------

function newHost(id: HostId, url: string, name: string | null, paired: boolean): HostState {
  return { id, url, name, paired, status: "offline", detail: null, info: null, layout: null, sessions: {}, events: [] };
}

/** Take a layout snapshot unless an older one arrives after a newer (the two channels do not order). */
function applyLayout(host: HostState, layout: LayoutSnapshot, fresh = false): void {
  if (!fresh && host.layout && layout.revision < host.layout.revision) return;
  host.layout = layout;
  // Facts of Sessions no Tab points at any more are stale.
  const live = new Set<SessionId>();
  for (const tab of Object.values(layout.tabs)) if (tab.sessionId !== null) live.add(tab.sessionId);
  for (const key of Object.keys(host.sessions)) {
    const id = Number(key) as SessionId;
    if (!live.has(id)) delete host.sessions[id];
  }
}

/** Open the connection to the Host `id` with `token` and mirror what it says. */
function connect(id: HostId, token: string): void {
  clients.get(id)?.close();
  const host = mobile.hosts[id];
  if (!host) return;
  const home = id === LOCAL_HOST;
  // Only the page's Host is asked for its linked Tabs: they are placed in its Groups.
  const c = new RemoteClient(host.url, token, home);
  clients.set(id, c);
  host.paired = true;
  c.on("status", (status, detail) => {
    host.status = status;
    host.detail = detail;
  });
  c.on("hello", (info, device, layout, sessions, agentEvents) => {
    host.info = info;
    host.name = info.name;
    if (home) {
      mobile.device = device;
      writeStorage(NAME_KEY, device);
    }
    // A fresh start on every hello: the Host may have restarted with new Session ids.
    host.sessions = {};
    for (const s of sessions) host.sessions[s.sessionId] = s;
    applyLayout(host, layout, true);
    host.events = (agentEvents ?? []).slice(-EVENTS_PER_HOST);
    if (home) keepLastHeard();
  });
  c.on("layout", (layout) => {
    applyLayout(host, layout);
    if (home) keepLastHeard();
  });
  c.on("hosts", (hosts) => {
    if (!home) return;
    setLinkedHosts(parseLinkedHosts(hosts));
    keepLastHeard();
  });
  c.on("session", (session) => {
    host.sessions[session.sessionId] = session;
  });
  c.on("agentEvent", (event) => {
    host.events.push(event);
    if (host.events.length > EVENTS_PER_HOST) host.events.splice(0, host.events.length - EVENTS_PER_HOST);
  });
  c.on("unauthorized", () => {
    if (home) {
      forgetEverything();
      mobile.pairError = "The Host no longer knows this phone. Pair it again.";
      return;
    }
    // Removed on that Host: its Tabs wait behind a pairing again.
    setHostToken(host.url, null);
    clients.get(id)?.close();
    clients.delete(id);
    host.paired = false;
    host.layout = null;
    host.sessions = {};
    host.events = [];
    if (mobile.openTab?.host === id) mobile.openTab = null;
  });
  c.connect();
}

/** The Hosts the page's Host's linked Tabs point at: reach each the phone is paired with, let go of those no longer named. */
function setLinkedHosts(hosts: LinkedHost[]): void {
  linked = hosts;
  const tokens = hostTokens();
  const named = new Set(hosts.map((h) => h.id));
  for (const id of Object.keys(mobile.hosts)) {
    if (id === LOCAL_HOST || named.has(id)) continue;
    clients.get(id)?.close();
    clients.delete(id);
    delete mobile.hosts[id];
    if (mobile.openTab?.host === id) mobile.openTab = null;
    if (mobile.pairWith === id) mobile.pairWith = null;
  }
  for (const h of hosts) {
    const token = tokens[h.url] ?? null;
    const held = mobile.hosts[h.id];
    if (held && held.url === h.url) {
      if (!held.info) held.name = h.name ?? held.name;
      if (token && !clients.has(h.id)) connect(h.id, token);
      continue;
    }
    // New, or now somewhere else.
    clients.get(h.id)?.close();
    clients.delete(h.id);
    mobile.hosts[h.id] = newHost(h.id, h.url, h.name, token !== null);
    if (token) connect(h.id, token);
  }
}

function connectHome(token: string): void {
  const home = newHost(LOCAL_HOST, location.origin, null, true);
  const heard = readLastHeard();
  if (heard) {
    home.info = heard.info;
    home.name = heard.info?.name ?? null;
    home.layout = heard.layout;
    for (const s of heard.sessions) home.sessions[s.sessionId] = s;
  }
  mobile.hosts = { [LOCAL_HOST]: home };
  mobile.phase = "connected";
  if (heard) setLinkedHosts(heard.hosts);
  connect(LOCAL_HOST, token);
}

function forgetEverything(): void {
  for (const c of clients.values()) c.close();
  clients.clear();
  linked = [];
  writeStorage(TOKEN_KEY, null);
  writeStorage(HOST_TOKENS_KEY, null);
  writeStorage(LAST_HEARD_KEY, null);
  mobile.hosts = {};
  mobile.openTab = null;
  mobile.pairWith = null;
  mobile.sent = [];
  mobile.phase = "pair";
}

/** `#pair=CODE` from the QR link, consumed. */
function codeFromHash(): string {
  const m = /#pair=([A-Za-z0-9]+)/.exec(location.hash);
  if (!m) return "";
  history.replaceState(null, "", location.pathname + location.search);
  return m[1];
}

/** Start: connect with the stored token, or show the pairing screen. Returns the stopper. */
export function initMobile(): () => void {
  mobile.pairCode = codeFromHash();
  const token = readStorage(TOKEN_KEY);
  mobile.device = readStorage(NAME_KEY);
  mobile.view = readStorage(VIEW_KEY) === "tabs" ? "tabs" : "manager";
  if (token) connectHome(token);
  else mobile.phase = "pair";

  // Back in the foreground: do not wait out a backoff.
  const onVisible = () => {
    if (document.visibilityState !== "visible") return;
    mobile.now = Date.now();
    for (const c of clients.values()) c.reconnectNow();
  };
  document.addEventListener("visibilitychange", onVisible);
  const clock = setInterval(() => (mobile.now = Date.now()), CLOCK_MS);
  return () => {
    document.removeEventListener("visibilitychange", onVisible);
    clearInterval(clock);
    for (const c of clients.values()) c.close();
    clients.clear();
  };
}

// --- Pairing -----------------------------------------------------------------------------------

function why(e: unknown, host: string): string {
  if (e instanceof TypeError) return `Could not reach ${host}: ${e.message}`;
  return e instanceof Error ? e.message : String(e);
}

/**
 * Present the pairing code to the Host on the pairing screen (the page's own, or the one chosen
 * from the list); on success, connect.
 */
export async function pair(code: string, name: string): Promise<void> {
  const other = mobile.pairWith ? mobile.hosts[mobile.pairWith] : null;
  mobile.pairing = true;
  mobile.pairError = null;
  try {
    if (other) {
      const paired = await pairWithHost(other.url, code, name);
      setHostToken(other.url, paired.token);
      mobile.pairWith = null;
      connect(other.id, paired.token);
    } else {
      const paired = await pairWithHost(location.origin, code, name);
      writeStorage(TOKEN_KEY, paired.token);
      mobile.pairCode = "";
      connectHome(paired.token);
    }
  } catch (e) {
    mobile.pairError = why(e, other ? (other.name ?? new URL(other.url).host) : "the Host");
  } finally {
    mobile.pairing = false;
  }
}

/** Show the pairing screen for a Host linked Tabs point at; null goes back to the list. */
export function pairWith(host: HostId | null): void {
  mobile.pairError = null;
  mobile.pairWith = host !== null && mobile.hosts[host] && host !== LOCAL_HOST ? host : null;
}

/** Forget every token and go back to pairing (each Host still lists the phone until removed there). */
export function unpair(): void {
  forgetEverything();
}

// --- What is on screen -------------------------------------------------------------------------

export function setView(view: View): void {
  mobile.view = view;
  writeStorage(VIEW_KEY, view);
}

/**
 * Open a Tab's Terminal. A question its agent is waiting on moves there, where the user is
 * about to look: its Host stops holding it.
 */
export function openTab(tab: TabRow): void {
  const pending = tab.info?.pending;
  if (pending && tab.sessionId !== null) {
    void clients
      .get(tab.host)
      ?.command({ t: "release", sessionId: tab.sessionId, pendingId: pending.id })
      .catch((e) => console.error("letting go of a question failed", e));
  }
  mobile.openTab = tab;
}

export function closeTerminal(): void {
  mobile.openTab = null;
}

/** The page's own Host, once connected or remembered. */
export function homeHost(): HostState | null {
  return mobile.hosts[LOCAL_HOST] ?? null;
}

/** Every Host as the rows read it. */
export function hostViews(): HostViews {
  const out: HostViews = {};
  for (const h of Object.values(mobile.hosts)) {
    out[h.id] = {
      id: h.id,
      name: h.info?.name ?? h.name ?? new URL(h.url).host,
      paired: h.paired,
      online: h.status === "online",
      home: h.info?.home ?? null,
      layout: h.layout,
      sessions: h.sessions,
      events: h.events,
    };
  }
  return out;
}

/** The Groups and their Tabs' rows, as the Hosts' layouts and facts stand. */
export function rows(): GroupRows[] {
  return groupRows(hostViews());
}

/** The Hosts whose Tabs wait behind a pairing. */
export function unpaired(): Unpaired[] {
  return unpairedHosts(hostViews());
}

/** The open Tab's row as its Host now describes it, or as it was when opened once it is gone. */
export function currentTab(): TabRow | null {
  const opened = mobile.openTab;
  if (!opened) return null;
  return findRow(hostViews(), opened.id) ?? opened;
}

// --- Manager -----------------------------------------------------------------------------------

/** Every Agent session in the phone's three lists; a question just answered gives way to its Sent row. */
export function lanes(): Lanes {
  const all = sortedLanes(rows(), hostViews(), mobile.now);
  const sent = (l: Lane) => mobile.sent.some((s) => s.rowId === l.row.id && (l.pending === null || l.pending.id === s.pendingId));
  return { ...all, waiting: all.waiting.filter((l) => !sent(l)) };
}

/**
 * Answer `lane`'s question with option `index` (0-based). The card gives way to a Sent row at
 * once. There is no undo: the answer has reached the agent.
 */
export async function answer(lane: Lane, index: number): Promise<void> {
  const { row, pending } = lane;
  if (!pending || row.sessionId === null || index < 0 || index >= pending.options.length) return;
  const sent: Sent = { rowId: row.id, title: row.title, choice: pending.options[index], pendingId: pending.id };
  mobile.sent = [...mobile.sent.filter((s) => s.rowId !== row.id), sent];
  try {
    const client = clients.get(row.host);
    if (!client) throw new Error("Not connected to the Host.");
    await client.command({ t: "answer", sessionId: row.sessionId, pendingId: pending.id, option: index });
  } catch (e) {
    // Not sent (answered elsewhere, or the agent moved on): the card comes back if it still waits.
    mobile.sent = mobile.sent.filter((s) => s.pendingId !== pending.id || s.rowId !== row.id);
    console.error("answering failed", e);
  }
}

/** Whether the question a Sent row answered is still the one its agent waits on. */
function stillWaiting(sent: Sent): boolean {
  const info = findRow(hostViews(), sent.rowId)?.info;
  return !!info && info.status === "needs-input" && (info.pending === null || info.pending.id === sent.pendingId);
}

$effect.root(() => {
  // A Sent row goes a moment after its agent leaves Needs input.
  const leaving = new Set<string>();
  $effect(() => {
    for (const s of mobile.sent) {
      const { rowId, pendingId } = s;
      const key = `${rowId}:${pendingId}`;
      if (leaving.has(key) || stillWaiting(s)) continue;
      leaving.add(key);
      setTimeout(() => {
        leaving.delete(key);
        mobile.sent = mobile.sent.filter((x) => x.rowId !== rowId || x.pendingId !== pendingId);
      }, SENT_LINGER_MS);
    }
  });
});
