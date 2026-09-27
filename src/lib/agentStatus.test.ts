import { describe, expect, it } from "vitest";
import { computeAutomaticTitle, type AutomaticTitleInput } from "./agentStatus";

// The Agent status rules are the Host's now (src-tauri/core/src/status.rs), their tests with them.

describe("computeAutomaticTitle", () => {
  const base: AutomaticTitleInput = {
    agent: null,
    oscTitle: null,
    foreground: null,
    shellIsForeground: true,
    cwd: null,
    home: null,
  };

  it("prefers the agent name above everything else", () => {
    expect(
      computeAutomaticTitle({
        ...base,
        agent: "codex",
        oscTitle: "⠋ jack",
        foreground: "codex",
        shellIsForeground: false,
        cwd: "/Users/you/Dev/jack",
      }),
    ).toBe("Codex");
  });

  it("falls back to the OSC title when there is no agent", () => {
    expect(
      computeAutomaticTitle({ ...base, oscTitle: "vim: notes.md", foreground: "vim", shellIsForeground: false }),
    ).toBe("vim: notes.md");
  });

  it("ignores the shell's own OSC title while the shell is at its prompt", () => {
    expect(
      computeAutomaticTitle({
        ...base,
        oscTitle: "you@mac:~/Dev/jack",
        shellIsForeground: true,
        cwd: "/Users/you/Dev/jack",
      }),
    ).toBe("jack");
  });

  it("falls back to the foreground process when it is not the shell and there is no title", () => {
    expect(computeAutomaticTitle({ ...base, foreground: "vim", shellIsForeground: false })).toBe("vim");
  });

  it("ignores the foreground process when it is the shell", () => {
    expect(
      computeAutomaticTitle({ ...base, foreground: "zsh", shellIsForeground: true, cwd: "/Users/you/Dev/jack" }),
    ).toBe("jack");
  });

  it("falls back to the cwd basename", () => {
    expect(computeAutomaticTitle({ ...base, cwd: "/Users/you/Dev/jack" })).toBe("jack");
  });

  it("shows ~ for the home directory", () => {
    expect(computeAutomaticTitle({ ...base, cwd: "/Users/you", home: "/Users/you" })).toBe("~");
  });

  it("shows ~ when there is no cwd at all", () => {
    expect(computeAutomaticTitle({ ...base, cwd: null })).toBe("~");
  });

  it("trims a trailing slash before taking the basename", () => {
    expect(computeAutomaticTitle({ ...base, cwd: "/Users/you/Dev/jack/" })).toBe("jack");
  });
});
