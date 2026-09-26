// Reactive per-Session facts: the latest SessionInfo (with the Agent status the Host derived,
// src-tauri/core/src/status.rs), the Terminal's OSC title for the automatic Title, and this
// client's own markers (finished, highlight). Also computes the automatic Title (each Tab's
// `lastCwd` is the Host's to keep). See docs/architecture.md "Agent status" and "Naming".
//
// OWNER: sidebar agent.

import { inTauri, onSessionInfo } from "./ipc";
import { terminals } from "./terminal/manager";
import type { AgentStatus, SessionId, SessionInfo } from "./types";
import { computeAutomaticTitle } from "./agentStatus";
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

const sessions = $state<Record<SessionId, SessionState>>({});

export function sessionState(sessionId: SessionId | null): SessionState | null {
  if (sessionId === null) return null;
  return sessions[sessionId] ?? null;
}

function ensure(id: SessionId): SessionState {
  let s = sessions[id];
  if (!s) {
    s = { info: null, title: "", status: null, finished: false, highlight: false };
    sessions[id] = s;
  }
  return s;
}

function isActiveTabForSession(sessionId: SessionId): boolean {
  const tabId = tabIdForSession(sessionId);
  return tabId !== null && tabId === layout.activeTabId;
}

// --- Wire up Session facts -----------------------------------------------------------------

void onSessionInfo((info) => {
  const s = ensure(info.sessionId);
  const previousAgent = s.info?.agent ?? null;
  const prevStatus = s.status;
  s.info = info;
  s.status = info.status;

  if (previousAgent && !info.agent) {
    // The agent's last title (Claude Code: conversation text) must not outlive it.
    s.title = "";
    if (!isActiveTabForSession(info.sessionId)) s.finished = true;
  }
  if (
    !isActiveTabForSession(info.sessionId) &&
    (s.status === "done" || s.status === "needs-input") &&
    s.status !== prevStatus
  ) {
    s.highlight = true;
  }
});

terminals.on("title", (sessionId, title) => {
  ensure(sessionId).title = title;
});

terminals.on("exit", (sessionId) => {
  delete sessions[sessionId];
});

// Clear the sticky "finished"/"highlight" markers the moment a Tab is (re)activated.
$effect.root(() => {
  $effect(() => {
    const tab = layout.activeTabId ? layout.tabs[layout.activeTabId] : null;
    const sessionId = tab?.sessionId ?? null;
    if (sessionId === null) return;
    const s = sessions[sessionId];
    if (s) {
      s.finished = false;
      s.highlight = false;
    }
  });
});

// --- Unread --------------------------------------------------------------------------------

/**
 * Whether a Tab reads as unread: the user marked it, or its agent finished, stopped or asked for
 * input while the Tab was in the background.
 */
export function tabIsUnread(tab: Tab): boolean {
  const s = tab.sessionId !== null ? sessions[tab.sessionId] : null;
  return tab.unread || (s?.finished ?? false) || (s?.highlight ?? false);
}

/** Mark a Tab unread, or read: read also clears its agent's "finished" and highlight markers. */
export function setTabRead(tab: Tab, read: boolean): void {
  setTabUnread(tab.id, !read);
  const s = read && tab.sessionId !== null ? sessions[tab.sessionId] : null;
  if (s) {
    s.finished = false;
    s.highlight = false;
  }
}

// --- Home directory, for the `~` special case in the automatic Title -----------------------

let homeDir: string | null = null;
async function initHome(): Promise<void> {
  if (inTauri) {
    try {
      const { homeDir: getHomeDir } = await import("@tauri-apps/api/path");
      homeDir = await getHomeDir();
    } catch {
      homeDir = null;
    }
  } else {
    homeDir = "/Users/you"; // matches src/lib/mock.ts HOME
  }
}
void initHome();

/** The Title shown on a Tab: a user rename if set, else the automatic Title. */
export function tabTitle(tab: Tab): string {
  if (tab.customTitle) return tab.customTitle;
  const s = tab.sessionId !== null ? sessions[tab.sessionId] : null;
  return computeAutomaticTitle({
    agent: s?.info?.agent ?? null,
    oscTitle: s?.title ?? null,
    foreground: s?.info?.foreground ?? null,
    shellIsForeground: s?.info?.shellIsForeground ?? true,
    cwd: s?.info?.cwd ?? tab.lastCwd,
    home: homeDir,
  });
}
