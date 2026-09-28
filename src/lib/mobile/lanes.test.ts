import { describe, expect, it } from "vitest";
import { lanes } from "./lanes";
import { groupRows, type HostView, type HostViews } from "./rows";
import { LOCAL_HOST } from "../host/ids";
import type { AgentEvent, LayoutSnapshot, Pending, SessionInfo, Tab } from "../types";

const NOW = 1_000_000_000;
const MIN = 60_000;

function info(sessionId: number, over: Partial<SessionInfo> = {}): SessionInfo {
  return {
    sessionId,
    foreground: "claude",
    shellIsForeground: false,
    agent: "claude",
    cwd: `/Users/you/Dev/repo${sessionId}`,
    remote: false,
    git: null,
    title: null,
    bells: 0,
    status: "running",
    history: [],
    hooked: true,
    pending: null,
    ...over,
  };
}

function tab(id: string, sessionId: number | null, over: Partial<Tab> = {}): Tab {
  return { id, groupId: "g", sessionId, customTitle: null, lastCwd: null, ...over };
}

function layoutOf(tabs: Tab[]): LayoutSnapshot {
  return {
    revision: 1,
    groups: [{ id: "g", name: "Work", collapsed: false, tabIds: tabs.map((t) => t.id) }],
    tabs: Object.fromEntries(tabs.map((t) => [t.id, t])),
    activeTabId: null,
  };
}

function host(id: string, name: string, over: Partial<HostView>): HostView {
  return { id, name, paired: true, online: true, home: "/Users/you", layout: null, sessions: {}, events: [], ...over };
}

const question: Pending = {
  id: 7,
  kind: "permission",
  text: "Run this command?",
  detail: [{ text: "cargo test", tone: "plain" }],
  options: ["Yes", "No"],
  since: NOW - 9 * MIN,
};

describe("lanes", () => {
  const macEvents: AgentEvent[] = [
    { at: NOW - 30 * MIN, sessionId: 2, kind: "started", text: "Started “Add the key bar”" },
    { at: NOW - 20 * MIN, sessionId: 2, kind: "edit", text: "Updated src/KeyBar.svelte (+40 −3)" },
    { at: NOW - 19 * MIN, sessionId: 2, kind: "command", text: "Ran pnpm check" },
    { at: NOW - 18 * MIN, sessionId: 2, kind: "asked", text: "Asked to run a command" },
    { at: NOW - 5 * MIN, sessionId: 4, kind: "edit", text: "Wrote notes.md (+12)" },
  ];
  const hosts: HostViews = {
    [LOCAL_HOST]: host(LOCAL_HOST, "bens-mac", {
      layout: layoutOf([
        tab("shell", 1),
        tab("working", 2),
        tab("asked", 3),
        tab("idle-old", 4),
        tab("on-dell", null, { link: { hostId: "h_dell", tabId: "r1" } }),
        tab("idle-new", 5),
      ]),
      sessions: {
        1: info(1, { agent: null, status: null, foreground: "zsh", shellIsForeground: true }),
        2: info(2, { status: "running", history: [{ status: "running", at: NOW - 30 * MIN }] }),
        3: info(3, { status: "needs-input", pending: question, history: [{ status: "needs-input", at: NOW - 9 * MIN }] }),
        4: info(4, { status: "done", history: [{ status: "running", at: NOW - 40 * MIN }, { status: "done", at: NOW - 25 * MIN }] }),
        5: info(5, { status: "done", history: [{ status: "done", at: NOW - 2 * MIN }] }),
      },
      events: macEvents,
    }),
    h_dell: host("h_dell", "dell", {
      layout: layoutOf([tab("r1", 2)]),
      sessions: {
        // Screen only, and waiting longer than the Mac's question: it comes first.
        2: info(2, { agent: "codex", status: "needs-input", hooked: false, history: [{ status: "needs-input", at: NOW - 15 * MIN }] }),
      },
    }),
  };
  const all = lanes(groupRows(hosts), hosts, NOW);

  it("lists the agents waiting on you across every Host, the oldest question first", () => {
    expect(all.waiting.map((l) => [l.row.id, l.row.hostName, l.hooked, l.since])).toEqual([
      ["on-dell", "dell", false, NOW - 15 * MIN],
      ["asked", null, true, NOW - 9 * MIN],
    ]);
    expect(all.waiting[1].pending).toEqual(question);
    expect(all.waiting[0].pending).toBeNull();
  });

  it("says what a working agent last did, in its own Session's events, questions aside", () => {
    expect(all.working.map((l) => l.row.id)).toEqual(["working"]);
    expect(all.working[0]).toMatchObject({ doing: "Ran pnpm check", summary: "1 file changed, +40 −3", project: "repo2", description: "add the key" });
  });

  it("lists idle agents, the one that stopped last first, with what the last turn changed", () => {
    expect(all.idle.map((l) => [l.row.id, l.summary])).toEqual([
      ["idle-new", null],
      ["idle-old", "1 file changed, +12"],
    ]);
  });

  it("leaves out Tabs that run no agent", () => {
    const ids = [...all.waiting, ...all.working, ...all.idle].map((l) => l.row.id);
    expect(ids).not.toContain("shell");
    expect(ids).toHaveLength(5);
  });
});
