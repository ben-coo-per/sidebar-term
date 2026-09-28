// The phone's own keyboard for a Terminal: its keys, layer by layer, and what each sends given
// the modifiers held. It stands in for the phone's keyboard, which has no Esc, Tab, Ctrl or
// arrows and corrects what is typed. Pure; `Keyboard.svelte` draws it and tells the Terminal.

export type Layer = "letters" | "numbers" | "symbols";

/** Shift after one tap holds for the next key; tapped twice, until tapped again. */
export type Shift = "off" | "once" | "lock";

export interface Mods {
  shift: Shift;
  /** Armed for the next key. */
  ctrl: boolean;
  alt: boolean;
}

export const NO_MODS: Mods = { shift: "off", ctrl: false, alt: false };

export type Named = "esc" | "tab" | "return" | "backspace" | "delete" | "home" | "end" | "pageup" | "pagedown";
export type Arrow = "up" | "down" | "left" | "right";

export type KeyAction =
  /** A character, which the modifiers change. */
  | { kind: "text"; text: string }
  | { kind: "named"; key: Named }
  | { kind: "arrow"; arrow: Arrow }
  /** A sequence sent as it is, whatever the modifiers. */
  | { kind: "send"; sequence: string }
  | { kind: "shift" }
  | { kind: "ctrl" }
  | { kind: "alt" }
  | { kind: "layer"; to: Layer }
  | { kind: "paste" };

export interface Key {
  /** Unique on its layer. */
  id: string;
  label: string;
  /** What the key is, said aloud. */
  title: string;
  action: KeyAction;
  /** In tenths of the keyboard's width; 1 when left out. */
  width?: number;
  /** Held down, it sends again and again. */
  repeat?: boolean;
  /** Drawn as a key that does something rather than types something. */
  quiet?: boolean;
}

const text = (ch: string, title = ch): Key => ({ id: ch, label: ch, title, action: { kind: "text", text: ch } });
const chars = (row: string): Key[] => [...row].map((ch) => text(ch));
const named = (key: Named, label: string, title: string, more: Partial<Key> = {}): Key => ({
  id: key,
  label,
  title,
  action: { kind: "named", key },
  quiet: true,
  ...more,
});
const arrow = (a: Arrow, label: string): Key => ({
  id: a,
  label,
  title: a[0].toUpperCase() + a.slice(1),
  action: { kind: "arrow", arrow: a },
  repeat: true,
  quiet: true,
});
const layer = (to: Layer, label: string, title: string): Key => ({
  id: `to-${to}`,
  label,
  title,
  action: { kind: "layer", to },
  width: 1.5,
  quiet: true,
});

const BACKSPACE = named("backspace", "⌫", "Backspace", { width: 1.5, repeat: true });

/** The keys a phone keyboard lacks, above every layer. */
export const TOP_ROW: Key[] = [
  named("esc", "esc", "Escape"),
  named("tab", "tab", "Tab"),
  { id: "shift-tab", label: "⇧tab", title: "Shift-Tab", action: { kind: "send", sequence: "\x1b[Z" }, quiet: true },
  { id: "ctrl", label: "ctrl", title: "Control (applies to the next key)", action: { kind: "ctrl" }, quiet: true },
  { id: "alt", label: "alt", title: "Alt (applies to the next key)", action: { kind: "alt" }, quiet: true },
  { id: "ctrl-c", label: "^C", title: "Control-C", action: { kind: "send", sequence: "\x03" }, quiet: true },
  arrow("left", "←"),
  arrow("down", "↓"),
  arrow("up", "↑"),
  arrow("right", "→"),
];

/** What a shell is typed with, one tap away on every layer. */
export const SYMBOL_ROW: Key[] = chars("~/.-_|:\"'*");

const BOTTOM = (to: Layer, label: string, title: string): Key[] => [
  layer(to, label, title),
  { id: "paste", label: "paste", title: "Paste", action: { kind: "paste" }, width: 1.5, quiet: true },
  { id: "space", label: "space", title: "Space", action: { kind: "text", text: " " }, width: 4.5, repeat: true },
  named("return", "⏎", "Return", { width: 2.5 }),
];

const POINTS: Key[] = chars(".,?!'").map((k) => ({ ...k, width: 1.4 }));

export const LAYERS: Record<Layer, Key[][]> = {
  letters: [
    chars("qwertyuiop"),
    chars("asdfghjkl"),
    [{ id: "shift", label: "⇧", title: "Shift", action: { kind: "shift" }, width: 1.5, quiet: true }, ...chars("zxcvbnm"), BACKSPACE],
    BOTTOM("numbers", "123", "Numbers"),
  ],
  numbers: [
    chars("1234567890"),
    chars('-/:;()$&@"'),
    [layer("symbols", "#+=", "Symbols"), ...POINTS, BACKSPACE],
    BOTTOM("letters", "ABC", "Letters"),
  ],
  symbols: [
    chars("[]{}#%^*+="),
    [
      ...chars("_\\|~<>`"),
      named("pageup", "pg↑", "Page Up", { repeat: true }),
      named("pagedown", "pg↓", "Page Down", { repeat: true }),
      named("delete", "del", "Delete", { repeat: true }),
    ],
    [
      layer("numbers", "123", "Numbers"),
      ...chars(".,?").map((k) => ({ ...k, width: 1.4 })),
      named("home", "home", "Home", { width: 1.4 }),
      named("end", "end", "End", { width: 1.4 }),
      BACKSPACE,
    ],
    BOTTOM("letters", "ABC", "Letters"),
  ],
};

/** What a press does: a sequence for the Session, the modifiers and layer after it, or a paste. */
export interface Pressed {
  send: string | null;
  mods: Mods;
  layer: Layer;
  paste: boolean;
}

/** Ctrl-<key>: the control code of a letter, of @ [ \ ] ^ _ or of a space; `?` is DEL; anything else passes through. */
export function withCtrl(data: string): string {
  if (data.length !== 1) return data;
  if (data === " ") return "\x00";
  if (data === "?") return "\x7f";
  const code = data.toUpperCase().charCodeAt(0);
  return code >= 64 && code <= 95 ? String.fromCharCode(code - 64) : data;
}

/** xterm's modifier parameter: 1, plus 1 for Shift, 2 for Alt, 4 for Ctrl. */
function modifier(mods: Mods): number {
  return 1 + (mods.shift !== "off" ? 1 : 0) + (mods.alt ? 2 : 0) + (mods.ctrl ? 4 : 0);
}

const ARROW_LETTER: Record<Arrow, string> = { up: "A", down: "B", right: "C", left: "D" };

/** A cursor key (`letter`: A to D, H, F): DECCKM's form when plain, the modifier's form otherwise. */
function cursorKey(letter: string, mods: Mods, applicationCursor: boolean): string {
  const m = modifier(mods);
  if (m > 1) return `\x1b[1;${m}${letter}`;
  return (applicationCursor ? "\x1bO" : "\x1b[") + letter;
}

/** A key of the `CSI n ~` family. */
function tildeKey(n: number, mods: Mods): string {
  const m = modifier(mods);
  return m > 1 ? `\x1b[${n};${m}~` : `\x1b[${n}~`;
}

function namedKey(key: Named, mods: Mods, applicationCursor: boolean): string {
  const alt = mods.alt ? "\x1b" : "";
  switch (key) {
    case "esc":
      return alt + "\x1b";
    case "tab":
      return mods.shift !== "off" ? "\x1b[Z" : alt + "\t";
    case "return":
      // Shift-Return is a new line where the program tells the two apart (as Alt-Return).
      return mods.shift !== "off" ? "\x1b\r" : alt + "\r";
    case "backspace":
      return alt + (mods.ctrl ? "\x08" : "\x7f");
    case "delete":
      return tildeKey(3, mods);
    case "home":
      return cursorKey("H", mods, applicationCursor);
    case "end":
      return cursorKey("F", mods, applicationCursor);
    case "pageup":
      return tildeKey(5, mods);
    case "pagedown":
      return tildeKey(6, mods);
  }
}

/** The modifiers once a key has used them: those armed for one key are let go. */
function spent(mods: Mods): Mods {
  return { shift: mods.shift === "lock" ? "lock" : "off", ctrl: false, alt: false };
}

/**
 * What pressing `key` does on `layer` with `mods` held. `again`: the key is Shift, tapped a
 * second time at once (it locks). `applicationCursor`: DECCKM is set.
 */
export function press(key: Key, mods: Mods, layer: Layer, applicationCursor = false, again = false): Pressed {
  const a = key.action;
  switch (a.kind) {
    case "shift": {
      const shift: Shift = mods.shift === "off" ? "once" : mods.shift === "once" && again ? "lock" : "off";
      return { send: null, mods: { ...mods, shift }, layer, paste: false };
    }
    case "ctrl":
      return { send: null, mods: { ...mods, ctrl: !mods.ctrl }, layer, paste: false };
    case "alt":
      return { send: null, mods: { ...mods, alt: !mods.alt }, layer, paste: false };
    case "layer":
      return { send: null, mods, layer: a.to, paste: false };
    case "paste":
      return { send: null, mods: spent(mods), layer, paste: true };
    case "send":
      return { send: a.sequence, mods: spent(mods), layer, paste: false };
    case "named":
      return { send: namedKey(a.key, mods, applicationCursor), mods: spent(mods), layer, paste: false };
    case "arrow":
      return { send: cursorKey(ARROW_LETTER[a.arrow], mods, applicationCursor), mods: spent(mods), layer, paste: false };
    case "text": {
      const cased = mods.shift !== "off" ? a.text.toUpperCase() : a.text;
      const send = (mods.alt ? "\x1b" : "") + (mods.ctrl ? withCtrl(cased) : cased);
      return { send, mods: spent(mods), layer, paste: false };
    }
  }
}

/** What a key shows: a letter follows Shift. */
export function labelOf(key: Key, mods: Mods): string {
  return key.action.kind === "text" && key.label.length === 1 && mods.shift !== "off" ? key.label.toUpperCase() : key.label;
}
