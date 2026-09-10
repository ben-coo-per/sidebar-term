import { describe, expect, it } from "vitest";
import { findPaths } from "./fileLinks";

const texts = (line: string) => findPaths(line).map((m) => m.text);

describe("findPaths", () => {
  it("finds relative, home and absolute paths", () => {
    expect(texts("edit src/lib/foo.ts and ~/notes.md then /etc/hosts")).toEqual([
      "src/lib/foo.ts",
      "~/notes.md",
      "/etc/hosts",
    ]);
    expect(texts("./run.sh ../README.md")).toEqual(["./run.sh", "../README.md"]);
  });

  it("finds bare file names with an extension, and dotfiles", () => {
    expect(texts("README.md  src  package.json  .gitignore")).toEqual([
      "README.md",
      "package.json",
      ".gitignore",
    ]);
  });

  it("leaves out words, versions and numbers", () => {
    expect(texts("version 1.2.3 is 2.5x faster, e.g. see Makefile")).toEqual(["e.g"]);
    expect(texts("// a comment ... ~ / . ..")).toEqual([]);
  });

  it("drops line and column suffixes and trailing punctuation", () => {
    expect(texts("src/foo.ts:12:5: error")).toEqual(["src/foo.ts"]);
    expect(texts("at src/foo.ts:12.")).toEqual(["src/foo.ts"]);
    expect(texts("Updated README.md, then src/lib/ipc.ts.")).toEqual(["README.md", "src/lib/ipc.ts"]);
  });

  it("splits on quotes and brackets, as agents print paths", () => {
    expect(texts("⏺ Update(src/lib/terminal/system.ts)")).toEqual(["src/lib/terminal/system.ts"]);
    expect(texts(`open "a b/c.txt" and 'd.md' [e/f.rs] <g.ts> \`h.py\``)).toEqual([
      "b/c.txt",
      "d.md",
      "e/f.rs",
      "g.ts",
      "h.py",
    ]);
    expect(texts("│ src/app.html │")).toEqual(["src/app.html"]);
    expect(texts("--config=vite.config.js")).toEqual(["vite.config.js"]);
  });

  it("leaves URLs to the web-links addon", () => {
    expect(texts("see https://example.com/docs/a.html or file:///tmp/x.txt")).toEqual([]);
  });

  it("reports where each path starts", () => {
    const line = "⏺ Read(src/a.ts) and b/c.md:3";
    for (const m of findPaths(line)) expect(line.slice(m.index, m.index + m.text.length)).toBe(m.text);
    expect(findPaths(line).map((m) => m.index)).toEqual([7, 21]);
  });
});
