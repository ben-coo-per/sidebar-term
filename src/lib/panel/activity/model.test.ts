import { describe, expect, it } from "vitest";
import type { ActivityProcess, ActivitySnapshot } from "../../types";
import {
  formatBytes,
  formatCpu,
  formatTabStats,
  machineCpu,
  meterSegments,
  otherMemoryParts,
  parseActivitySection,
  sortProcesses,
} from "./model";

const MB = 1024 * 1024;
const GB = 1024 * MB;

function proc(pid: number, name: string, cpu: number, mem: number, sessionId: number | null = null): ActivityProcess {
  return { pid, name, cpu, mem, sessionId };
}

function snapshot(partial: Partial<ActivitySnapshot>): ActivitySnapshot {
  return {
    cpuCount: 10,
    cpuTotal: 0,
    memUsed: 0,
    memWired: 0,
    memCompressed: 0,
    memTotal: 32 * GB,
    sessions: [],
    processes: [],
    ...partial,
  };
}

describe("sortProcesses", () => {
  const list = [
    proc(1, "launchd", 0, 18 * MB),
    proc(2, "zsh", 0, 3 * MB, 1),
    proc(3, "WindowServer", 32.8, 400 * MB),
    proc(4, "cargo", 145, 1 * GB, 2),
    proc(5, "Finder", 0, 140 * MB),
  ];

  it("puts the busiest first, and Session processes before others at the same load", () => {
    expect(sortProcesses(list, "cpu").map((p) => p.name)).toEqual(["cargo", "WindowServer", "zsh", "Finder", "launchd"]);
  });

  it("sorts by memory", () => {
    expect(sortProcesses(list, "mem").map((p) => p.name)).toEqual(["cargo", "WindowServer", "Finder", "launchd", "zsh"]);
  });

  it("does not mutate its input", () => {
    const before = list.map((p) => p.pid);
    sortProcesses(list, "cpu");
    expect(list.map((p) => p.pid)).toEqual(before);
  });
});

describe("formatting", () => {
  it.each([
    [0, "0.0"],
    [3.44, "3.4"],
    [99.94, "99.9"],
    [99.96, "100"],
    [145.2, "145"],
  ])("formatCpu(%s) = %s", (pct, want) => {
    expect(formatCpu(pct)).toBe(want);
  });

  it.each([
    [0, "0 B"],
    [812 * 1024, "812 KB"],
    [3 * MB, "3.0 MB"],
    [41.2 * MB, "41.2 MB"],
    [999 * MB, "999 MB"],
    [1000 * MB, "0.98 GB"],
    [1.21 * GB, "1.21 GB"],
    [14.2 * GB, "14.2 GB"],
  ])("formatBytes(%s) = %s", (bytes, want) => {
    expect(formatBytes(bytes)).toBe(want);
  });

  it("reports machine CPU out of every core", () => {
    expect(machineCpu(snapshot({ cpuCount: 10, cpuTotal: 250 }))).toBe(25);
    expect(machineCpu(snapshot({ cpuCount: 0, cpuTotal: 250 }))).toBe(0);
  });
});

describe("meterSegments", () => {
  const snap = snapshot({
    cpuCount: 10,
    cpuTotal: 400,
    memUsed: 16 * GB,
    memTotal: 32 * GB,
    sessions: [
      { sessionId: 7, cpu: 100, mem: 4 * GB, processes: 3 },
      { sessionId: 3, cpu: 50, mem: 4 * GB, processes: 1 },
      { sessionId: 9, cpu: 0, mem: 0, processes: 1 },
    ],
  });

  it("gives each busy Session a share in Tab order, then everything else", () => {
    expect(meterSegments(snap, "cpu", [3, 7, 9])).toEqual([
      { sessionId: 3, fraction: 0.05 },
      { sessionId: 7, fraction: 0.1 },
      { sessionId: null, fraction: 0.25 },
    ]);
  });

  it("puts Sessions without a Tab last", () => {
    expect(meterSegments(snap, "mem", [3]).map((s) => s.sessionId)).toEqual([3, 7, null]);
  });

  it("splits Memory Used out of physical memory", () => {
    expect(meterSegments(snap, "mem", [7, 3])).toEqual([
      { sessionId: 7, fraction: 0.125 },
      { sessionId: 3, fraction: 0.125 },
      { sessionId: null, fraction: 0.25 },
    ]);
  });

  it("never overflows the meter", () => {
    const over = snapshot({ cpuCount: 1, cpuTotal: 300, sessions: [{ sessionId: 1, cpu: 250, mem: 0, processes: 1 }] });
    const segments = meterSegments(over, "cpu", [1]);
    expect(segments).toEqual([{ sessionId: 1, fraction: 1 }]);
  });

  it("is empty when the whole is unknown", () => {
    expect(meterSegments(snapshot({ memTotal: 0 }), "mem", [])).toEqual([]);
  });
});

describe("parseActivitySection", () => {
  it("shows Tab stats unless turned off", () => {
    expect(parseActivitySection(undefined)).toEqual({ tabStats: true });
    expect(parseActivitySection({ tabStats: false })).toEqual({ tabStats: false });
    expect(parseActivitySection({ tabStats: "no" })).toEqual({ tabStats: true });
  });
});

describe("formatTabStats", () => {
  it("rounds CPU and prints memory as Activity Monitor does", () => {
    expect(formatTabStats(35.4, 1.21 * 1024 ** 3)).toBe("35% · 1.21 GB");
    expect(formatTabStats(0, 4.6 * 1024 ** 2)).toBe("0% · 4.6 MB");
  });
});

describe("otherMemoryParts", () => {

  const snap = (memUsed: number, memWired: number, memCompressed: number, inTabs: number): ActivitySnapshot => ({
    cpuCount: 8,
    cpuTotal: 0,
    memUsed,
    memWired,
    memCompressed,
    memTotal: 8 * GB,
    sessions: [{ sessionId: 1, cpu: 0, mem: inTabs, processes: 1 }],
    processes: [],
  });

  it("splits everything else into wired, compressed and other apps", () => {
    expect(otherMemoryParts(snap(5 * GB, 1.5 * GB, 1.5 * GB, 0.5 * GB))).toEqual({
      wired: 1.5 * GB,
      compressed: 1.5 * GB,
      apps: 1.5 * GB,
    });
  });

  it("never goes negative when the Tabs' footprints overlap the compressor", () => {
    const parts = otherMemoryParts(snap(5 * GB, 1.5 * GB, 2 * GB, 3 * GB));
    expect(parts).toEqual({ wired: 1.5 * GB, compressed: 0.5 * GB, apps: 0 });
  });
});
