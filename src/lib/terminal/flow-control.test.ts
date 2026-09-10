import { describe, expect, it } from "vitest";
import { DEFAULT_HIGH_WATERMARK, DEFAULT_LOW_WATERMARK, FlowController } from "./flow-control";

function setup(high = 100, low = 10) {
  const calls: string[] = [];
  const flow = new FlowController({
    high,
    low,
    onPause: () => calls.push("pause"),
    onResume: () => calls.push("resume"),
  });
  return { flow, calls };
}

describe("FlowController", () => {
  it("uses the research watermarks by default", () => {
    const flow = new FlowController({ onPause() {}, onResume() {} });
    expect(flow.high).toBe(DEFAULT_HIGH_WATERMARK);
    expect(flow.low).toBe(DEFAULT_LOW_WATERMARK);
  });

  it("rejects watermarks without hysteresis", () => {
    const cb = { onPause() {}, onResume() {} };
    expect(() => new FlowController({ ...cb, high: 10, low: 10 })).toThrow(RangeError);
    expect(() => new FlowController({ ...cb, high: 10, low: 20 })).toThrow(RangeError);
    expect(() => new FlowController({ ...cb, high: 10, low: -1 })).toThrow(RangeError);
  });

  it("does not pause at or below the high watermark", () => {
    const { flow, calls } = setup();
    flow.written(60);
    flow.written(40);
    expect(flow.pending).toBe(100);
    expect(flow.paused).toBe(false);
    expect(calls).toEqual([]);
  });

  it("pauses once when pending rises above the high watermark", () => {
    const { flow, calls } = setup();
    flow.written(101);
    flow.written(500);
    flow.written(1);
    expect(flow.paused).toBe(true);
    expect(calls).toEqual(["pause"]);
  });

  it("stays paused between the watermarks and resumes once below low", () => {
    const { flow, calls } = setup();
    flow.written(150);
    flow.processed(100); // 50 pending: still above low
    expect(flow.paused).toBe(true);
    flow.processed(40); // exactly low: not below yet
    expect(flow.paused).toBe(true);
    expect(calls).toEqual(["pause"]);
    flow.processed(1); // 9 pending
    expect(flow.paused).toBe(false);
    flow.processed(9);
    expect(calls).toEqual(["pause", "resume"]);
  });

  it("never resumes without a pause", () => {
    const { flow, calls } = setup();
    flow.written(50);
    flow.processed(50);
    expect(calls).toEqual([]);
  });

  it("does not let pending go negative", () => {
    const { flow } = setup();
    flow.written(5);
    flow.processed(10);
    expect(flow.pending).toBe(0);
    flow.written(5);
    expect(flow.pending).toBe(5);
  });

  it("cycles pause/resume under a sustained flood with async parsing", () => {
    const { flow, calls } = setup(100, 10);
    // A fake terminal: write callbacks fire later, in order, like xterm's WriteBuffer.
    const queue: number[] = [];
    const write = (n: number) => {
      flow.written(n);
      queue.push(n);
    };
    const parseOne = () => flow.processed(queue.shift()!);

    for (let i = 0; i < 5; i++) write(30); // 150 pending: paused after the 4th chunk (120)
    expect(calls).toEqual(["pause"]);
    while (queue.length) parseOne(); // resumes once, when pending drops to 0 (< 10)
    expect(calls).toEqual(["pause", "resume"]);
    for (let i = 0; i < 4; i++) write(30);
    expect(calls).toEqual(["pause", "resume", "pause"]);
    while (queue.length) parseOne();
    expect(calls).toEqual(["pause", "resume", "pause", "resume"]);
    expect(flow.pending).toBe(0);
    expect(flow.paused).toBe(false);
  });
});
