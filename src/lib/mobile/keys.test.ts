import { describe, expect, it } from "vitest";
import { LAYERS, NO_MODS, SYMBOL_ROW, TOP_ROW, labelOf, press, withCtrl, type Key, type Layer, type Mods } from "./keys";

function key(layer: Layer | "top", id: string): Key {
  const rows = layer === "top" ? [TOP_ROW] : LAYERS[layer];
  const found = rows.flat().find((k) => k.id === id);
  if (!found) throw new Error(`no key ${id} on ${layer}`);
  return found;
}

const mods = (m: Partial<Mods>): Mods => ({ ...NO_MODS, ...m });

describe("the layers", () => {
  it("fill the keyboard's width, row by row, with keys that differ", () => {
    for (const rows of [...Object.values(LAYERS), [TOP_ROW, SYMBOL_ROW]]) {
      for (const row of rows) {
        const units = row.reduce((n, k) => n + (k.width ?? 1), 0);
        expect(units).toBeLessThanOrEqual(10);
        expect(units).toBeGreaterThanOrEqual(9);
        expect(new Set(row.map((k) => k.id)).size).toBe(row.length);
      }
    }
  });

  it("have every printable ASCII character somewhere", () => {
    const typed = new Set<string>();
    for (const k of [...Object.values(LAYERS).flat(2), ...SYMBOL_ROW]) {
      if (k.action.kind !== "text") continue;
      typed.add(k.action.text);
      typed.add(k.action.text.toUpperCase());
    }
    for (let code = 32; code < 127; code++) expect(typed, JSON.stringify(String.fromCharCode(code))).toContain(String.fromCharCode(code));
  });
});

describe("press", () => {
  it("types a letter, and lets go of a Shift held for one key", () => {
    expect(press(key("letters", "a"), NO_MODS, "letters")).toEqual({ send: "a", mods: NO_MODS, layer: "letters", paste: false });
    expect(press(key("letters", "a"), mods({ shift: "once" }), "letters")).toMatchObject({ send: "A", mods: NO_MODS });
    expect(press(key("letters", "a"), mods({ shift: "lock" }), "letters")).toMatchObject({ send: "A", mods: mods({ shift: "lock" }) });
  });

  it("Shift holds for a key, locks when tapped again at once, and lets go", () => {
    const shift = key("letters", "shift");
    expect(press(shift, NO_MODS, "letters").mods.shift).toBe("once");
    expect(press(shift, mods({ shift: "once" }), "letters", false, true).mods.shift).toBe("lock");
    expect(press(shift, mods({ shift: "once" }), "letters").mods.shift).toBe("off");
    expect(press(shift, mods({ shift: "lock" }), "letters", false, true).mods.shift).toBe("off");
  });

  it("Ctrl and Alt arm for the next key", () => {
    expect(press(key("top", "ctrl"), NO_MODS, "letters")).toMatchObject({ send: null, mods: mods({ ctrl: true }) });
    expect(press(key("letters", "c"), mods({ ctrl: true }), "letters")).toMatchObject({ send: "\x03", mods: NO_MODS });
    expect(press(key("letters", "b"), mods({ alt: true }), "letters")).toMatchObject({ send: "\x1bb", mods: NO_MODS });
    expect(press(key("top", "ctrl"), mods({ ctrl: true }), "letters").mods.ctrl).toBe(false);
  });

  it("changes layer without letting go of the modifiers", () => {
    expect(press(key("letters", "to-numbers"), mods({ ctrl: true }), "letters")).toEqual({
      send: null,
      mods: mods({ ctrl: true }),
      layer: "numbers",
      paste: false,
    });
    expect(press(key("numbers", "to-symbols"), NO_MODS, "numbers").layer).toBe("symbols");
    expect(press(key("symbols", "to-letters"), NO_MODS, "symbols").layer).toBe("letters");
  });

  it("sends the named keys as a terminal expects them", () => {
    expect(press(key("top", "esc"), NO_MODS, "letters").send).toBe("\x1b");
    expect(press(key("top", "tab"), NO_MODS, "letters").send).toBe("\t");
    expect(press(key("top", "tab"), mods({ shift: "once" }), "letters").send).toBe("\x1b[Z");
    expect(press(key("top", "shift-tab"), NO_MODS, "letters").send).toBe("\x1b[Z");
    expect(press(key("top", "ctrl-c"), NO_MODS, "letters").send).toBe("\x03");
    expect(press(key("letters", "return"), NO_MODS, "letters").send).toBe("\r");
    expect(press(key("letters", "return"), mods({ shift: "once" }), "letters").send).toBe("\x1b\r");
    expect(press(key("letters", "return"), mods({ alt: true }), "letters").send).toBe("\x1b\r");
    expect(press(key("letters", "backspace"), NO_MODS, "letters").send).toBe("\x7f");
    expect(press(key("letters", "backspace"), mods({ alt: true }), "letters").send).toBe("\x1b\x7f");
    expect(press(key("symbols", "delete"), NO_MODS, "symbols").send).toBe("\x1b[3~");
    expect(press(key("symbols", "pageup"), NO_MODS, "symbols").send).toBe("\x1b[5~");
    expect(press(key("symbols", "pagedown"), mods({ ctrl: true }), "symbols").send).toBe("\x1b[6;5~");
  });

  it("sends arrows by DECCKM when plain, and by the modifier otherwise", () => {
    expect(press(key("top", "up"), NO_MODS, "letters", false).send).toBe("\x1b[A");
    expect(press(key("top", "up"), NO_MODS, "letters", true).send).toBe("\x1bOA");
    expect(press(key("top", "left"), mods({ alt: true }), "letters", true).send).toBe("\x1b[1;3D");
    expect(press(key("top", "right"), mods({ ctrl: true, shift: "once" }), "letters").send).toBe("\x1b[1;6C");
    expect(press(key("symbols", "home"), NO_MODS, "symbols", true).send).toBe("\x1bOH");
    expect(press(key("symbols", "end"), NO_MODS, "symbols").send).toBe("\x1b[F");
  });

  it("asks for a paste, and sends nothing itself", () => {
    expect(press(key("letters", "paste"), mods({ shift: "once" }), "letters")).toEqual({ send: null, mods: NO_MODS, layer: "letters", paste: true });
  });
});

describe("withCtrl", () => {
  it("gives the control code of a letter or of @ [ \\ ] ^ _, and passes the rest", () => {
    expect(withCtrl("a")).toBe("\x01");
    expect(withCtrl("Z")).toBe("\x1a");
    expect(withCtrl("[")).toBe("\x1b");
    expect(withCtrl(" ")).toBe("\x00");
    expect(withCtrl("?")).toBe("\x7f");
    expect(withCtrl("1")).toBe("1");
    expect(withCtrl("ab")).toBe("ab");
  });
});

describe("labelOf", () => {
  it("shows a letter as Shift will type it", () => {
    expect(labelOf(key("letters", "a"), NO_MODS)).toBe("a");
    expect(labelOf(key("letters", "a"), mods({ shift: "once" }))).toBe("A");
    expect(labelOf(key("letters", "space"), mods({ shift: "lock" }))).toBe("space");
    expect(labelOf(key("top", "esc"), mods({ shift: "once" }))).toBe("esc");
  });
});
