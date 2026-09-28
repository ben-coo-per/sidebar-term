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
import { agentLabel, computeAutomaticTitle, type AgentLabel } from "./agentStatus";
import { feed } from "./manager/feed.svelte";
import { lastPrompt } from "./manager/model";
import { recordStatus } from "./manager/history";
import { isLocal, LOCAL_HOST, sessionKey, type HostId, type SessionKey } from "./host/ids";
import { setTabUnread, tabIdForSession, tabInView, type Tab } from "./layout.svelte";

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

/** The Session's Tab is the one in view, its Terminal showing: the active Tab, or Manager's. */
function isActiveTabForSession(key: SessionKey): boolean {
  const tabId = tabIdForSession(key);
  return tabId !== null && tabId === tabInView()?.id;
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

// Clear the sticky "finished"/"highlight" markers the moment a Tab comes into view: activated,
// selected in Manager, or showing again as the window changes mode.
$effect.root(() => {
  $effect(() => {
    const tab = tabInView();
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

/** The prompt the Tab's agent was last given, from its Host's agent events (a hooked agent's). */
function lastPromptOf(tab: Tab): string | null {
  if (tab.sessionId === null) return null;
  return lastPrompt((feed.byHost[tab.host] ?? []).filter((e) => e.sessionId === tab.sessionId));
}

/**
 * What a Tab's agent is called (`agentLabel`): its project and a few words on what it is at; the
 * Tab's rename, with nothing added, once it has one. Null while the Tab runs no agent.
 */
export function tabAgentLabel(tab: Tab): AgentLabel | null {
  const s = sessionOf(tab);
  const agent = s?.info?.agent ?? null;
  if (!agent) return null;
  const remote = s?.info?.remote ?? false;
  return agentLabel({
    customTitle: tab.customTitle,
    git: remote ? null : (s?.info?.git ?? null),
    cwd: remote ? null : (s?.info?.cwd ?? tab.lastCwd),
    home: homes.get(tab.host) ?? null,
    oscTitle: s?.title || s?.info?.title || null,
    agent,
    lastPrompt: lastPromptOf(tab),
  });
}

/** The Title shown on a Tab: a user rename if set, else the automatic Title. */
export function tabTitle(tab: Tab): string {
  if (tab.customTitle) return tab.customTitle;
  const s = sessionOf(tab);
  const remote = s?.info?.remote ?? false;
  return computeAutomaticTitle({
    agent: s?.info?.agent ?? null,
    git: remote ? null : (s?.info?.git ?? null),
    lastPrompt: lastPromptOf(tab),
    oscTitle: s?.title || s?.info?.title || null,
    foreground: s?.info?.foreground ?? null,
    shellIsForeground: s?.info?.shellIsForeground ?? true,
    cwd: s?.info?.cwd ?? tab.lastCwd,
    home: homes.get(tab.host) ?? null,
  });
}
