// What the phone shows: paired or not, connected or not, the sidebar, and which Tab is open.
// The token lives in localStorage (an installed home-screen page keeps it indefinitely).

import { RemoteClient, type ConnectionStatus } from "./client";
import type { SidebarSnapshot, SidebarTab } from "./protocol";

const TOKEN_KEY = "sidebar-term:remote-token";
const NAME_KEY = "sidebar-term:remote-name";

export type Phase = "loading" | "pair" | "connected";

export const mobile = $state<{
  phase: Phase;
  status: ConnectionStatus;
  /** Why we are offline, when the Mac said. */
  statusDetail: string | null;
  /** This phone's name as the Mac knows it. */
  device: string | null;
  sidebar: SidebarSnapshot | null;
  /** The Tab whose Session is on screen, as it was when opened; null on the list. */
  openTab: SidebarTab | null;
  /** Code from the QR link (`#pair=CODE`), prefilled on the pairing screen. */
  pairCode: string;
  pairError: string | null;
  pairing: boolean;
}>({
  phase: "loading",
  status: "offline",
  statusDetail: null,
  device: null,
  sidebar: null,
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

function connect(token: string) {
  client?.close();
  const c = new RemoteClient(RemoteClient.urlFor(location), token);
  client = c;
  mobile.phase = "connected";
  c.on("status", (status, detail) => {
    mobile.status = status;
    mobile.statusDetail = detail;
  });
  c.on("hello", (device, sidebar) => {
    mobile.device = device;
    writeStorage(NAME_KEY, device);
    if (sidebar) mobile.sidebar = sidebar;
  });
  c.on("sidebar", (sidebar) => (mobile.sidebar = sidebar));
  c.on("unauthorized", () => {
    writeStorage(TOKEN_KEY, null);
    client = null;
    mobile.phase = "pair";
    mobile.pairError = "The Mac no longer knows this phone. Pair it again.";
  });
  c.connect();
}

/** Present the pairing code to the Mac; on success, connect. */
export async function pair(code: string, name: string): Promise<void> {
  mobile.pairing = true;
  mobile.pairError = null;
  try {
    const res = await fetch("/api/pair", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ code, name }),
    });
    const body = (await res.json().catch(() => ({}))) as { token?: string; error?: string };
    if (!res.ok || !body.token) {
      mobile.pairError = body.error ?? `Pairing failed (${res.status}).`;
      return;
    }
    writeStorage(TOKEN_KEY, body.token);
    mobile.pairCode = "";
    connect(body.token);
  } catch (e) {
    mobile.pairError = `Could not reach the Mac: ${e instanceof Error ? e.message : String(e)}`;
  } finally {
    mobile.pairing = false;
  }
}

/** Forget the token and go back to pairing (the Mac still lists the phone until revoked there). */
export function unpair(): void {
  writeStorage(TOKEN_KEY, null);
  client?.close();
  client = null;
  mobile.openTab = null;
  mobile.sidebar = null;
  mobile.phase = "pair";
}

export function openTab(tab: SidebarTab): void {
  mobile.openTab = tab;
}

export function closeTerminal(): void {
  mobile.openTab = null;
}

/** The open Tab as the sidebar last described it, or as it was when opened once it is gone. */
export function currentTab(): SidebarTab | null {
  const opened = mobile.openTab;
  if (!opened) return null;
  return findTab(opened.id) ?? opened;
}

export function findTab(tabId: string): SidebarTab | null {
  if (!mobile.sidebar) return null;
  for (const g of mobile.sidebar.groups) {
    const tab = g.tabs.find((t) => t.id === tabId);
    if (tab) return tab;
  }
  return null;
}
