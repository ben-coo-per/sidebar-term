import { describe, expect, it } from "vitest";
import type { SuiteSnapshot } from "../types";
import { formatDuration, formatEta, newestSuite, parseSuiteSection, roundEta, sortSuites, suiteView } from "./model";

function snap(partial: Partial<SuiteSnapshot>): SuiteSnapshot {
  return {
    id: 1,
    sessionId: 3,
    runner: "vitest",
    phase: "testing",
    startedAt: 1_000,
    elapsedMs: 42_000,
    done: 0,
    total: null,
    failed: 0,
    outcome: null,
    etaMs: null,
    source: "none",
    typicalMs: null,
    longestMs: null,
    runs: 0,
    ...partial,
  };
}

describe("durations", () => {
  it.each([
    [0, "0 s"],
    [42_900, "42 s"],
    [90_000, "1m 30s"],
    [130_000, "2m 10s"],
    [245_000, "4m 05s"],
    [3_720_000, "1h 02m"],
  ])("formatDuration(%s) = %s", (ms, want) => {
    expect(formatDuration(ms)).toBe(want);
  });

  it("rounds an ETA to 5 s under a minute and 15 s above, never under 5 s", () => {
    expect(roundEta(0)).toBe(5_000);
    expect(roundEta(2_400)).toBe(5_000);
    expect(roundEta(43_000)).toBe(45_000);
    expect(roundEta(58_000)).toBe(60_000);
    expect(roundEta(66_000)).toBe(60_000);
    expect(roundEta(70_000)).toBe(75_000);
    expect(roundEta(130_000)).toBe(135_000);
  });

  it.each([
    [43_000, "~45 s"],
    [61_000, "~1m"],
    [130_000, "~2m 15s"],
  ])("formatEta(%s) = %s", (ms, want) => {
    expect(formatEta(ms)).toBe(want);
  });
});

describe("suiteView", () => {
  it("state 1: progress known, with failures and an ETA", () => {
    const v = suiteView(snap({ source: "stream", done: 12, total: 48, failed: 2, etaMs: 61_000 }));
    expect(v.text).toBe("12/48 · 2✗ · ~1m");
    expect(v.short).toBe("12/48");
    expect(v.fraction).toBeCloseTo(0.25);
    expect(v.failedFraction).toBeCloseTo(2 / 48);
    expect(v.tone).toBe("failed");
    expect(v.title).toBe("vitest · 12 of 48 · 2 failed · 42 s, about 1 min left");
  });

  it("progress known, all passing so far, no ETA yet", () => {
    const v = suiteView(snap({ source: "stream", done: 0, total: 48, etaMs: null, elapsedMs: 800 }));
    expect(v.text).toBe("0/48");
    expect(v.tone).toBe("running");
    expect(v.title).toBe("vitest · 0 of 48 · 0 s");
  });

  it("tests done without a total (jest) counts up and follows the history", () => {
    const v = suiteView(snap({ runner: "jest", source: "stream", done: 12, failed: 1, etaMs: 20_000, typicalMs: 60_000 }));
    expect(v.text).toBe("12 done · 1✗ · ~20 s");
    expect(v.short).toBe("12 done");
    expect(v.fraction).toBeCloseTo(0.7);
    expect(v.tone).toBe("failed");
    const noHistory = suiteView(snap({ runner: "jest", source: "stream", done: 3 }));
    expect(noHistory.text).toBe("3 done · 42 s");
    expect(noHistory.fraction).toBeNull();
  });

  it("state 2: history only", () => {
    const v = suiteView(snap({ runner: "pytest", source: "history", elapsedMs: 90_000, typicalMs: 130_000, longestMs: 150_000, runs: 5, etaMs: 40_000 }));
    expect(v.text).toBe("1m 30s of ~2m 10s");
    expect(v.short).toBe("1m 30s");
    expect(v.fraction).toBeCloseTo(90 / 130);
    expect(v.tone).toBe("estimate");
    expect(v.title).toBe("pytest · 1m 30s of ~2m 10s, about 40 s left (from 5 runs)");
  });

  it("one run in history says about, and past the median the bar stays full", () => {
    const v = suiteView(snap({ source: "history", elapsedMs: 140_000, typicalMs: 130_000, longestMs: 150_000, runs: 1, etaMs: 0 }));
    expect(v.text).toBe("2m 20s · usually about 2m 10s");
    expect(v.fraction).toBe(1);
    expect(v.tone).toBe("estimate");
    expect(v.title).toContain("(from 1 run)");
  });

  it("overrun: past the longest run, amber", () => {
    const v = suiteView(snap({ runner: "cargo", source: "history", elapsedMs: 245_000, typicalMs: 130_000, longestMs: 150_000, runs: 5, etaMs: 0 }));
    expect(v.text).toBe("4m 05s · longer than usual");
    expect(v.tone).toBe("overrun");
    expect(v.fraction).toBe(1);
    expect(v.title).toBe("cargo test · 4m 05s, longer than any of the last 5 runs (longest 2m 30s)");
  });

  it("state 3: nothing known", () => {
    const v = suiteView(snap({ elapsedMs: 90_000 }));
    expect(v.text).toBe("testing · 1m 30s");
    expect(v.short).toBe("1m 30s");
    expect(v.fraction).toBeNull();
    expect(v.tone).toBe("running");
  });

  it("building says so, whatever the history", () => {
    const v = suiteView(snap({ runner: "go", phase: "building", source: "history", elapsedMs: 20_000, typicalMs: 60_000, runs: 3 }));
    expect(v.text).toBe("building · 20 s");
    expect(v.fraction).toBeNull();
    expect(v.title).toBe("go test · building · 20 s");
  });

  it("finished: the result lingers, red when anything failed", () => {
    const failed = suiteView(snap({ phase: "done", source: "stream", done: 48, total: 48, failed: 2, outcome: "failed", elapsedMs: 62_000 }));
    expect(failed.text).toBe("48/48 · 2 failed");
    expect(failed.tone).toBe("failed");
    expect(failed.fraction).toBe(1);
    expect(failed.failedFraction).toBeCloseTo(2 / 48);
    expect(failed.title).toBe("vitest · finished in 1m 02s · 48 of 48 · 2 failed");

    const passed = suiteView(snap({ phase: "done", source: "stream", done: 48, total: 48, outcome: "passed" }));
    expect(passed.text).toBe("48/48");
    expect(passed.tone).toBe("done");

    const plain = suiteView(snap({ phase: "done" }));
    expect(plain.text).toBe("done");
    expect(plain.short).toBe("done");
    expect(plain.tone).toBe("done");

    const wrapperFailed = suiteView(snap({ runner: "cargo", phase: "done", source: "stream", total: 12, done: 12, outcome: "failed" }));
    expect(wrapperFailed.text).toBe("12/12 · failed");
    expect(wrapperFailed.tone).toBe("failed");
    expect(wrapperFailed.failedFraction).toBe(0);

    const plainFailed = suiteView(snap({ runner: "go", phase: "done", outcome: "failed" }));
    expect(plainFailed.text).toBe("failed");
    expect(plainFailed.failedFraction).toBe(1);
  });
});

describe("which Suite a Tab shows", () => {
  const a = snap({ id: 1, startedAt: 100 });
  const b = snap({ id: 2, startedAt: 200 });
  const c = snap({ id: 3, startedAt: 200 });

  it("is the newest one", () => {
    expect(newestSuite([])).toBeNull();
    expect(newestSuite([a, c, b])).toBe(c);
  });

  it("lists newest first for the Panel, without mutating", () => {
    const list = [a, b, c];
    expect(sortSuites(list).map((s) => s.id)).toEqual([3, 2, 1]);
    expect(list.map((s) => s.id)).toEqual([1, 2, 3]);
  });
});

describe("parseSuiteSection", () => {
  it("injects unless turned off", () => {
    expect(parseSuiteSection(undefined)).toEqual({ progress: true });
    expect(parseSuiteSection({ progress: false })).toEqual({ progress: false });
    expect(parseSuiteSection({ progress: "no" })).toEqual({ progress: true });
  });
});
