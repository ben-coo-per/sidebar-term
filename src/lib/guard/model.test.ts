import { describe, expect, it } from "vitest";
import { DEFAULT_GUARD_LIMIT, guardButtonTitle, parseGuardSection } from "./model";

describe("parseGuardSection", () => {
  it("defaults to off at 85% for anything missing or malformed", () => {
    for (const raw of [undefined, null, 3, "on", [], {}]) {
      expect(parseGuardSection(raw)).toEqual({ on: false, limitPercent: DEFAULT_GUARD_LIMIT });
    }
  });

  it("keeps a saved state and an offered limit only", () => {
    expect(parseGuardSection({ on: true, limitPercent: 75 })).toEqual({ on: true, limitPercent: 75 });
    expect(parseGuardSection({ on: "yes", limitPercent: 77 })).toEqual({ on: false, limitPercent: DEFAULT_GUARD_LIMIT });
  });
});

describe("guardButtonTitle", () => {
  it("says what it does, and how many Tabs are frozen while on", () => {
    expect(guardButtonTitle(false, 85, 0)).toMatch(/^Memory Guard: freeze/);
    expect(guardButtonTitle(true, 80, 0)).toContain("above 80% memory; no Tab frozen");
    expect(guardButtonTitle(true, 80, 1)).toContain("1 Tab frozen");
    expect(guardButtonTitle(true, 80, 2)).toContain("2 Tabs frozen");
  });
});
