import { describe, expect, it } from "vitest";
import { describeStrain } from "./strain";
import type { HostPressure } from "../types";

const G = 1024 ** 3;

function pressure(over: Partial<HostPressure> = {}): HostPressure {
  return { level: "ok", mem: 2 * G, high: 9 * G, max: 10 * G, swap: 0, swapMax: 2 * G, some: 0, full: 0, top: [], ...over };
}

const titles = (sid: number) => (sid === 12 ? "“doorstep”" : null);

describe("describeStrain", () => {
  it("says nothing while the Host is fine", () => {
    expect(describeStrain(null, false, titles)).toBeNull();
    expect(describeStrain(pressure(), false, titles)).toBeNull();
  });

  it("names the Tab using the most memory when the Sessions are out of it", () => {
    const s = describeStrain(
      pressure({ level: "throttled", mem: 9 * G, swap: 2 * G, full: 42.4, top: [{ sessionId: 12, mem: 6 * G, swap: 0 }] }),
      false,
      titles,
    );
    expect(s?.level).toBe("throttled");
    expect(s?.text).toBe(
      "Out of memory for its Sessions: they use 9.00 GB of 9.00 GB, swap full, waiting on memory 42% of the time. Using the most: “doorstep”, 6.00 GB.",
    );
  });

  it("falls back to the Session's id, and notes slowness on top of pressure", () => {
    const s = describeStrain(pressure({ level: "tight", swap: G / 2, top: [{ sessionId: 3, mem: G, swap: 0 }] }), true, titles);
    expect(s?.level).toBe("tight");
    expect(s?.text).toContain("Memory is tight");
    expect(s?.text).toContain("512 MB in swap");
    expect(s?.text).toContain("Using the most: Session 3");
    expect(s?.text).toMatch(/slow to answer\.$/);
  });

  it("calls a Host slow when it answers late with no pressure to explain it", () => {
    expect(describeStrain(null, true, titles)?.level).toBe("slow");
    expect(describeStrain(pressure(), true, titles)?.level).toBe("slow");
  });
});
