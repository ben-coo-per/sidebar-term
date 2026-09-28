import { describe, expect, it } from "vitest";
import { findRow, groupRows, strayId, tabRow, unpairedHosts, type Facts, type HostView, type HostViews } from "./rows";
import { LOCAL_HOST } from "../host/ids";
import type { AgentEvent, LayoutSnapshot, SessionInfo, Tab } from "../types";

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

/** The page's own Host, with these facts. */
function mac(sessions: Facts = {}, over: Partial<HostView> = {}): HostView {
  return { id: LOCAL_HOST, name: "bens-mac", paired: true, online: true, home: HOME, layout: null, sessions, events: [], ...over };
}

/** A Host linked Tabs point at. */
function dell(over: Partial<HostView> = {}): HostView {
  return { id: "h_dell", name: "dell", paired: true, online: true, home: "/home/you", layout: null, sessions: {}, events: [], ...over };
}

describe("tabRow", () => {
  it("derives the Title as the Mac does: a rename, the agent's project and doing, the OSC title, the process, the cwd", () => {
    expect(tabRow(tab("t", { customTitle: "Build" }), mac({ 1: info(1) })).title).toBe("Build");
    expect(tabRow(tab("t"), mac({ 1: info(1) })).title).toBe("jack");
    expect(tabRow(tab("t"), mac({ 1: info(1, { agent: "claude", status: "running" }) }))).toMatchObject({
      title: "jack",
      agent: "claude",
      status: "running",
      label: { project: "jack", description: null },
    });
    expect(tabRow(tab("t"), mac({ 1: info(1, { agent: "claude", title: "◐ Fix the pairing handshake" }) })).title).toBe(
      "jack · fix the pairing",
    );
    expect(tabRow(tab("t"), mac({ 1: info(1, { shellIsForeground: false, foreground: "vim", title: "vim: notes.md" }) })).title).toBe(
      "vim: notes.md",
    );
    expect(tabRow(tab("t"), mac({ 1: info(1, { shellIsForeground: false, foreground: "ssh", remote: true }) }))).toMatchObject({
      title: "ssh",
      remote: true,
    });
    expect(tabRow(tab("t"), mac({ 1: info(1, { cwd: HOME }) })).title).toBe("~");
  });

  it("falls back to the Tab's last cwd before the Host has facts, and lists a Session-less Tab", () => {
    expect(tabRow(tab("t", { lastCwd: "/tmp/work" }), mac())).toMatchObject({ title: "work", sessionId: 1, status: null, label: null });
    expect(tabRow(tab("t", { sessionId: null, lastCwd: null }), mac())).toMatchObject({ title: "~", sessionId: null });
  });

  it("names an agent after its last prompt, from its Host's events, when its title says nothing", () => {
    const events: AgentEvent[] = [
      { at: 1, sessionId: 1, kind: "started", text: "Started “Tidy the pairing screen”" },
      { at: 2, sessionId: 2, kind: "started", text: "Started “Another Session's”" },
    ];
    const row = tabRow(tab("t"), mac({ 1: info(1, { agent: "codex", status: "running" }) }, { events }));
    expect(row.label).toEqual({ project: "jack", description: "tidy the pairing" });
    expect(row.title).toBe("jack · tidy the pairing");
  });

  it("says which Host a Tab's Session runs on, except for the page's own", () => {
    expect(tabRow(tab("t"), mac())).toMatchObject({ host: LOCAL_HOST, hostName: null, reachable: true });
    expect(tabRow(tab("t"), dell({ online: false }))).toMatchObject({ host: "h_dell", hostName: "dell", reachable: false });
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
  const hosts: HostViews = { [LOCAL_HOST]: mac({ 1: info(1, { agent: "codex", status: "needs-input" }) }, { layout }) };

  it("lists every Group's Tabs in order, skipping ids the layout does not know", () => {
    const rows = groupRows(hosts);
    expect(rows.map((g) => g.name)).toEqual(["Work", "Empty"]);
    expect(rows[0].tabs.map((t) => t.title)).toEqual(["jack", "Two"]);
    expect(rows[0].tabs[0].status).toBe("needs-input");
    expect(rows[1].tabs).toEqual([]);
  });

  it("finds one Tab's row, or nothing once it is gone", () => {
    expect(findRow(hosts, "b")?.title).toBe("Two");
    expect(findRow(hosts, "zzz")).toBeNull();
    expect(findRow({ [LOCAL_HOST]: mac() }, "a")).toBeNull();
    expect(findRow({}, "a")).toBeNull();
  });
});

describe("linked Tabs", () => {
  /** The Mac's layout: a local Tab, then a link to the Dell's `r1`, then one to its `r2`. */
  const macLayout: LayoutSnapshot = {
    revision: 8,
    groups: [{ id: "g1", name: "Work", collapsed: false, tabIds: ["a", "l1", "l2"] }],
    tabs: {
      a: tab("a", { groupId: "g1", sessionId: 1 }),
      l1: tab("l1", { groupId: "g1", sessionId: null, link: { hostId: "h_dell", tabId: "r1" } }),
      l2: tab("l2", { groupId: "g1", sessionId: null, link: { hostId: "h_dell", tabId: "r2" } }),
    },
    activeTabId: "l1",
  };
  /** The Dell's own layout: its Groups do not matter here, its Tabs do. */
  const dellLayout: LayoutSnapshot = {
    revision: 2,
    groups: [{ id: "d", name: "Tabs", collapsed: false, tabIds: ["r1", "r2"] }],
    tabs: {
      r1: tab("r1", { groupId: "d", sessionId: 1, customTitle: "Agent on the Dell" }),
      r2: tab("r2", { groupId: "d", sessionId: 2 }),
    },
    activeTabId: "r1",
  };
  const dellFacts: Facts = {
    1: info(1, { agent: "claude", status: "running", cwd: "/home/you/repos/jack" }),
    2: info(2, { cwd: "/home/you" }),
  };
  const home = mac({ 1: info(1) }, { layout: macLayout });

  it("sit in the Mac's Groups, described by the Host they point at", () => {
    const rows = groupRows({ [LOCAL_HOST]: home, h_dell: dell({ layout: dellLayout, sessions: dellFacts }) });
    expect(rows).toHaveLength(1);
    expect(rows[0].tabs.map((t) => [t.id, t.host, t.hostName, t.sessionId, t.title])).toEqual([
      ["a", LOCAL_HOST, null, 1, "jack"],
      ["l1", "h_dell", "dell", 1, "Agent on the Dell"],
      ["l2", "h_dell", "dell", 2, "~"],
    ]);
    // Session 1 on the Mac and Session 1 on the Dell are two Sessions.
    expect(rows[0].tabs[0].agent).toBeNull();
    expect(rows[0].tabs[1]).toMatchObject({ agent: "claude", status: "running", reachable: true });
  });

  it("are greyed while their Host does not answer, and placeholders before it ever did", () => {
    const asleep = groupRows({ [LOCAL_HOST]: home, h_dell: dell({ layout: dellLayout, sessions: dellFacts, online: false }) });
    expect(asleep[0].tabs[1]).toMatchObject({ title: "Agent on the Dell", reachable: false, sessionId: 1 });

    const unheard = groupRows({ [LOCAL_HOST]: home, h_dell: dell({ online: false }) });
    expect(unheard[0].tabs.map((t) => [t.id, t.title, t.sessionId, t.reachable])).toEqual([
      ["a", "jack", 1, true],
      ["l1", "…", null, false],
      ["l2", "…", null, false],
    ]);
  });

  it("wait behind a pairing when the phone holds no token for their Host, or does not know the Host", () => {
    const hosts = { [LOCAL_HOST]: home, h_dell: dell({ paired: false }) };
    expect(groupRows(hosts)[0].tabs.map((t) => t.id)).toEqual(["a"]);
    expect(unpairedHosts(hosts)).toEqual([{ host: "h_dell", name: "dell", tabs: 2 }]);

    expect(groupRows({ [LOCAL_HOST]: home })[0].tabs.map((t) => t.id)).toEqual(["a"]);
    expect(unpairedHosts({ [LOCAL_HOST]: home })).toEqual([]);
    expect(unpairedHosts({ [LOCAL_HOST]: home, h_dell: dell({ layout: dellLayout }) })).toEqual([]);
  });

  it("go once their Host no longer has the Tab, before the Mac hears so", () => {
    const closed: LayoutSnapshot = { ...dellLayout, groups: [{ ...dellLayout.groups[0], tabIds: ["r2"] }], tabs: { r2: dellLayout.tabs.r2 } };
    const rows = groupRows({ [LOCAL_HOST]: home, h_dell: dell({ layout: closed, sessions: dellFacts }) });
    expect(rows[0].tabs.map((t) => t.id)).toEqual(["a", "l2"]);
  });

  it("a Tab the Host has that no link places goes to the Group named after the Host, as the Mac will link it", () => {
    const more: LayoutSnapshot = {
      ...dellLayout,
      groups: [{ ...dellLayout.groups[0], tabIds: ["r1", "r3", "r2"] }],
      tabs: { ...dellLayout.tabs, r3: tab("r3", { groupId: "d", sessionId: 3, customTitle: "Made on the phone" }) },
    };
    const there = dell({ layout: more, sessions: dellFacts });
    const rows = groupRows({ [LOCAL_HOST]: home, h_dell: there });
    expect(rows.map((g) => [g.id, g.name])).toEqual([
      ["g1", "Work"],
      ["host:h_dell", "dell"],
    ]);
    expect(rows[1].tabs.map((t) => [t.id, t.title, t.host])).toEqual([[strayId("h_dell", "r3"), "Made on the phone", "h_dell"]]);
    expect(findRow({ [LOCAL_HOST]: home, h_dell: there }, strayId("h_dell", "r3"))?.sessionId).toBe(3);

    // The Mac already has a Group named after the Host: the Tab joins it.
    const named: LayoutSnapshot = { ...macLayout, groups: [...macLayout.groups, { id: "g9", name: "dell", collapsed: false, tabIds: [] }] };
    const joined = groupRows({ [LOCAL_HOST]: mac({}, { layout: named }), h_dell: there });
    expect(joined.map((g) => g.id)).toEqual(["g1", "g9"]);
    expect(joined[1].tabs.map((t) => t.id)).toEqual([strayId("h_dell", "r3")]);
  });
});
