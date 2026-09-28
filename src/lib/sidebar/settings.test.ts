import { describe, expect, it } from "vitest";
import {
  DEFAULT_MANAGER_ZOOM,
  DEFAULT_MODE,
  DEFAULT_PANEL,
  DEFAULT_SIDEBAR_WIDTH,
  MAX_MANAGER_NEEDS_WIDTH,
  MAX_PANEL_HEIGHT,
  MIN_MANAGER_LANES_HEIGHT,
  MIN_SIDEBAR_WIDTH,
  parseSidebarSection,
} from "./settings";

const DEFAULTS = {
  width: DEFAULT_SIDEBAR_WIDTH,
  panel: DEFAULT_PANEL,
  unread: [],
  mode: DEFAULT_MODE,
  managerZoom: DEFAULT_MANAGER_ZOOM,
  managerLanesHeight: null,
  managerNeedsWidth: null,
};

describe("parseSidebarSection", () => {
  it("reads what the Host moved out of a version-1 layout.json", () => {
    expect(parseSidebarSection({ width: 300, panel: { view: "usage", collapsed: true, height: 200 }, unread: ["t2"] })).toEqual({
      width: 300,
      panel: { view: "usage", collapsed: true, height: 200 },
      unread: ["t2"],
      mode: DEFAULT_MODE,
      managerZoom: DEFAULT_MANAGER_ZOOM,
      managerLanesHeight: null,
      managerNeedsWidth: null,
    });
  });

  it("reads the window's mode and Manager's zoom", () => {
    const parsed = parseSidebarSection({ mode: "manager", managerZoom: "4h" });
    expect(parsed.mode).toBe("manager");
    expect(parsed.managerZoom).toBe("4h");
    expect(parseSidebarSection({ mode: "lanes", managerZoom: "2h" })).toMatchObject({ mode: DEFAULT_MODE, managerZoom: DEFAULT_MANAGER_ZOOM });
  });

  it("opens in Manager until the user picks a mode, and keeps the one picked", () => {
    expect(DEFAULT_MODE).toBe("manager");
    expect(parseSidebarSection({}).mode).toBe("manager");
    expect(parseSidebarSection({ mode: "tabs" }).mode).toBe("tabs");
  });

  it("reads the sizes Manager's areas were dragged to, clamped, and none when never dragged", () => {
    expect(parseSidebarSection({ managerLanesHeight: 300, managerNeedsWidth: 420 })).toMatchObject({
      managerLanesHeight: 300,
      managerNeedsWidth: 420,
    });
    expect(parseSidebarSection({ managerLanesHeight: 1, managerNeedsWidth: 99999 })).toMatchObject({
      managerLanesHeight: MIN_MANAGER_LANES_HEIGHT,
      managerNeedsWidth: MAX_MANAGER_NEEDS_WIDTH,
    });
    expect(parseSidebarSection({ managerLanesHeight: null, managerNeedsWidth: "wide" })).toMatchObject({
      managerLanesHeight: null,
      managerNeedsWidth: null,
    });
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
