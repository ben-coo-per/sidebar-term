import { describe, expect, it } from "vitest";
import { agentLabel, computeAutomaticTitle, type AutomaticTitleInput } from "./agentStatus";

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

  it("names an agent by its project and what it is at, above everything else", () => {
    const git = { repoName: "jack", commonDir: "/j/.git", worktreeRoot: "/j", worktreeName: null, branch: "main", headShort: null };
    expect(
      computeAutomaticTitle({
        ...base,
        agent: "claude",
        git,
        oscTitle: "◐ Fix the pairing handshake",
        foreground: "claude",
        shellIsForeground: false,
        cwd: "/Users/you/Dev/jack",
      }),
    ).toBe("jack · fix the pairing");
    // Codex's title carries no subject: the project alone.
    expect(
      computeAutomaticTitle({
        ...base,
        agent: "codex",
        oscTitle: "⠋ jack",
        foreground: "codex",
        shellIsForeground: false,
        cwd: "/Users/you/Dev/jack",
      }),
    ).toBe("jack");
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

describe("agentLabel", () => {
  const git = (repoName: string, worktreeName: string | null = null) => ({
    repoName,
    commonDir: `/r/${repoName}/.git`,
    worktreeRoot: `/r/${repoName}`,
    worktreeName,
    branch: "main",
    headShort: null,
  });
  const base = { customTitle: null, git: null, cwd: null, home: "/Users/you", oscTitle: null, agent: "claude" as const, lastPrompt: null };

  it("names the project, and takes Claude Code's own summary from its title", () => {
    expect(agentLabel({ ...base, git: git("sidebar-term"), oscTitle: "◐ Fix the badge colours in the sidebar" })).toEqual({
      project: "sidebar-term",
      description: "fix the badge",
    });
    expect(agentLabel({ ...base, git: git("jack", "navbar"), oscTitle: "✳ Refactor moveTabToHost." })).toEqual({
      project: "jack/navbar",
      description: "refactor moveTabToHost",
    });
  });

  it("falls back to the last prompt, then to nothing", () => {
    expect(agentLabel({ ...base, agent: "codex", git: git("api"), oscTitle: "⠋ api", lastPrompt: "Move the API to Postgres 17" })).toEqual({
      project: "api",
      description: "move the API",
    });
    expect(agentLabel({ ...base, agent: "gemini", cwd: "/Users/you/notes", oscTitle: "◇ Ready (notes)" })).toEqual({
      project: "notes",
      description: null,
    });
  });

  it("says nothing that only repeats the project or the agent", () => {
    expect(agentLabel({ ...base, git: git("jig"), oscTitle: "✳ Claude Code" }).description).toBeNull();
    expect(agentLabel({ ...base, git: git("jig"), oscTitle: "◐ jig" }).description).toBeNull();
  });

  it("keeps a rename as it is", () => {
    expect(agentLabel({ ...base, customTitle: "auth refactor", git: git("jig"), oscTitle: "◐ Other" })).toEqual({
      project: "auth refactor",
      description: null,
    });
  });

  it("uses the folder, ~ for home, or the agent's name", () => {
    expect(agentLabel({ ...base, cwd: "/Users/you" }).project).toBe("~");
    expect(agentLabel({ ...base, cwd: "/tmp/scratch/" }).project).toBe("scratch");
    expect(agentLabel(base).project).toBe("Claude Code");
  });
});
