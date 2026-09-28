import { describe, expect, it } from "vitest";
import type { LayoutSnapshot, Tab } from "../types";
import { hostTabOrder, linksOutOfLine } from "./links";

const there = (snap: LayoutSnapshot) => hostTabOrder(snap);

function tab(id: string, groupId: string, link?: Tab["link"]): Tab {
  return { id, groupId, sessionId: null, customTitle: null, lastCwd: null, ...(link ? { link } : {}) };
}

function snap(groups: [string, string[]][], tabs: Tab[]): LayoutSnapshot {
  return {
    revision: 1,
    groups: groups.map(([id, tabIds]) => ({ id, name: id, collapsed: false, tabIds })),
    tabs: Object.fromEntries(tabs.map((t) => [t.id, t])),
    activeTabId: null,
  };
}

// The Dell: r1 r2 in one Group, r3 in another.
const dell = snap(
  [
    ["a", ["r1", "r2"]],
    ["b", ["r3", "ghost"]],
  ],
  [tab("r1", "a"), tab("r2", "a"), tab("r3", "b")],
);

describe("hostTabOrder", () => {
  it("lists the Host's Tabs top to bottom, skipping ids it has no Tab for", () => {
    expect(hostTabOrder(dell)).toEqual(["r1", "r2", "r3"]);
  });

  it("leaves out the Tabs this Mac asked it to close", () => {
    expect(hostTabOrder(dell, new Set(["r2"]))).toEqual(["r1", "r3"]);
  });
});

describe("linksOutOfLine", () => {
  const link = (tabId: string, hostId = "h_dell") => ({ hostId, tabId });

  it("is in line when every Tab there has one link here, wherever it was put", () => {
    const mac = snap(
      [["g", ["t1", "l3", "l1", "l2"]]],
      [tab("t1", "g"), tab("l1", "g", link("r1")), tab("l2", "g", link("r2")), tab("l3", "g", link("r3"))],
    );
    expect(linksOutOfLine(mac, "h_dell", there(dell))).toBe(false);
  });

  it("is out of line for a Tab there with no link (a stray)", () => {
    const mac = snap([["g", ["l1", "l2"]]], [tab("l1", "g", link("r1")), tab("l2", "g", link("r2"))]);
    expect(linksOutOfLine(mac, "h_dell", there(dell))).toBe(true);
  });

  it("is out of line for a link whose Tab is gone there", () => {
    const mac = snap(
      [["g", ["l1", "l2", "l3", "l4"]]],
      [tab("l1", "g", link("r1")), tab("l2", "g", link("r2")), tab("l3", "g", link("r3")), tab("l4", "g", link("r4"))],
    );
    expect(linksOutOfLine(mac, "h_dell", there(dell))).toBe(true);
  });

  it("does not count a Tab being closed there as a stray", () => {
    const mac = snap([["g", ["l1", "l3"]]], [tab("l1", "g", link("r1")), tab("l3", "g", link("r3"))]);
    expect(linksOutOfLine(mac, "h_dell", hostTabOrder(dell, new Set(["r2"])))).toBe(false);
  });

  it("counts only this Host's links", () => {
    const mac = snap(
      [["g", ["l1", "l2", "x"]]],
      [tab("l1", "g", link("r1")), tab("l2", "g", link("r2")), tab("x", "g", link("r3", "h_other"))],
    );
    expect(linksOutOfLine(mac, "h_dell", there(dell))).toBe(true);
    expect(linksOutOfLine(mac, "h_other", ["r3"])).toBe(false);
  });
});
