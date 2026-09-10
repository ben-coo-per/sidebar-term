import { describe, expect, it } from "vitest";
import {
  ACTIONS,
  actionFor,
  comboError,
  comboFromEvent,
  defaultBindings,
  diffFromDefaults,
  formatCombo,
  normalizeKey,
  parseOverrides,
  resolveBindings,
  type Combo,
} from "./hotkeys";

const combo = (key: string, mods: Partial<Omit<Combo, "key">> = {}): Combo => ({
  key,
  meta: false,
  ctrl: false,
  alt: false,
  shift: false,
  ...mods,
});

const event = (key: string, code: string, mods: Partial<Record<"metaKey" | "ctrlKey" | "altKey" | "shiftKey", boolean>> = {}) => ({
  key,
  code,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});

describe("normalizeKey", () => {
  it("reads shifted punctuation and digits off the physical key", () => {
    expect(normalizeKey({ key: "~", code: "Backquote" })).toBe("`");
    expect(normalizeKey({ key: "{", code: "BracketLeft" })).toBe("[");
    expect(normalizeKey({ key: "!", code: "Digit1" })).toBe("1");
  });

  it("lower-cases letters from `key`, so other layouts keep their letters", () => {
    expect(normalizeKey({ key: "T", code: "KeyT" })).toBe("t");
    expect(normalizeKey({ key: "y", code: "KeyT" })).toBe("y");
  });

  it("keeps named keys", () => {
    expect(normalizeKey({ key: "ArrowUp", code: "ArrowUp" })).toBe("ArrowUp");
    expect(normalizeKey({ key: " ", code: "Space" })).toBe("Space");
  });
});

describe("comboFromEvent", () => {
  it("maps Cmd-Shift-` to the previous-Tab-in-Group default", () => {
    const c = comboFromEvent(event("~", "Backquote", { metaKey: true, shiftKey: true }));
    expect(actionFor(defaultBindings(), c)).toBe("tab.prevInGroup");
  });

  it("treats Ctrl as Cmd off macOS", () => {
    const c = comboFromEvent(event("t", "KeyT", { ctrlKey: true }), false);
    expect(c).toEqual(combo("t", { meta: true }));
  });
});

describe("default bindings", () => {
  it("binds Cmd-1..9 to the Group jumps and Cmd-` to cycling within the Group", () => {
    const b = defaultBindings();
    expect(actionFor(b, combo("1", { meta: true }))).toBe("group.jump.1");
    expect(actionFor(b, combo("9", { meta: true }))).toBe("group.jump.9");
    expect(actionFor(b, combo("`", { meta: true }))).toBe("tab.nextInGroup");
  });

  it("has no duplicate or invalid defaults", () => {
    const seen = new Set<string>();
    for (const a of ACTIONS) {
      if (!a.default) continue;
      expect(comboError(a.default), a.id).toBeNull();
      const key = formatCombo(a.default);
      expect(seen.has(key), `${a.id} duplicates ${key}`).toBe(false);
      seen.add(key);
    }
  });
});

describe("formatCombo", () => {
  it("uses macOS modifier order and glyphs", () => {
    expect(formatCombo(combo("`", { meta: true, shift: true }))).toBe("⇧⌘`");
    expect(formatCombo(combo("ArrowUp", { meta: true, alt: true }))).toBe("⌥⌘↑");
    expect(formatCombo(combo("k", { ctrl: true, alt: true, shift: true, meta: true }))).toBe("⌃⌥⇧⌘K");
    expect(formatCombo(null)).toBe("");
  });
});

describe("comboError", () => {
  it("requires a Cmd, Ctrl or Opt modifier", () => {
    expect(comboError(combo("a"))).not.toBeNull();
    expect(comboError(combo("a", { shift: true }))).not.toBeNull();
    expect(comboError(combo("F5", { alt: true }))).toBeNull();
  });

  it("refuses terminal control keys and the menu bar's reserved combos", () => {
    expect(comboError(combo("c", { ctrl: true }))).toMatch(/control key/);
    expect(comboError(combo("q", { meta: true }))).toMatch(/Quit/);
    expect(comboError(combo("v", { meta: true }))).toMatch(/Paste/);
    expect(comboError(combo("Tab", { ctrl: true }))).toBeNull();
  });
});

describe("overrides", () => {
  it("round-trips through diffFromDefaults and resolveBindings", () => {
    const b = defaultBindings();
    b["tab.nextInGroup"] = combo("ArrowDown", { meta: true });
    b["group.jump.9"] = null;
    const diff = diffFromDefaults(b);
    expect(Object.keys(diff).sort()).toEqual(["group.jump.9", "tab.nextInGroup"]);
    expect(resolveBindings(diff)).toEqual(b);
  });

  it("drops unknown actions and malformed combos from persisted data", () => {
    const parsed = parseOverrides({
      "tab.new": { key: "n", meta: true },
      "tab.close": null,
      "no.such.action": { key: "x", meta: true },
      "tab.next": { key: 5 },
      "tab.prev": "cmd+p",
    });
    expect(parsed).toEqual({ "tab.new": combo("n", { meta: true }), "tab.close": null });
    expect(parseOverrides("garbage")).toEqual({});
  });
});
