// Hotkeys: the app actions that can be bound to a key combination, their defaults, and the pure
// rules for reading, matching, formatting and validating combinations. The reactive, persisted
// bindings live in ./hotkeys.svelte.ts; the window listener that dispatches them in ./shortcuts.ts.

/** One key plus modifiers. `key` is normalised by `normalizeKey` (e.g. "t", "1", "`", "ArrowUp"). */
export interface Combo {
  key: string;
  meta: boolean;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
}

export const GROUP_JUMP_COUNT = 9;

export type GroupJumpAction = `group.jump.${1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9}`;

export type ActionId =
  | "tab.new"
  | "tab.close"
  | "tab.next"
  | "tab.prev"
  | "tab.nextInGroup"
  | "tab.prevInGroup"
  | "tab.moveUp"
  | "tab.moveDown"
  | "tab.markUnread"
  | "group.new"
  | GroupJumpAction
  | "sidebar.toggle"
  | "settings.toggle";

export type ActionCategory = "Tabs" | "Groups" | "App";

export interface ActionDef {
  id: ActionId;
  label: string;
  category: ActionCategory;
  default: Combo | null;
}

export type Bindings = Record<ActionId, Combo | null>;

function cmd(key: string, mods: Partial<Omit<Combo, "key" | "meta">> = {}): Combo {
  return { key, meta: true, ctrl: false, alt: false, shift: false, ...mods };
}

export function groupJumpAction(n: number): GroupJumpAction {
  return `group.jump.${n}` as GroupJumpAction;
}

export const ACTIONS: readonly ActionDef[] = [
  { id: "tab.new", label: "New Tab", category: "Tabs", default: cmd("t") },
  { id: "tab.close", label: "Close Tab", category: "Tabs", default: cmd("w") },
  { id: "tab.nextInGroup", label: "Next Tab in Group", category: "Tabs", default: cmd("`") },
  { id: "tab.prevInGroup", label: "Previous Tab in Group", category: "Tabs", default: cmd("`", { shift: true }) },
  { id: "tab.next", label: "Next Tab", category: "Tabs", default: cmd("]", { shift: true }) },
  { id: "tab.prev", label: "Previous Tab", category: "Tabs", default: cmd("[", { shift: true }) },
  { id: "tab.moveUp", label: "Move Tab Up", category: "Tabs", default: cmd("ArrowUp", { alt: true }) },
  { id: "tab.moveDown", label: "Move Tab Down", category: "Tabs", default: cmd("ArrowDown", { alt: true }) },
  { id: "tab.markUnread", label: "Mark Tab as Unread", category: "Tabs", default: cmd("u", { shift: true }) },
  { id: "group.new", label: "New Group", category: "Groups", default: cmd("n", { shift: true }) },
  ...Array.from({ length: GROUP_JUMP_COUNT }, (_, i): ActionDef => ({
    id: groupJumpAction(i + 1),
    label: `Go to Group ${i + 1}`,
    category: "Groups",
    default: cmd(String(i + 1)),
  })),
  { id: "sidebar.toggle", label: "Toggle Sidebar", category: "App", default: cmd("b") },
  { id: "settings.toggle", label: "Settings", category: "App", default: cmd(",") },
];

const ACTION_IDS = new Set<string>(ACTIONS.map((a) => a.id));

export function isActionId(id: string): id is ActionId {
  return ACTION_IDS.has(id);
}

export function defaultBindings(): Bindings {
  return Object.fromEntries(ACTIONS.map((a) => [a.id, a.default])) as Bindings;
}

/** Defaults with the user's overrides applied. An override of `null` means "unassigned". */
export function resolveBindings(overrides: Partial<Record<ActionId, Combo | null>>): Bindings {
  const out = defaultBindings();
  for (const [id, combo] of Object.entries(overrides)) {
    if (isActionId(id)) out[id] = combo ?? null;
  }
  return out;
}

/** Only the bindings that differ from their default, for persistence. */
export function diffFromDefaults(bindings: Bindings): Partial<Record<ActionId, Combo | null>> {
  const out: Partial<Record<ActionId, Combo | null>> = {};
  for (const a of ACTIONS) {
    if (!combosEqual(bindings[a.id], a.default)) out[a.id] = bindings[a.id];
  }
  return out;
}

// ---------------------------------------------------------------------------------------------
// Reading key events
// ---------------------------------------------------------------------------------------------

const MODIFIER_KEYS = new Set(["Meta", "Control", "Alt", "Shift", "CapsLock", "Fn", "OS"]);

/** Physical keys whose `key` changes with Shift (`!` for 1, `~` for `): read them off `code`. */
const CODE_KEYS: Record<string, string> = {
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
};

/**
 * The key of a KeyboardEvent, independent of Shift: letters lower-cased (from `key`, so non-QWERTY
 * layouts get their own letters), digits and punctuation from the physical `code`.
 */
export function normalizeKey(e: Pick<KeyboardEvent, "key" | "code">): string {
  if (/^[a-zA-Z]$/.test(e.key)) return e.key.toLowerCase();
  const digit = /^(?:Digit|Numpad)(\d)$/.exec(e.code);
  if (digit) return digit[1];
  if (CODE_KEYS[e.code]) return CODE_KEYS[e.code];
  if (e.key === " " || e.code === "Space") return "Space";
  if (e.key.length === 1) return e.key.toLowerCase();
  return e.key;
}

export function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac/.test(navigator.platform ?? navigator.userAgent);
}

export function isModifierOnly(e: Pick<KeyboardEvent, "key">): boolean {
  return MODIFIER_KEYS.has(e.key);
}

/**
 * The Combo a KeyboardEvent represents. Off macOS (dev in a browser) Ctrl stands in for Cmd, so
 * the Cmd-based defaults still work there.
 */
export function comboFromEvent(
  e: Pick<KeyboardEvent, "key" | "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">,
  mac = true,
): Combo {
  return {
    key: normalizeKey(e),
    meta: mac ? e.metaKey : e.ctrlKey,
    ctrl: mac ? e.ctrlKey : e.metaKey,
    alt: e.altKey,
    shift: e.shiftKey,
  };
}

export function combosEqual(a: Combo | null | undefined, b: Combo | null | undefined): boolean {
  if (!a || !b) return !a && !b;
  return a.key === b.key && a.meta === b.meta && a.ctrl === b.ctrl && a.alt === b.alt && a.shift === b.shift;
}

/** The action bound to `combo`, if any. */
export function actionFor(bindings: Bindings, combo: Combo): ActionId | null {
  for (const a of ACTIONS) {
    if (combosEqual(bindings[a.id], combo)) return a.id;
  }
  return null;
}

// ---------------------------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------------------------

const KEY_GLYPHS: Record<string, string> = {
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Enter: "↩",
  Tab: "⇥",
  Backspace: "⌫",
  Delete: "⌦",
  Escape: "⎋",
  Space: "Space",
  PageUp: "⇞",
  PageDown: "⇟",
  Home: "↖",
  End: "↘",
};

export function formatKey(key: string): string {
  if (KEY_GLYPHS[key]) return KEY_GLYPHS[key];
  return key.length === 1 ? key.toUpperCase() : key;
}

/** macOS menu order: ⌃⌥⇧⌘ then the key, e.g. "⌘⇧`". */
export function formatCombo(combo: Combo | null): string {
  if (!combo) return "";
  return (
    (combo.ctrl ? "⌃" : "") +
    (combo.alt ? "⌥" : "") +
    (combo.shift ? "⇧" : "") +
    (combo.meta ? "⌘" : "") +
    formatKey(combo.key)
  );
}

// ---------------------------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------------------------

/** Combos the app must not take: they belong to the menu bar or to copy/paste in the Terminal. */
const RESERVED: { combo: Combo; reason: string }[] = [
  { combo: cmd("q"), reason: "Quit" },
  { combo: cmd("h"), reason: "Hide" },
  { combo: cmd("m"), reason: "Minimize" },
  { combo: cmd("c"), reason: "Copy" },
  { combo: cmd("v"), reason: "Paste" },
  { combo: cmd("x"), reason: "Cut" },
  { combo: cmd("a"), reason: "Select All" },
];

/** Why `combo` can't be a Hotkey, or null when it can. */
export function comboError(combo: Combo): string | null {
  if (!combo.meta && !combo.ctrl && !combo.alt) {
    return "Include ⌘, ⌃ or ⌥ — plain keys belong to the terminal.";
  }
  const controlChar = /^[a-z[\]\\]$/.test(combo.key) || combo.key === "Space";
  if (combo.ctrl && !combo.meta && !combo.alt && !combo.shift && controlChar) {
    return `${formatCombo(combo)} is a terminal control key.`;
  }
  const reserved = RESERVED.find((r) => combosEqual(r.combo, combo));
  if (reserved) return `${formatCombo(combo)} is reserved for ${reserved.reason}.`;
  return null;
}

// ---------------------------------------------------------------------------------------------
// Persistence parsing
// ---------------------------------------------------------------------------------------------

function parseCombo(raw: unknown): Combo | null | undefined {
  if (raw === null) return null;
  if (!raw || typeof raw !== "object") return undefined;
  const r = raw as Record<string, unknown>;
  if (typeof r.key !== "string" || r.key === "") return undefined;
  return { key: r.key, meta: r.meta === true, ctrl: r.ctrl === true, alt: r.alt === true, shift: r.shift === true };
}

/** Defensive parse of persisted overrides: unknown actions and malformed combos are dropped. */
export function parseOverrides(raw: unknown): Partial<Record<ActionId, Combo | null>> {
  const out: Partial<Record<ActionId, Combo | null>> = {};
  if (!raw || typeof raw !== "object") return out;
  for (const [id, value] of Object.entries(raw as Record<string, unknown>)) {
    if (!isActionId(id)) continue;
    const combo = parseCombo(value);
    if (combo !== undefined) out[id] = combo;
  }
  return out;
}
