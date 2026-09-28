import { describe, expect, it } from "vitest";
import {
  formatDuration,
  holdOrder,
  lastLines,
  lastPrompt,
  laneStart,
  segments,
  sortLanes,
  ticks,
  turnSummary,
  windowSpan,
  type Orderable,
} from "./model";
import type { AgentEvent, StatusChange } from "../types";

const MIN = 60_000;
const NOW = new Date(2026, 8, 28, 11, 55).getTime();
const ago = (m: number) => NOW - m * MIN;

describe("segments", () => {
  const history: StatusChange[] = [
    { status: "running", at: ago(27) },
    { status: "needs-input", at: ago(15) },
    { status: "running", at: ago(14) },
    { status: "needs-input", at: ago(2) },
  ];

  it("places each stretch in the window, the last one up to now", () => {
    const s = segments(history, NOW, 60 * MIN);
    expect(s.map((x) => x.status)).toEqual(["running", "needs-input", "running", "needs-input"]);
    expect(s[0].left).toBeCloseTo(((60 - 27) / 60) * 100);
    expect(s[0].width).toBeCloseTo((12 / 60) * 100);
    expect(s[3].left + s[3].width).toBeCloseTo(100);
  });

  it("clips a stretch that began before the window", () => {
    const s = segments(history, NOW, 15 * MIN);
    expect(s[0]).toMatchObject({ status: "needs-input", left: 0 });
    expect(s[0].width).toBeCloseTo((1 / 15) * 100);
  });

  it("leaves a gap where there was no agent", () => {
    const s = segments([{ status: "running", at: ago(30) }, { status: null, at: ago(20) }, { status: "done", at: ago(10) }], NOW, 60 * MIN);
    expect(s.map((x) => x.status)).toEqual(["running", "done"]);
  });
});

describe("the window", () => {
  it("spans the zoom, or since the oldest lane began", () => {
    expect(windowSpan("4h", [], NOW)).toBe(240 * MIN);
    expect(windowSpan("start", [ago(130), ago(14)], NOW)).toBe(130 * MIN);
    expect(windowSpan("start", [ago(1)], NOW)).toBe(5 * MIN);
  });

  it("labels its quarters on the clock", () => {
    expect(ticks(NOW, 60 * MIN).map((t) => t.label)).toEqual(["10:55", "11:10", "11:25", "11:40"]);
  });

  it("starts a lane at its first agent status", () => {
    expect(laneStart([{ status: null, at: 1 }, { status: "done", at: 5 }])).toBe(5);
    expect(laneStart([])).toBeNull();
  });
});

describe("lane order", () => {
  const lane = (key: string, kind: Orderable["kind"], since: number, order: number): Orderable => ({ key, kind, since, order });

  it("puts the longest wait first, then working, then idle", () => {
    const sorted = sortLanes([
      lane("idle", "idle", ago(8), 0),
      lane("work", "working", ago(14), 1),
      lane("new-wait", "waiting", ago(2), 2),
      lane("old-wait", "waiting", ago(5), 3),
      lane("work2", "working", ago(1), 4),
    ]);
    expect(sorted.map((l) => l.key)).toEqual(["old-wait", "new-wait", "work", "work2", "idle"]);
  });

  it("holds the order while the pointer is over the lanes", () => {
    expect(holdOrder(["a", "b", "c"], ["c", "a", "d"], true)).toEqual(["a", "c", "d"]);
    expect(holdOrder(["a", "b", "c"], ["c", "a", "d"], false)).toEqual(["c", "a", "d"]);
  });
});

describe("durations", () => {
  it("reads short", () => {
    expect(formatDuration(20_000)).toBe("<1m");
    expect(formatDuration(14 * MIN)).toBe("14m");
    expect(formatDuration(130 * MIN)).toBe("2h 10m");
    expect(formatDuration(180 * MIN)).toBe("3h");
  });
});

describe("turnSummary", () => {
  const e = (kind: AgentEvent["kind"], text: string): AgentEvent => ({ at: 0, sessionId: 1, kind, text });

  it("counts the files and lines changed since the last prompt", () => {
    const events = [
      e("edit", "Updated old.rs (+100)"),
      e("started", "Started “docs sweep”"),
      e("edit", "Updated README.md (+40 −7)"),
      e("read", "Read src/a.ts"),
      e("edit", "Wrote docs/new.md (+100)"),
      e("edit", "Updated README.md (−30)"),
    ];
    expect(turnSummary(events)).toBe("2 files changed, +140 −37");
  });

  it("is null when nothing was edited", () => {
    expect(turnSummary([e("started", "Started “x”"), e("command", "Ran ls")])).toBeNull();
  });
});

describe("lastLines", () => {
  it("keeps the last non-empty lines", () => {
    expect(lastLines(["a", "", "b   ", "  ", "c", ""], 2)).toEqual(["b", "c"]);
  });
});

describe("lastPrompt", () => {
  it("reads the last Started event", () => {
    const e = (kind: AgentEvent["kind"], text: string): AgentEvent => ({ at: 0, sessionId: 1, kind, text });
    expect(lastPrompt([e("started", "Started “one”"), e("edit", "Updated a"), e("started", "Started “two words”")])).toBe("two words");
    expect(lastPrompt([e("edit", "Updated a")])).toBeNull();
  });
});
