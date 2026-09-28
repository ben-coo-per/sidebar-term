import { describe, expect, it } from "vitest";
import { HISTORY_MS, recordStatus, screenOnlyEvent } from "./history";

describe("recordStatus", () => {
  it("records each change and trims to the window", () => {
    let h = recordStatus([], "running", 1000);
    h = recordStatus(h, "running", 2000);
    expect(h).toEqual([{ status: "running", at: 1000 }]);
    h = recordStatus(h, "done", 3000);
    expect(h).toHaveLength(2);
    // Far later: the change in force at the window's start stays, older ones go.
    const later = 3000 + HISTORY_MS + 10;
    h = recordStatus(h, "running", later);
    expect(h).toEqual([
      { status: "done", at: 3000 },
      { status: "running", at: later },
    ]);
  });

  it("starts only once an agent status appears, and ends the lane when the agent leaves", () => {
    expect(recordStatus([], null, 10)).toEqual([]);
    const h = recordStatus(recordStatus([], "needs-input", 20), null, 30);
    expect(h.map((c) => c.status)).toEqual(["needs-input", null]);
  });
});

describe("screenOnlyEvent", () => {
  it("words a status change as the Host does", () => {
    expect(screenOnlyEvent("needs-input")).toEqual({ kind: "asked", text: "Waiting on you (screen only)" });
    expect(screenOnlyEvent(null)).toBeNull();
  });
});
