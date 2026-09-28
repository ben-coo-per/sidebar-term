import { describe, expect, it } from "vitest";
import {
  DEFAULT_MANAGER_ZOOM,
  DEFAULT_PANEL,
  DEFAULT_SIDEBAR_WIDTH,
  MAX_PANEL_HEIGHT,
  MIN_SIDEBAR_WIDTH,
  parseSidebarSection,
} from "./settings";

const DEFAULTS = { width: DEFAULT_SIDEBAR_WIDTH, panel: DEFAULT_PANEL, unread: [], mode: "tabs", managerZoom: DEFAULT_MANAGER_ZOOM };

describe("parseSidebarSection", () => {
  it("reads what the Host moved out of a version-1 layout.json", () => {
    expect(parseSidebarSection({ width: 300, panel: { view: "usage", collapsed: true, height: 200 }, unread: ["t2"] })).toEqual({
      width: 300,
      panel: { view: "usage", collapsed: true, height: 200 },
      unread: ["t2"],
      mode: "tabs",
      managerZoom: DEFAULT_MANAGER_ZOOM,
    });
  });

  it("reads the window's mode and Manager's zoom", () => {
    const parsed = parseSidebarSection({ mode: "manager", managerZoom: "4h" });
    expect(parsed.mode).toBe("manager");
    expect(parsed.managerZoom).toBe("4h");
    expect(parseSidebarSection({ mode: "lanes", managerZoom: "2h" })).toMatchObject({ mode: "tabs", managerZoom: DEFAULT_MANAGER_ZOOM });
  });

  it("falls back to the defaults for anything missing or malformed", () => {
    expect(parseSidebarSection(undefined)).toEqual(DEFAULTS);
    expect(parseSidebarSection("junk")).toEqual(DEFAULTS);
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
