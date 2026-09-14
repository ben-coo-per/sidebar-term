import { describe, expect, it } from "vitest";
import { buildSidebarSnapshot, type SidebarSource } from "./sidebar";
import type { SessionState } from "../sessions.svelte";
import type { SessionInfo } from "../types";

function info(sessionId: number, extra: Partial<SessionInfo> = {}): SessionInfo {
  return { sessionId, foreground: "zsh", shellIsForeground: true, agent: null, cwd: "/Users/you/Dev/jack", remote: false, git: null, ...extra };
}

function state(extra: Partial<SessionState>): SessionState {
  return { info: null, title: "", lastActivityAt: null, lastBellAt: null, status: null, finished: false, highlight: false, ...extra };
}

describe("buildSidebarSnapshot", () => {
  it("lists every Tab in Group order with what its row shows, dropping unknown ids", () => {
    const states: Record<number, SessionState> = {
      1: state({
        info: info(1, {
          agent: "claude",
          git: { repoName: "jack", commonDir: "/Users/you/Dev/jack/.git", worktreeRoot: "/Users/you/Dev/jack", worktreeName: null, branch: "main", headShort: null },
        }),
        status: "needs-input",
      }),
      2: state({ info: info(2, { remote: true }), finished: true }),
    };
    const src: SidebarSource = {
      groups: [
        { id: "g1", name: "Work", tabIds: ["t1", "gone", "t2"] },
        { id: "g2", name: "Empty", tabIds: [] },
      ],
      tabs: { t1: { id: "t1", sessionId: 1 }, t2: { id: "t2", sessionId: 2 }, t3: { id: "t3", sessionId: null } },
      activeTabId: "t2",
      titleOf: (id) => `title of ${id}`,
      stateOf: (sid) => (sid === null ? null : (states[sid] ?? null)),
    };
    expect(buildSidebarSnapshot(src)).toEqual({
      activeTabId: "t2",
      groups: [
        {
          id: "g1",
          name: "Work",
          tabs: [
            {
              id: "t1",
              sessionId: 1,
              title: "title of t1",
              agent: "claude",
              status: "needs-input",
              finished: false,
              git: { repoName: "jack", commonDir: "/Users/you/Dev/jack/.git", worktreeRoot: "/Users/you/Dev/jack", worktreeName: null, branch: "main", headShort: null },
              remote: false,
            },
            { id: "t2", sessionId: 2, title: "title of t2", agent: null, status: null, finished: true, git: null, remote: true },
          ],
        },
        { id: "g2", name: "Empty", tabs: [] },
      ],
    });
  });

  it("a Tab without a Session or facts still lists, session-less", () => {
    const snap = buildSidebarSnapshot({
      groups: [{ id: "g", name: "Tabs", tabIds: ["t"] }],
      tabs: { t: { id: "t", sessionId: null } },
      activeTabId: null,
      titleOf: () => "~",
      stateOf: () => null,
    });
    expect(snap.groups[0].tabs[0]).toEqual({ id: "t", sessionId: null, title: "~", agent: null, status: null, finished: false, git: null, remote: false });
  });
});
