import { describe, expect, it } from "vitest";
import { findRow, groupRows, tabRow, type Facts } from "./rows";
import type { LayoutSnapshot, SessionInfo, Tab } from "../types";

function info(sessionId: number, over: Partial<SessionInfo> = {}): SessionInfo {
  return {
    sessionId,
    foreground: "zsh",
    shellIsForeground: true,
    agent: null,
    cwd: "/Users/you/Dev/jack",
    remote: false,
    git: null,
    title: null,
    bells: 0,
    status: null,
    history: [],
    hooked: false,
    pending: null,
    ...over,
  };
}

function tab(id: string, over: Partial<Tab> = {}): Tab {
  return { id, groupId: "g", sessionId: 1, customTitle: null, lastCwd: null, ...over };
}

const HOME = "/Users/you";

describe("tabRow", () => {
  it("derives the Title as the Mac does: a rename, the agent's project and doing, the OSC title, the process, the cwd", () => {
    const facts: Facts = { 1: info(1) };
    expect(tabRow(tab("t", { customTitle: "Build" }), facts, HOME).title).toBe("Build");
    expect(tabRow(tab("t"), facts, HOME).title).toBe("jack");
    expect(tabRow(tab("t"), { 1: info(1, { agent: "claude", status: "running" }) }, HOME)).toMatchObject({
      title: "jack",
      agent: "claude",
      status: "running",
    });
    expect(tabRow(tab("t"), { 1: info(1, { agent: "claude", title: "◐ Fix the pairing handshake" }) }, HOME).title).toBe(
      "jack · fix the pairing",
    );
    expect(
      tabRow(tab("t"), { 1: info(1, { shellIsForeground: false, foreground: "vim", title: "vim: notes.md" }) }, HOME).title,
    ).toBe("vim: notes.md");
    expect(tabRow(tab("t"), { 1: info(1, { shellIsForeground: false, foreground: "ssh", remote: true }) }, HOME)).toMatchObject({
      title: "ssh",
      remote: true,
    });
    expect(tabRow(tab("t"), { 1: info(1, { cwd: HOME }) }, HOME).title).toBe("~");
  });

  it("falls back to the Tab's last cwd before the Host has facts, and lists a Session-less Tab", () => {
    expect(tabRow(tab("t", { lastCwd: "/tmp/work" }), {}, HOME)).toMatchObject({ title: "work", sessionId: 1, status: null });
    expect(tabRow(tab("t", { sessionId: null, lastCwd: null }), {}, HOME)).toMatchObject({ title: "~", sessionId: null });
  });
});

describe("groupRows and findRow", () => {
  const layout: LayoutSnapshot = {
    revision: 3,
    groups: [
      { id: "g1", name: "Work", collapsed: false, tabIds: ["a", "b", "ghost"] },
      { id: "g2", name: "Empty", collapsed: true, tabIds: [] },
    ],
    tabs: {
      a: tab("a", { groupId: "g1", sessionId: 1 }),
      b: tab("b", { groupId: "g1", sessionId: 2, customTitle: "Two" }),
    },
    activeTabId: "b",
  };
  const facts: Facts = { 1: info(1, { agent: "codex", status: "needs-input" }) };

  it("lists every Group's Tabs in order, skipping ids the layout does not know", () => {
    const rows = groupRows(layout, facts, HOME);
    expect(rows.map((g) => g.name)).toEqual(["Work", "Empty"]);
    expect(rows[0].tabs.map((t) => t.title)).toEqual(["jack", "Two"]);
    expect(rows[0].tabs[0].status).toBe("needs-input");
    expect(rows[1].tabs).toEqual([]);
  });

  it("finds one Tab's row, or nothing once it is gone", () => {
    expect(findRow(layout, facts, HOME, "b")?.title).toBe("Two");
    expect(findRow(layout, facts, HOME, "zzz")).toBeNull();
    expect(findRow(null, facts, HOME, "a")).toBeNull();
  });
});
