// Reactive per-Session facts, for every Host's Sessions: the latest SessionInfo (with the Agent
// status the Host derived, src-tauri/core/src/status.rs), the Terminal's OSC title for the
// automatic Title, and this client's own markers (finished, highlight). Also computes the
// automatic Title (each Tab's `lastCwd` is its Host's to keep). The local Host's facts arrive on
// the `session-info` event; a paired Host's on its socket (src/lib/host/hosts.svelte.ts), through
// `applySessionInfo`. Keyed by `SessionKey` (src/lib/host/ids.ts): Session ids are per Host.
// See docs/architecture.md "Agent status", "Naming" and "Hosts".
//
// OWNER: sidebar agent.

import { inTauri, onSessionInfo } from "./ipc";
import { terminals } from "./terminal/manager";
import type { AgentStatus, SessionInfo } from "./types";
import { computeAutomaticTitle } from "./agentStatus";
import { recordStatus } from "./manager/history";
import { isLocal, LOCAL_HOST, sessionKey, type HostId, type SessionKey } from "./host/ids";
import { layout, setTabUnread, tabIdForSession, type Tab } from "./layout.svelte";

export interface SessionState {
  info: SessionInfo | null;
  /**
   * Latest OSC 0/2 title as this page's Terminal parsed it: the same bytes the Host reads, but
   * here the moment they arrive, for the automatic Title. "" once cleared or before the first.
   */
  title: string;
  /** Running / Needs input / Done from the Host, or null when this isn't an Agent session. */
  status: AgentStatus | null;
  /** An agent finished (exited back to the shell) while this Tab was in the background. */
  finished: boolean;
  /** Done/Needs-input arrived while backgrounded; stays highlighted until the Tab is activated. */
  highlight: boolean;
}

const sessions = $state<Record<SessionKey, SessionState>>({});

export function sessionState(key: SessionKey | null): SessionState | null {
  if (key === null) return null;
  return sessions[key] ?? null;
}

/** The facts of a Tab's Session, on whichever Host it lives. */
export function sessionOf(tab: Tab): SessionState | null {
  return tab.sessionId === null ? null : sessionState(sessionKey(tab.host, tab.sessionId));
}

function ensure(key: SessionKey): SessionState {
  let s = sessions[key];
  if (!s) {
    s = { info: null, title: "", status: null, finished: false, highlight: false };
    sessions[key] = s;
  }
  return s;
}

/** The Session's Tab is the one in view: Tabs mode, its Terminal showing (in Manager none is). */
function isActiveTabForSession(key: SessionKey): boolean {
  const tabId = tabIdForSession(key);
  return layout.mode === "tabs" && tabId !== null && tabId === layout.activeTabId;
}

// --- Wire up Session facts -----------------------------------------------------------------

/**
 * A Host's word on one of its Sessions: the local Host's on every `session-info` event, a paired
 * Host's on every `session` message (and all of them in `hello`). Sets this client's markers
 * from the change: an agent that finished, or a status that needs a look, while the Tab was not
 * in view.
 */
export function applySessionInfo(host: HostId, info: SessionInfo): void {
  const key = sessionKey(host, info.sessionId);
  const s = ensure(key);
  // A Host older than Manager sends no history, hooks or question: keep the history from what
  // this client sees (from when it connected), and read the rest as a screen-only agent's.
  if ((info as Partial<SessionInfo>).history === undefined) {
    info = { ...info, history: recordStatus(s.info?.history ?? [], info.status ?? null, Date.now()), hooked: info.hooked ?? false, pending: info.pending ?? null };
  }
  const previousAgent = s.info?.agent ?? null;
  const prevStatus = s.status;
  s.info = info;
  s.status = info.status;

  if (previousAgent && !info.agent) {
    // The agent's last title (Claude Code: conversation text) must not outlive it.
    s.title = "";
    if (!isActiveTabForSession(key)) s.finished = true;
  }
  if (!isActiveTabForSession(key) && (s.status === "done" || s.status === "needs-input") && s.status !== prevStatus) {
    s.highlight = true;
  }
}

/** Forget a Session's facts (it ended, or its Host was removed). */
export function forgetSession(key: SessionKey): void {
  delete sessions[key];
}

void onSessionInfo((info) => applySessionInfo(LOCAL_HOST, info));

terminals.on("title", (key, title) => {
  ensure(key).title = title;
});

terminals.on("exit", (key) => {
  forgetSession(key);
});

// Clear the sticky "finished"/"highlight" markers the moment a Tab is (re)activated, or shows
// again as the window leaves Manager.
$effect.root(() => {
  $effect(() => {
    if (layout.mode !== "tabs") return;
    const tab = layout.activeTabId ? layout.tabs[layout.activeTabId] : null;
    if (!tab || tab.sessionId === null) return;
    const s = sessions[sessionKey(tab.host, tab.sessionId)];
    if (s) {
      s.finished = false;
      s.highlight = false;
    }
  });
});

// --- Unread --------------------------------------------------------------------------------

/**
 * Whether a Tab reads as unread: the user marked it, or its agent finished, stopped or asked for
 * input while the Tab was in the background. The same rules on every Host.
 */
export function tabIsUnread(tab: Tab): boolean {
  const s = sessionOf(tab);
  return tab.unread || (s?.finished ?? false) || (s?.highlight ?? false);
}

/** Mark a Tab unread, or read: read also clears its agent's "finished" and highlight markers. */
export function setTabRead(tab: Tab, read: boolean): void {
  setTabUnread(tab.id, !read);
  const s = read ? sessionOf(tab) : null;
  if (s) {
    s.finished = false;
    s.highlight = false;
  }
}

// --- Home directories, for the `~` special case in the automatic Title ---------------------

/** Each Host's home: this Mac's read at startup, a paired Host's from its `hello`. */
const homes = new Map<HostId, string | null>();

async function initHome(): Promise<void> {
  if (inTauri) {
    try {
      const { homeDir: getHomeDir } = await import("@tauri-apps/api/path");
      homes.set(LOCAL_HOST, await getHomeDir());
    } catch {
      homes.set(LOCAL_HOST, null);
    }
  } else {
    homes.set(LOCAL_HOST, "/Users/you"); // matches src/lib/mock.ts HOME
  }
}
void initHome();

/** A Host's home directory, when known (for the `~` in a name). */
export function hostHome(host: HostId): string | null {
  return homes.get(host) ?? null;
}

export function setHostHome(host: HostId, home: string | null): void {
  if (!isLocal(host)) homes.set(host, home);
}

/** The Title shown on a Tab: a user rename if set, else the automatic Title. */
export function tabTitle(tab: Tab): string {
  if (tab.customTitle) return tab.customTitle;
  const s = sessionOf(tab);
  return computeAutomaticTitle({
    agent: s?.info?.agent ?? null,
    oscTitle: s?.title ?? null,
    foreground: s?.info?.foreground ?? null,
    shellIsForeground: s?.info?.shellIsForeground ?? true,
    cwd: s?.info?.cwd ?? tab.lastCwd,
    home: homes.get(tab.host) ?? null,
  });
}
