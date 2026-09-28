import { describe, expect, it } from "vitest";
import { decodeOutputFrame, encodeOutputFrame, isReply, parseServerMessage, Replies } from "./protocol";

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
    expect(parseServerMessage('{"t":"session","session":{"sessionId":1,"status":"running"}}')).toMatchObject({
      t: "session",
      session: { status: "running" },
    });
    expect(parseServerMessage("nope")).toBeNull();
    expect(parseServerMessage('{"x":1}')).toBeNull();
    expect(parseServerMessage("null")).toBeNull();
    expect(parseServerMessage("[]")).toBeNull();
  });

  it("tells a reply from an error that answers nothing", () => {
    expect(isReply({ t: "ok", id: 3 })).toBe(true);
    expect(isReply({ t: "error", id: 3, message: "no Tab" })).toBe(true);
    expect(isReply({ t: "error", message: "bad message" })).toBe(false);
    expect(isReply({ t: "pong" })).toBe(false);
  });
});

describe("Replies", () => {
  it("hands out fresh ids and settles each command by its reply", async () => {
    const r = new Replies();
    const a = r.open();
    const b = r.open();
    expect(a.id).not.toBe(b.id);
    expect(r.size).toBe(2);
    expect(r.settle({ t: "ok", id: b.id, result: { id: "t1", groupId: "g", sessionId: 5, customTitle: null, lastCwd: null } })).toBe(true);
    await expect(b.reply).resolves.toMatchObject({ id: "t1" });
    expect(r.settle({ t: "error", id: a.id, message: "no Tab x" })).toBe(true);
    await expect(a.reply).rejects.toThrow("no Tab x");
    expect(r.size).toBe(0);
    expect(r.settle({ t: "ok", id: 99 })).toBe(false);
  });

  it("rejects everything pending when the connection fails", async () => {
    const r = new Replies();
    const a = r.open();
    const b = r.open();
    r.fail("dropped");
    await expect(a.reply).rejects.toThrow("dropped");
    await expect(b.reply).rejects.toThrow("dropped");
    expect(r.size).toBe(0);
  });
});
