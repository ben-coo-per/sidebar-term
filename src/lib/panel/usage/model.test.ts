import { describe, expect, it } from "vitest";
import {
  currentPercent,
  fillFraction,
  formatAgo,
  formatDuration,
  formatResetsIn,
  freshnessNote,
  parseUsageSection,
  peakPercent,
  usageLevel,
  waitingNote,
} from "./model";
import type { AgentUsage, UsageWindow } from "../../types";

const MIN = 60_000;
const HOUR = 60 * MIN;
const NOW = 1_777_000_000_000;

function win(usedPercent: number, resetsAt: number | null): UsageWindow {
  return { label: "5h", usedPercent, resetsAt };
}

describe("usage model", () => {
  it("levels", () => {
    expect(usageLevel(0)).toBe("ok");
    expect(usageLevel(79.9)).toBe("ok");
    expect(usageLevel(80)).toBe("high");
    expect(usageLevel(95)).toBe("full");
    expect(usageLevel(130)).toBe("full");
  });

  it("reads 0% once a window has reset", () => {
    expect(currentPercent(win(60, NOW + 1), NOW)).toBe(60);
    expect(currentPercent(win(60, NOW), NOW)).toBe(0);
    expect(currentPercent(win(60, null), NOW)).toBe(60);
  });

  it("clamps the bar", () => {
    expect(fillFraction(-5)).toBe(0);
    expect(fillFraction(48)).toBeCloseTo(0.48);
    expect(fillFraction(120)).toBe(1);
  });

  it("durations", () => {
    expect(formatDuration(30_000)).toBe("<1m");
    expect(formatDuration(42 * MIN)).toBe("42m");
    expect(formatDuration(2 * HOUR)).toBe("2h");
    expect(formatDuration(2 * HOUR + 10 * MIN + 30_000)).toBe("2h 10m");
    expect(formatDuration(76 * HOUR)).toBe("3d 4h");
    expect(formatDuration(48 * HOUR)).toBe("2d");
  });

  it("reset and age text", () => {
    expect(formatResetsIn(win(1, null), NOW)).toBe("");
    expect(formatResetsIn(win(1, NOW - 1), NOW)).toBe("reset");
    expect(formatResetsIn(win(1, NOW + 90 * MIN), NOW)).toBe("1h 30m");
    expect(formatAgo(NOW - 5_000, NOW)).toBe("just now");
    expect(formatAgo(NOW - 3 * HOUR, NOW)).toBe("3h ago");
  });

  it("peak is the fullest window now", () => {
    const a: AgentUsage = {
      agent: "codex",
      windows: [win(12, NOW + HOUR), win(97, NOW - 1)],
      plan: null,
      updatedAt: null,
      error: null,
      rateLimitedUntil: null,
    };
    expect(peakPercent(a, NOW)).toBe(12); // the 97% window has reset
    expect(peakPercent({ ...a, windows: [] }, NOW)).toBeNull();
  });

  it("says how old stale numbers are, and when a rate limit is being waited out", () => {
    const a: AgentUsage = {
      agent: "claude",
      windows: [win(48, NOW + HOUR)],
      plan: "max",
      updatedAt: NOW - 3 * MIN,
      error: null,
      rateLimitedUntil: null,
    };
    expect(freshnessNote(a, NOW)).toBe("max"); // within the read interval: fresh enough
    expect(freshnessNote({ ...a, updatedAt: NOW - 12 * MIN }, NOW)).toBe("max · as of 12m ago");
    expect(freshnessNote({ ...a, plan: null, updatedAt: NOW - 3 * HOUR }, NOW)).toBe("as of 3h ago");
    expect(freshnessNote({ ...a, updatedAt: null }, NOW)).toBe("max");
    expect(waitingNote(a, NOW)).toBe("");
    expect(waitingNote({ ...a, rateLimitedUntil: NOW + 25 * MIN }, NOW)).toBe(
      "Waiting for the limit to clear · reads again in 25m",
    );
    expect(waitingNote({ ...a, rateLimitedUntil: NOW - 1 }, NOW)).toBe("Waiting for the limit to clear");
  });

  it("parses the settings section", () => {
    expect(parseUsageSection(undefined)).toEqual(["claude", "codex"]);
    expect(parseUsageSection({ agents: "claude" })).toEqual(["claude", "codex"]);
    expect(parseUsageSection({ agents: [] })).toEqual([]);
    expect(parseUsageSection({ agents: ["codex", "claude", "gemini", 7] })).toEqual(["claude", "codex"]);
  });
});
