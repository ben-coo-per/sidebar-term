import { describe, expect, it } from "vitest";
import { resumeInput, shellQuote } from "./model";
import type { ResumeEntry } from "../types";

const entry = (over: Partial<ResumeEntry> = {}): ResumeEntry => ({
  key: "tab_1",
  kind: "claude",
  line: "claude --resume 5b6d103b",
  cwd: "/Users/you/Dev/jack",
  ...over,
});

describe("shellQuote", () => {
  it("leaves plain words bare", () => {
    for (const w of ["npm", "--port=3000", "src/app.py", "user@host:/x", "/Users/you/Dev"]) {
      expect(shellQuote(w)).toBe(w);
    }
  });

  it("single-quotes everything else", () => {
    expect(shellQuote("")).toBe("''");
    expect(shellQuote("/Users/you/My Projects")).toBe("'/Users/you/My Projects'");
    expect(shellQuote("it's")).toBe(`'it'\\''s'`);
    expect(shellQuote("=ls")).toBe("'=ls'");
    expect(shellQuote("$HOME")).toBe("'$HOME'");
  });
});

describe("resumeInput", () => {
  it("types the line on a cleared prompt when the shell is already there", () => {
    expect(resumeInput(entry(), "/Users/you/Dev/jack")).toBe("\x15claude --resume 5b6d103b\r");
  });

  it("changes directory first when the shell is elsewhere", () => {
    expect(resumeInput(entry({ cwd: "/Users/you/My Dev" }), "/Users/you")).toBe(
      "\x15cd -- '/Users/you/My Dev' && claude --resume 5b6d103b\r",
    );
    expect(resumeInput(entry(), null)).toBe("\x15cd -- /Users/you/Dev/jack && claude --resume 5b6d103b\r");
  });

  it("runs where the shell is when the entry has no directory", () => {
    expect(resumeInput(entry({ kind: "command", line: "npm run dev", cwd: null }), "/x")).toBe("\x15npm run dev\r");
  });
});
