// What the phone shows: paired or not, connected or not, the Host's layout and Session facts
// (mirrored as the Mac's sidebar mirrors its local Host's), and which Tab is open. The token
// lives in localStorage (an installed home-screen page keeps it indefinitely).

import { pairWithHost, RemoteClient, type ConnectionStatus } from "../host/client";
import { findRow, groupRows, type Facts, type GroupRows, type TabRow } from "./rows";
import type { HostInfo, LayoutSnapshot, SessionId } from "../types";

const TOKEN_KEY = "sidebar-term:remote-token";
const NAME_KEY = "sidebar-term:remote-name";

export type Phase = "loading" | "pair" | "connected";

export const mobile = $state<{
  phase: Phase;
  status: ConnectionStatus;
  /** Why we are offline, when the Host said. */
  statusDetail: string | null;
  /** This phone's name as the Host knows it. */
  device: string | null;
  /** The Host we are connected to, from `hello`. */
  host: HostInfo | null;
  /** The Host's layout, whole; an older revision than the one shown is ignored. */
  layout: LayoutSnapshot | null;
  /** The Host's latest facts about each of its Sessions. */
  sessions: Facts;
  /** The Tab whose Session is on screen, as it was when opened; null on the list. */
  openTab: TabRow | null;
  /** Code from the QR link (`#pair=CODE`), prefilled on the pairing screen. */
  pairCode: string;
  pairError: string | null;
  pairing: boolean;
}>({
  phase: "loading",
  status: "offline",
  statusDetail: null,
  device: null,
  host: null,
  layout: null,
  sessions: {},
  openTab: null,
  pairCode: "",
  pairError: null,
  pairing: false,
});

let client: RemoteClient | null = null;

/** The live connection, for the terminal screen; null while unpaired. */
export function remoteClient(): RemoteClient | null {
  return client;
}

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
  if (token) connect(token);
  else mobile.phase = "pair";

  // Back in the foreground: do not wait out a backoff.
  const onVisible = () => {
    if (document.visibilityState === "visible") client?.reconnectNow();
  };
  document.addEventListener("visibilitychange", onVisible);
  return () => {
    document.removeEventListener("visibilitychange", onVisible);
    client?.close();
    client = null;
  };
}

/** Take a layout snapshot unless an older one arrives after a newer (the two channels do not order). */
function applyLayout(layout: LayoutSnapshot) {
  if (mobile.layout && layout.revision < mobile.layout.revision) return;
  mobile.layout = layout;
  // Facts of Sessions no Tab points at any more are stale.
  const live = new Set<SessionId>();
  for (const tab of Object.values(layout.tabs)) if (tab.sessionId !== null) live.add(tab.sessionId);
  for (const key of Object.keys(mobile.sessions)) {
    const id = Number(key) as SessionId;
    if (!live.has(id)) delete mobile.sessions[id];
  }
}

function connect(token: string) {
  client?.close();
  const c = new RemoteClient(location.origin, token);
  client = c;
  mobile.phase = "connected";
  c.on("status", (status, detail) => {
    mobile.status = status;
    mobile.statusDetail = detail;
  });
  c.on("hello", (host, device, layout, sessions) => {
    mobile.host = host;
    mobile.device = device;
    writeStorage(NAME_KEY, device);
    // A fresh start on every hello: the Host may have restarted with new Session ids.
    mobile.layout = null;
    mobile.sessions = {};
    for (const s of sessions) mobile.sessions[s.sessionId] = s;
    applyLayout(layout);
  });
  c.on("layout", applyLayout);
  c.on("session", (session) => {
    mobile.sessions[session.sessionId] = session;
  });
  c.on("unauthorized", () => {
    writeStorage(TOKEN_KEY, null);
    client = null;
    mobile.phase = "pair";
    mobile.pairError = "The Host no longer knows this phone. Pair it again.";
  });
  c.connect();
}

/** Present the pairing code to the Host; on success, connect. */
export async function pair(code: string, name: string): Promise<void> {
  mobile.pairing = true;
  mobile.pairError = null;
  try {
    const paired = await pairWithHost(location.origin, code, name);
    writeStorage(TOKEN_KEY, paired.token);
    mobile.pairCode = "";
    connect(paired.token);
  } catch (e) {
    mobile.pairError = e instanceof TypeError ? `Could not reach the Host: ${e.message}` : e instanceof Error ? e.message : String(e);
  } finally {
    mobile.pairing = false;
  }
}

/** Forget the token and go back to pairing (the Host still lists the phone until revoked there). */
export function unpair(): void {
  writeStorage(TOKEN_KEY, null);
  client?.close();
  client = null;
  mobile.openTab = null;
  mobile.layout = null;
  mobile.sessions = {};
  mobile.host = null;
  mobile.phase = "pair";
}

export function openTab(tab: TabRow): void {
  mobile.openTab = tab;
}

export function closeTerminal(): void {
  mobile.openTab = null;
}

/** The Groups and their Tabs' rows, as the Host's layout and facts stand. */
export function rows(): GroupRows[] {
  return mobile.layout ? groupRows(mobile.layout, mobile.sessions, mobile.host?.home ?? null) : [];
}

/** The open Tab's row as the Host now describes it, or as it was when opened once it is gone. */
export function currentTab(): TabRow | null {
  const opened = mobile.openTab;
  if (!opened) return null;
  return findRow(mobile.layout, mobile.sessions, mobile.host?.home ?? null, opened.id) ?? opened;
}
