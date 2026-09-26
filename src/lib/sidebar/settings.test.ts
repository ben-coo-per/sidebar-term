import { describe, expect, it } from "vitest";
import { DEFAULT_PANEL, DEFAULT_SIDEBAR_WIDTH, MAX_PANEL_HEIGHT, MIN_SIDEBAR_WIDTH, parseSidebarSection } from "./settings";

describe("parseSidebarSection", () => {
  it("reads what the Host moved out of a version-1 layout.json", () => {
    expect(parseSidebarSection({ width: 300, panel: { view: "usage", collapsed: true, height: 200 }, unread: ["t2"] })).toEqual({
      width: 300,
      panel: { view: "usage", collapsed: true, height: 200 },
      unread: ["t2"],
    });
  });

  it("falls back to the defaults for anything missing or malformed", () => {
    expect(parseSidebarSection(undefined)).toEqual({ width: DEFAULT_SIDEBAR_WIDTH, panel: DEFAULT_PANEL, unread: [] });
    expect(parseSidebarSection("junk")).toEqual({ width: DEFAULT_SIDEBAR_WIDTH, panel: DEFAULT_PANEL, unread: [] });
    const parsed = parseSidebarSection({ width: "wide", panel: { view: "nope", height: "tall" }, unread: ["a", 7, null] });
    expect(parsed.width).toBe(DEFAULT_SIDEBAR_WIDTH);
    expect(parsed.panel).toEqual(DEFAULT_PANEL);
    expect(parsed.unread).toEqual(["a"]);
  });

  it("clamps the width and the Panel height", () => {
    const parsed = parseSidebarSection({ width: 10, panel: { view: "activity", collapsed: false, height: 9999 } });
    expect(parsed.width).toBe(MIN_SIDEBAR_WIDTH);
    expect(parsed.panel.height).toBe(MAX_PANEL_HEIGHT);
  });
});
