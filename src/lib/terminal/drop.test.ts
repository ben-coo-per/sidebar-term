import { describe, expect, it, vi } from "vitest";

const ipc = vi.hoisted(() => ({
  dropPaths: vi.fn<() => Promise<string[]>>(),
  saveDroppedFile: vi.fn<(f: File) => Promise<string>>(),
}));
vi.mock("../ipc", () => ipc);

import { resolveDroppedPaths, shellEscape } from "./drop";

describe("shellEscape", () => {
  it("escapes spaces and shell metacharacters", () => {
    expect(shellEscape("/Users/me/My Shots/a (1)&b.png")).toBe("/Users/me/My\\ Shots/a\\ \\(1\\)\\&b.png");
    expect(shellEscape("/tmp/it's $HOME")).toBe("/tmp/it\\'s\\ \\$HOME");
  });

  it("leaves plain paths and non-ASCII spaces alone", () => {
    expect(shellEscape("/tmp/a-b_c.d@2x+e:f%g.png")).toBe("/tmp/a-b_c.d@2x+e:f%g.png");
    expect(shellEscape("/tmp/10.00.00 AM.png")).toBe("/tmp/10.00.00 AM.png");
  });
});

describe("resolveDroppedPaths", () => {
  const file = (name: string) => new File([new Uint8Array([1])], name);

  it("uses pasteboard paths matched by name, in drop order", async () => {
    ipc.dropPaths.mockResolvedValue(["/a/two.png", "/b/one.png"]);
    await expect(resolveDroppedPaths([file("one.png"), file("two.png")])).resolves.toEqual([
      "/b/one.png",
      "/a/two.png",
    ]);
  });

  it("saves files the pasteboard has no path for", async () => {
    ipc.dropPaths.mockResolvedValue(["/old/drag.txt"]);
    ipc.saveDroppedFile.mockImplementation(async (f) => `/tmp/drops/${f.name}`);
    await expect(resolveDroppedPaths([file("shot.png")])).resolves.toEqual(["/tmp/drops/shot.png"]);
  });

  it("skips a file that could not be saved", async () => {
    ipc.dropPaths.mockResolvedValue([]);
    ipc.saveDroppedFile.mockRejectedValue(new Error("disk full"));
    vi.spyOn(console, "error").mockImplementation(() => {});
    await expect(resolveDroppedPaths([file("x.png")])).resolves.toEqual([]);
  });
});
