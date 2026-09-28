import { describe, expect, it } from "vitest";
import { LAYERS, NO_MODS, SYMBOL_ROW, TOP_ROW, keyAt, labelOf, press, withCtrl, type DrawnRow, type Key, type Layer, type Mods } from "./keys";

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

describe("keyAt", () => {
  // Two rows 42 high and 5 apart, keys 34 wide and 5 apart; the second row starts 19 in.
  const row = (top: number, left: number, keys: string): DrawnRow<string> => ({
    top,
    bottom: top + 42,
    keys: [...keys].map((key, i) => ({ left: left + i * 39, right: left + i * 39 + 34, key })),
  });
  const rows = [row(100, 3, "qwe"), row(147, 22, "as")];

  it("is the key under the finger", () => {
    expect(keyAt(20, 120, rows)).toBe("q");
    expect(keyAt(45, 101, rows)).toBe("w");
    expect(keyAt(70, 180, rows)).toBe("s");
  });

  it("is the nearer key, between two", () => {
    expect(keyAt(38, 120, rows)).toBe("q");
    expect(keyAt(41, 120, rows)).toBe("w");
  });

  it("is a key of the nearer row, between two rows", () => {
    expect(keyAt(30, 143, rows)).toBe("q");
    expect(keyAt(30, 146, rows)).toBe("a");
  });

  it("is the key at the end of its row, off that end", () => {
    expect(keyAt(1, 170, rows)).toBe("a");
    expect(keyAt(380, 170, rows)).toBe("s");
    expect(keyAt(380, 120, rows)).toBe("e");
  });

  it("is a key of the top or bottom row just off it, and none further away", () => {
    expect(keyAt(20, 95, rows)).toBe("q");
    expect(keyAt(40, 195, rows)).toBe("a");
    expect(keyAt(20, 80, rows)).toBeNull();
    expect(keyAt(40, 210, rows)).toBeNull();
  });

  it("passes over a row that is not drawn", () => {
    const hidden: DrawnRow<string> = { top: 0, bottom: 0, keys: [{ left: 0, right: 0, key: "~" }] };
    expect(keyAt(20, 95, [hidden, ...rows])).toBe("q");
    expect(keyAt(20, 95, [hidden])).toBeNull();
    expect(keyAt(20, 95, [])).toBeNull();
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
