import { describe, expect, it } from "vitest";
import { decodeOutputFrame, encodeOutputFrame, parseServerMessage } from "./protocol";

describe("output frames", () => {
  it("round-trip the Session id and the bytes", () => {
    const bytes = new TextEncoder().encode("hello\x1b[31m");
    const frame = encodeOutputFrame(0x01020304, bytes);
    expect(Array.from(frame.subarray(0, 4))).toEqual([1, 2, 3, 4]);
    const decoded = decodeOutputFrame(frame.buffer);
    expect(decoded?.sessionId).toBe(0x01020304);
    expect(new TextDecoder().decode(decoded!.bytes)).toBe("hello\x1b[31m");
  });

  it("decode from a view into a larger buffer, and refuse a short frame", () => {
    const big = new Uint8Array(16);
    big.set([0, 0, 0, 7, 65, 66], 4);
    const view = big.subarray(4, 10);
    expect(decodeOutputFrame(view)).toEqual({ sessionId: 7, bytes: new Uint8Array([65, 66]) });
    expect(decodeOutputFrame(new Uint8Array([0, 0, 1]))).toBeNull();
    expect(decodeOutputFrame(encodeOutputFrame(3, new Uint8Array()))?.bytes.length).toBe(0);
  });
});

describe("parseServerMessage", () => {
  it("accepts a tagged object and nothing else", () => {
    expect(parseServerMessage('{"t":"pong"}')).toEqual({ t: "pong" });
    expect(parseServerMessage('{"t":"attached","sessionId":4,"cols":120,"rows":40}')).toMatchObject({ t: "attached", cols: 120 });
    expect(parseServerMessage("nope")).toBeNull();
    expect(parseServerMessage('{"x":1}')).toBeNull();
    expect(parseServerMessage("null")).toBeNull();
    expect(parseServerMessage("[]")).toBeNull();
  });
});
