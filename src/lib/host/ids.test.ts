import { describe, expect, it } from "vitest";
import { isLocal, LOCAL_HOST, parseSessionKey, sessionKey } from "./ids";

describe("Session keys", () => {
  it("name a Session by its Host and id, and read back", () => {
    expect(sessionKey(LOCAL_HOST, 3)).toBe("local/3");
    expect(parseSessionKey("local/3")).toEqual({ host: "local", id: 3 });
    expect(parseSessionKey(sessionKey("h_ab12", 0))).toEqual({ host: "h_ab12", id: 0 });
    // Two Hosts' Session 1 are two keys.
    expect(sessionKey(LOCAL_HOST, 1)).not.toBe(sessionKey("h_ab12", 1));
  });

  it("refuse what is not a key", () => {
    expect(parseSessionKey("3")).toBeNull();
    expect(parseSessionKey("/3")).toBeNull();
    expect(parseSessionKey("local/x")).toBeNull();
    expect(parseSessionKey("local/-1")).toBeNull();
    expect(parseSessionKey("local/1.5")).toBeNull();
  });

  it("know the local Host", () => {
    expect(isLocal(LOCAL_HOST)).toBe(true);
    expect(isLocal("h_1")).toBe(false);
  });
});
