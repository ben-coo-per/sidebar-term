import { describe, expect, it } from "vitest";
import { codeFromPairingLink, newHostId, normalizeHostUrl, parseHostsSection, webSocketUrl } from "./settings";

describe("normalizeHostUrl", () => {
  it("takes an origin, a bare host name, the phone page's URL, or a pasted trailing slash", () => {
    expect(normalizeHostUrl("https://dell.tail1234.ts.net")).toBe("https://dell.tail1234.ts.net");
    expect(normalizeHostUrl("dell.tail1234.ts.net")).toBe("https://dell.tail1234.ts.net");
    expect(normalizeHostUrl("  http://127.0.0.1:47699/  ")).toBe("http://127.0.0.1:47699");
    expect(normalizeHostUrl("https://dell.tail1234.ts.net/m")).toBe("https://dell.tail1234.ts.net");
    expect(normalizeHostUrl("http://127.0.0.1:47699/m#pair=ABCDEFGH")).toBe("http://127.0.0.1:47699");
    // The browser mock's fake Host.
    expect(normalizeHostUrl("mock://dell/")).toBe("mock://dell");
  });

  it("refuses what is not a Host's server", () => {
    expect(normalizeHostUrl("")).toBeNull();
    expect(normalizeHostUrl("ftp://dell")).toBeNull();
    expect(normalizeHostUrl("https://dell/other")).toBeNull();
    expect(normalizeHostUrl("https://user:pw@dell")).toBeNull();
    expect(normalizeHostUrl("http://")).toBeNull();
  });

  it("reads the code out of a pasted pairing link", () => {
    expect(codeFromPairingLink("http://127.0.0.1:47699/m#pair=ABCDEFGH")).toBe("ABCDEFGH");
    expect(codeFromPairingLink("http://127.0.0.1:47699")).toBeNull();
  });
});

describe("webSocketUrl", () => {
  it("is /ws on the same origin, secure when the Host is", () => {
    expect(webSocketUrl("https://dell.tail1234.ts.net")).toBe("wss://dell.tail1234.ts.net/ws");
    expect(webSocketUrl("http://127.0.0.1:47699")).toBe("ws://127.0.0.1:47699/ws");
  });
});

describe("the hosts section", () => {
  it("parses what is whole and drops the rest", () => {
    const parsed = parseHostsSection([
      { id: "h_1", url: "http://127.0.0.1:47699/", token: "t1", name: "dell" },
      { id: "h_2", url: "dell.tail1234.ts.net", token: "t2" },
      { id: "h_1", url: "http://dup", token: "t3" },
      { id: "h_3", url: "http://no-token" },
      { id: "", url: "http://no-id", token: "t" },
      { id: "h_4", url: "nope://x", token: "t" },
      "junk",
      null,
    ]);
    expect(parsed).toEqual([
      { id: "h_1", url: "http://127.0.0.1:47699", token: "t1", name: "dell" },
      { id: "h_2", url: "https://dell.tail1234.ts.net", token: "t2", name: null },
    ]);
    expect(parseHostsSection(undefined)).toEqual([]);
    expect(parseHostsSection({ hosts: [] })).toEqual([]);
  });

  it("makes distinct ids", () => {
    const a = newHostId();
    expect(a).toMatch(/^h_[0-9a-f]{16}$/);
    expect(newHostId()).not.toBe(a);
  });
});
