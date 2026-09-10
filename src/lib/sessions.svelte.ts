// Reactive per-Session facts: the latest SessionInfo, the Terminal's OSC title/bell/activity,
// and the derived Agent status (see src/lib/agentStatus.ts for the pure rules). Also keeps each
// Tab's `lastCwd` in step and computes the automatic Title. See docs/architecture.md
// "Agent status" and "Naming".
//
// OWNER: sidebar agent.

import { inTauri, onSessionInfo } from "./ipc";
import { terminals } from "./terminal/manager";
import type { SessionId, SessionInfo } from "./types";
import { computeAgentStatus, computeAutomaticTitle, type AgentStatus } from "./agentStatus";
import { layout, setTabLastCwd, tabIdForSession, type Tab } from "./layout.svelte";

export interface SessionState {
  info: SessionInfo | null;
  /** Latest OSC 0/2 title. "" once cleared, "" before the first one arrives. */
  title: string;
  lastActivityAt: number | null;
  lastBellAt: number | null;
  /** Live Running / Needs input / Done, or null when this isn't an Agent session. */
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
    s = { info: null, title: "", lastActivityAt: null, lastBellAt: null, status: null, finished: false, highlight: false };
    sessions[id] = s;
  }
  return s;
}

function isActiveTabForSession(sessionId: SessionId): boolean {
  const tabId = tabIdForSession(sessionId);
  return tabId !== null && tabId === layout.activeTabId;
}

function recompute(id: SessionId): void {
  const s = sessions[id];
  if (!s) return;
  const agent = s.info?.agent ?? null;
  const prevStatus = s.status;
  s.status = computeAgentStatus({
    agent,
    title: s.title,
    now: Date.now(),
    lastActivityAt: s.lastActivityAt,
    lastBellAt: s.lastBellAt,
  });
  if (
    !isActiveTabForSession(id) &&
    (s.status === "done" || s.status === "needs-input") &&
    s.status !== prevStatus
  ) {
    s.highlight = true;
  }
}

// --- Wire up Session facts -----------------------------------------------------------------

void onSessionInfo((info) => {
  const s = ensure(info.sessionId);
  const previousAgent = s.info?.agent ?? null;
  s.info = info;

  if (!info.remote && info.cwd) {
    const tabId = tabIdForSession(info.sessionId);
    if (tabId) setTabLastCwd(tabId, info.cwd);
  }

  if (previousAgent && !info.agent) {
    // The agent's last title (Claude Code: conversation text) must not outlive it.
    s.title = "";
    if (!isActiveTabForSession(info.sessionId)) s.finished = true;
  }
  recompute(info.sessionId);
});

terminals.on("title", (sessionId, title) => {
  ensure(sessionId).title = title;
  recompute(sessionId);
});

terminals.on("bell", (sessionId) => {
  ensure(sessionId).lastBellAt = Date.now();
  recompute(sessionId);
});

terminals.on("activity", (sessionId) => {
  ensure(sessionId).lastActivityAt = Date.now();
  recompute(sessionId);
});

terminals.on("exit", (sessionId) => {
  delete sessions[sessionId];
});

// Claude Code's "Running" status expires purely on elapsed time (no new event fires it), so
// re-derive live statuses on a ~1s tick to catch that transition promptly.
if (typeof window !== "undefined") {
  setInterval(() => {
    for (const key of Object.keys(sessions)) recompute(Number(key) as SessionId);
  }, 1000);
}

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
