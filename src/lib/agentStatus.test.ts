import { describe, expect, it } from "vitest";
import {
  computeAgentStatus,
  computeAutomaticTitle,
  CLAUDE_RUNNING_WINDOW_MS,
  type AutomaticTitleInput,
} from "./agentStatus";

describe("computeAgentStatus", () => {
  it("returns null when there is no agent", () => {
    expect(
      computeAgentStatus({ agent: null, title: "", now: 0, lastActivityAt: null, lastBellAt: null }),
    ).toBeNull();
  });

  describe("codex", () => {
    it("is running while the title starts with a braille spinner frame", () => {
      expect(
        computeAgentStatus({
          agent: "codex",
          title: "⠋ jack",
          now: 0,
          lastActivityAt: null,
          lastBellAt: null,
        }),
      ).toBe("running");
    });

    it("needs input when the title contains Action Required", () => {
      expect(
        computeAgentStatus({
          agent: "codex",
          title: "[ ! ] Action Required",
          now: 0,
          lastActivityAt: null,
          lastBellAt: null,
        }),
      ).toBe("needs-input");
    });

    it("is done once the spinner/prefix is gone", () => {
      expect(
        computeAgentStatus({ agent: "codex", title: "jack", now: 0, lastActivityAt: null, lastBellAt: null }),
      ).toBe("done");
    });

    it("is done on an empty/cleared title", () => {
      expect(
        computeAgentStatus({ agent: "codex", title: "", now: 0, lastActivityAt: null, lastBellAt: null }),
      ).toBe("done");
    });
  });

  describe("gemini", () => {
    it("is running when the title starts with ✦", () => {
      expect(
        computeAgentStatus({
          agent: "gemini",
          title: "✦ Working… (jack)",
          now: 0,
          lastActivityAt: null,
          lastBellAt: null,
        }),
      ).toBe("running");
    });

    it("needs input when the title starts with ✋", () => {
      expect(
        computeAgentStatus({
          agent: "gemini",
          title: "✋ Action Required (jack)",
          now: 0,
          lastActivityAt: null,
          lastBellAt: null,
        }),
      ).toBe("needs-input");
    });

    it("is done when the title starts with ◇", () => {
      expect(
        computeAgentStatus({
          agent: "gemini",
          title: "◇ Ready (jack)",
          now: 0,
          lastActivityAt: null,
          lastBellAt: null,
        }),
      ).toBe("done");
    });

    it("falls back to done for an unrecognised title", () => {
      expect(
        computeAgentStatus({ agent: "gemini", title: "", now: 0, lastActivityAt: null, lastBellAt: null }),
      ).toBe("done");
    });
  });

  describe("claude", () => {
    it("is running within the activity window", () => {
      expect(
        computeAgentStatus({
          agent: "claude",
          title: "",
          now: 1000,
          lastActivityAt: 1000 - (CLAUDE_RUNNING_WINDOW_MS - 1),
          lastBellAt: null,
        }),
      ).toBe("running");
    });

    it("is done once the activity window has elapsed with no bell", () => {
      expect(
        computeAgentStatus({
          agent: "claude",
          title: "",
          now: 10_000,
          lastActivityAt: 10_000 - CLAUDE_RUNNING_WINDOW_MS,
          lastBellAt: null,
        }),
      ).toBe("done");
    });

    it("needs input on a bell that lands after the last activity", () => {
      expect(
        computeAgentStatus({
          agent: "claude",
          title: "",
          now: 10_000,
          lastActivityAt: 5000,
          lastBellAt: 5500,
        }),
      ).toBe("needs-input");
    });

    it("prefers running over a stale bell once new activity arrives", () => {
      expect(
        computeAgentStatus({
          agent: "claude",
          title: "",
          now: 10_000,
          lastActivityAt: 9999,
          lastBellAt: 5500,
        }),
      ).toBe("running");
    });

    it("is done with neither activity nor a bell yet", () => {
      expect(
        computeAgentStatus({ agent: "claude", title: "", now: 0, lastActivityAt: null, lastBellAt: null }),
      ).toBe("done");
    });
  });
});

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
