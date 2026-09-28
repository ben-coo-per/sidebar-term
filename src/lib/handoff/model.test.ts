import { describe, expect, it } from "vitest";
import {
  cloneTyped,
  freshClaudeLine,
  handoffInput,
  handoffNote,
  landing,
  mapPath,
  moveDetail,
  pushTyped,
  switchLine,
  unpushedSummary,
} from "./model";
import { expandHome, repoPathOnHost } from "../host/settings";
import type { GitStatus, ResumeEntry } from "../types";

const claude: ResumeEntry = { key: "t1", kind: "claude", line: "claude --model opus --resume 5b6d103b", cwd: "/Users/you/Dev/jack" };
const command: ResumeEntry = { key: "t2", kind: "command", line: "npm run dev", cwd: "/Users/you/Dev/jack/web" };
const LEFT = "\x1b[D";

describe("where a repo is on a Host", () => {
  it("takes the override, else the Checkout root, with ~ as the Host's home", () => {
    const host = { checkoutRoot: "~/Dev", repoPaths: { jack: "/srv/jack-main" } };
    expect(repoPathOnHost(host, "jack", "/home/ben")).toBe("/srv/jack-main");
    expect(repoPathOnHost(host, "other", "/home/ben")).toBe("/home/ben/Dev/other");
    expect(repoPathOnHost(host, "other", null)).toBeNull();
    expect(repoPathOnHost({ checkoutRoot: null, repoPaths: {} }, "jack", "/home/ben")).toBeNull();
    expect(repoPathOnHost(host, null, "/home/ben")).toBeNull();
    expect(expandHome("~", "/home/ben/")).toBe("/home/ben/");
    expect(expandHome("/abs", null)).toBe("/abs");
  });
});

describe("landing", () => {
  it("is the checkout when the Host has it, a clone when not, home otherwise", () => {
    expect(landing({ repoPath: "/home/b/Dev/jack", repoExists: true, root: "/home/b/Dev", rootExists: true, home: "/home/b" })).toEqual({
      kind: "checkout",
      cwd: "/home/b/Dev/jack",
    });
    expect(landing({ repoPath: "/home/b/Dev/jack", repoExists: false, root: "/home/b/Dev", rootExists: true, home: "/home/b" })).toEqual({
      kind: "clone",
      cwd: "/home/b/Dev",
      path: "/home/b/Dev/jack",
    });
    expect(landing({ repoPath: "/home/b/Dev/jack", repoExists: false, root: "/home/b/Dev", rootExists: false, home: "/home/b" })).toEqual({
      kind: "clone",
      cwd: "/home/b",
      path: "/home/b/Dev/jack",
    });
    expect(landing({ repoPath: null, repoExists: false, root: null, rootExists: false, home: "/home/b" })).toEqual({ kind: "home", cwd: "/home/b" });
  });
});

describe("mapPath", () => {
  it("puts a path under the Worktree at the same place under the checkout", () => {
    expect(mapPath("/Users/you/Dev/jack", "/Users/you/Dev/jack", "/home/b/Dev/jack")).toBe("/home/b/Dev/jack");
    expect(mapPath("/Users/you/Dev/jack/web/src", "/Users/you/Dev/jack", "/home/b/Dev/jack")).toBe("/home/b/Dev/jack/web/src");
    expect(mapPath("/Users/you/elsewhere", "/Users/you/Dev/jack", "/home/b/Dev/jack")).toBe("/home/b/Dev/jack");
    expect(mapPath("/Users/you/Dev/jackfruit", "/Users/you/Dev/jack", "/home/b/Dev/jack")).toBe("/home/b/Dev/jack");
  });
});

describe("the typed lines", () => {
  it("switch, clone and push are quoted", () => {
    expect(switchLine("feat/x")).toBe("git switch feat/x");
    expect(switchLine("my branch")).toBe("git switch 'my branch'");
    expect(cloneTyped("git@github.com:you/jack.git", "/home/b/Dev/jack")).toBe("git clone git@github.com:you/jack.git /home/b/Dev/jack");
    expect(cloneTyped(null, "/home/b/Dev/jack")).toBe(`git clone  /home/b/Dev/jack${LEFT.repeat(" /home/b/Dev/jack".length)}`);
    expect(pushTyped()).toBe("\x15git push -u origin HEAD");
  });

  it("a fresh claude keeps the flags, drops --resume and quotes the note", () => {
    expect(freshClaudeLine(claude, "note here")).toBe("claude --model opus 'note here'");
    expect(freshClaudeLine({ line: "claude --resume abc" }, "it's")).toBe("claude 'it'\\''s'");
    expect(handoffNote({ repo: "jack", branch: "main", id: "abc", reason: "the upload failed" })).toBe(
      "Handed off from my Mac in jack on main. The Claude Code conversation abc stayed there (the upload failed). Where we left off: ",
    );
    expect(handoffNote({ repo: null, branch: null, id: "abc", reason: "r" })).toMatch(/^Handed off from my Mac\. /);
  });
});

describe("handoffInput", () => {
  const checkout = { kind: "checkout" as const, cwd: "/home/b/Dev/jack" };
  const base = { landing: checkout, localRoot: "/Users/you/Dev/jack", branch: "main", cloneUrl: "git@x:jack.git", conversationMoved: true, note: "n" };

  it("resumes a moved conversation after the switch, at the checkout", () => {
    expect(handoffInput({ ...base, entry: claude })).toBe("\x15git switch main && claude --model opus --resume 5b6d103b\r");
  });

  it("reruns a command at the entry's place under the checkout", () => {
    expect(handoffInput({ ...base, entry: command })).toBe("\x15cd -- /home/b/Dev/jack/web && git switch main && npm run dev\r");
    expect(handoffInput({ ...base, entry: command, branch: null })).toBe("\x15cd -- /home/b/Dev/jack/web && npm run dev\r");
  });

  it("types only the switch when nothing was running, and nothing without a branch", () => {
    expect(handoffInput({ ...base, entry: null })).toBe("\x15git switch main\r");
    expect(handoffInput({ ...base, entry: null, branch: null })).toBe("");
    expect(handoffInput({ ...base, landing: { kind: "home", cwd: "/home/b" }, entry: null })).toBe("");
  });

  it("starts a fresh claude with the note for the user to finish when the conversation stayed", () => {
    expect(handoffInput({ ...base, entry: claude, conversationMoved: false, note: "Where we left off: " })).toBe(
      `\x15git switch main && claude --model opus 'Where we left off: '${LEFT}`,
    );
  });

  it("at home, reruns the line where the shell is", () => {
    expect(handoffInput({ ...base, landing: { kind: "home", cwd: "/home/b" }, entry: command })).toBe("\x15npm run dev\r");
  });

  it("with no checkout on the Host, types the clone with everything after it, not run", () => {
    const clone = { kind: "clone" as const, cwd: "/home/b/Dev", path: "/home/b/Dev/jack" };
    expect(handoffInput({ ...base, landing: clone, entry: command })).toBe(
      "\x15git clone git@x:jack.git /home/b/Dev/jack && cd -- /home/b/Dev/jack && git switch main && npm run dev",
    );
    expect(handoffInput({ ...base, landing: clone, entry: null, branch: null })).toBe("\x15git clone git@x:jack.git /home/b/Dev/jack");
    expect(handoffInput({ ...base, landing: clone, entry: claude, conversationMoved: false, note: "n" })).toBe(
      `\x15git clone git@x:jack.git /home/b/Dev/jack && cd -- /home/b/Dev/jack && git switch main && claude --model opus 'n'${LEFT}`,
    );
  });
});

describe("what the confirmation says", () => {
  const git = (over: Partial<GitStatus>): GitStatus => ({ branch: "main", upstream: "origin/main", ahead: 0, changes: 0, remoteUrl: null, ...over });

  it("names what stays on the Mac", () => {
    expect(unpushedSummary(null)).toBeNull();
    expect(unpushedSummary(git({}))).toBeNull();
    expect(unpushedSummary(git({ changes: 1 }))).toBe("The checkout has 1 uncommitted change; they stay on this Mac.");
    expect(unpushedSummary(git({ changes: 3, ahead: 2 }))).toBe("The checkout has 3 uncommitted changes and 2 unpushed commits; they stay on this Mac.");
    expect(unpushedSummary(git({ branch: "feat", upstream: null }))).toBe("The checkout has a branch never pushed (feat); they stay on this Mac.");
  });

  it("explains the landing, what reruns, the switch and the warning", () => {
    expect(
      moveDetail({ hostName: "dell", landing: { kind: "checkout", cwd: "/home/b/Dev/jack" }, entry: claude, branch: "main", git: git({ ahead: 1 }), mapped: true, inRepo: true }),
    ).toEqual([
      "Lands in /home/b/Dev/jack on dell.",
      "The Claude Code conversation moves with it and is resumed there.",
      "git switch main is typed first.",
      "The checkout has 1 unpushed commit; they stay on this Mac.",
    ]);
    expect(moveDetail({ hostName: "dell", landing: { kind: "clone", cwd: "/home/b", path: "/home/b/Dev/jack" }, entry: command, branch: null, git: null, mapped: true, inRepo: true })).toEqual([
      "dell has no checkout at /home/b/Dev/jack: git clone is typed there for you to run.",
      "Reruns there: npm run dev",
    ]);
    expect(moveDetail({ hostName: "dell", landing: { kind: "home", cwd: null }, entry: null, branch: "main", git: null, mapped: false, inRepo: true })).toEqual([
      "No Checkout root is set for dell (Settings > Hosts): lands in its home.",
      "Nothing is running in it.",
    ]);
    expect(moveDetail({ hostName: "dell", landing: { kind: "home", cwd: null }, entry: null, branch: null, git: null, mapped: false, inRepo: false })[0]).toBe(
      "Not in a repo: lands in dell's home.",
    );
  });
});
