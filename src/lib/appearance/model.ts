// Appearance: the colours and fonts the user chose on the Settings page, in place of the defaults
// (src/lib/theme.css for the app, src/lib/terminal/theme.ts for the Terminal). Pure: what can be
// chosen, how the saved `appearance` section is read, and what the choices turn into. The live
// state, the DOM and the Terminals are ./appearance.svelte.ts.

import { DEFAULT_TERMINAL_LOOK, type TerminalLook } from "../terminal/theme";

/** One colour the user can set. */
export interface ColorDef<Id extends string = string> {
  id: Id;
  label: string;
  /** Where the colour shows, when the label alone doesn't say. */
  detail?: string;
}

export interface ColorGroup<Id extends string = string> {
  title: string;
  colors: ColorDef<Id>[];
}

/** The app's colours: each `id` is a token of src/lib/theme.css. */
export const UI_COLOR_GROUPS: ColorGroup[] = [
  {
    title: "Surfaces",
    colors: [
      { id: "--sidebar-bg", label: "Sidebar", detail: "The sidebar, the window bar and the Panel." },
      { id: "--sidebar-bg-raised", label: "Raised", detail: "A row under the pointer, menus, dialogs, buttons." },
      { id: "--sidebar-bg-active", label: "Active", detail: "The Tab in view, a pressed button." },
      { id: "--sidebar-border", label: "Border" },
      { id: "--sidebar-divider", label: "Divider" },
      { id: "--scrollbar-thumb", label: "Scrollbar" },
    ],
  },
  {
    title: "Text",
    colors: [
      { id: "--text-primary", label: "Primary" },
      { id: "--text-secondary", label: "Secondary" },
      { id: "--text-tertiary", label: "Tertiary" },
      { id: "--text-on-accent", label: "On accent", detail: "Text on a button filled with the accent." },
    ],
  },
  {
    title: "Accent",
    colors: [
      { id: "--accent", label: "Accent" },
      { id: "--accent-strong", label: "Accent, strong", detail: "Accent text on a dim accent fill." },
      { id: "--focus-ring", label: "Focus ring" },
    ],
  },
  {
    title: "Agent status",
    colors: [
      { id: "--status-running", label: "Working" },
      { id: "--status-needs-input", label: "Waiting on you" },
      { id: "--status-finished", label: "Finished" },
      { id: "--status-frozen", label: "Frozen", detail: "A Tab Memory Guard froze." },
    ],
  },
  {
    title: "Tab colours",
    colors: [
      { id: "--tab-color-plain", label: "No repo", detail: "A Tab outside any repo." },
      ...Array.from({ length: 8 }, (_, i) => ({ id: `--repo-color-${i}`, label: `Repo ${i + 1}` })),
    ],
  },
  {
    title: "Activity and Usage",
    colors: [
      { id: "--activity-other", label: "Other processes", detail: "Their share of the Activity meters." },
      { id: "--activity-other-text", label: "Other processes, text" },
      { id: "--meter-track", label: "Meter track" },
      { id: "--usage-fill", label: "Usage" },
      { id: "--usage-high", label: "Usage, nearly spent" },
      { id: "--usage-full", label: "Usage, spent" },
    ],
  },
  {
    title: "Diffs, Tray and danger",
    colors: [
      { id: "--diff-add", label: "Added" },
      { id: "--diff-remove", label: "Removed" },
      { id: "--tray-on", label: "Tray toggle, on", detail: "Caffeinate while it keeps the Mac awake." },
      { id: "--danger", label: "Danger", detail: "Errors, and buttons that remove something." },
    ],
  },
];

/** The Terminal's colours the user can set: keys of its xterm theme. */
export type TerminalColorId =
  | "background"
  | "foreground"
  | "cursor"
  | "selectionBackground"
  | "selectionInactiveBackground"
  | AnsiColorId;

export type AnsiColorId =
  | "black"
  | "red"
  | "green"
  | "yellow"
  | "blue"
  | "magenta"
  | "cyan"
  | "white"
  | "brightBlack"
  | "brightRed"
  | "brightGreen"
  | "brightYellow"
  | "brightBlue"
  | "brightMagenta"
  | "brightCyan"
  | "brightWhite";

export const ANSI_COLORS: ColorDef<AnsiColorId>[] = [
  { id: "black", label: "Black" },
  { id: "red", label: "Red" },
  { id: "green", label: "Green" },
  { id: "yellow", label: "Yellow" },
  { id: "blue", label: "Blue" },
  { id: "magenta", label: "Magenta" },
  { id: "cyan", label: "Cyan" },
  { id: "white", label: "White" },
  { id: "brightBlack", label: "Bright black", detail: "Dim text, such as zsh's suggestions." },
  { id: "brightRed", label: "Bright red" },
  { id: "brightGreen", label: "Bright green" },
  { id: "brightYellow", label: "Bright yellow" },
  { id: "brightBlue", label: "Bright blue" },
  { id: "brightMagenta", label: "Bright magenta" },
  { id: "brightCyan", label: "Bright cyan" },
  { id: "brightWhite", label: "Bright white" },
];

export const TERMINAL_COLOR_GROUPS: ColorGroup<TerminalColorId>[] = [
  {
    title: "Terminal",
    colors: [
      { id: "background", label: "Background", detail: "Also behind Manager and the Settings page." },
      { id: "foreground", label: "Text" },
      { id: "cursor", label: "Cursor" },
      { id: "selectionBackground", label: "Selection" },
      { id: "selectionInactiveBackground", label: "Selection, window in the background" },
    ],
  },
  { title: "Terminal palette", colors: ANSI_COLORS },
];

const UI_COLOR_IDS = new Set(UI_COLOR_GROUPS.flatMap((g) => g.colors.map((c) => c.id)));
const TERMINAL_COLOR_IDS = new Set<string>(TERMINAL_COLOR_GROUPS.flatMap((g) => g.colors.map((c) => c.id)));

/** Tokens that are another token at a fixed opacity: they follow it and are not set on their own. */
const TINTS: { id: string; of: string; alpha: string }[] = [
  { id: "--accent-dim", of: "--accent", alpha: "33" },
  { id: "--status-needs-input-dim", of: "--status-needs-input", alpha: "1a" },
  { id: "--status-needs-input-border", of: "--status-needs-input", alpha: "40" },
  { id: "--tray-on-dim", of: "--tray-on", alpha: "26" },
  { id: "--danger-dim", of: "--danger", alpha: "26" },
];

/** The token behind everything that sits on the Terminal's background. */
const TERMINAL_BACKGROUND_TOKEN = "--term-bg";

/** The fonts: a family is what the user typed ("" for the default), sizes are numbers. */
export interface Fonts {
  /** The app's text: the sidebar, the window bar, Manager, Settings. */
  ui: string;
  /** The app's fixed-width text: paths, commands, codes. */
  mono: string;
  terminal: string;
  /** The app's text size, as a multiple of the default. */
  uiScale: number;
  /** Pixels. */
  terminalSize: number;
  /** A multiple of the font's own line height. */
  terminalLineHeight: number;
  /** A CSS weight, 100 to 900. */
  terminalWeight: number;
}

export const FONT_DEFAULTS: Fonts = {
  ui: "",
  mono: "",
  terminal: "",
  uiScale: 1,
  terminalSize: DEFAULT_TERMINAL_LOOK.fontSize,
  terminalLineHeight: DEFAULT_TERMINAL_LOOK.lineHeight,
  terminalWeight: 400,
};

/** Limits of the numeric fonts settings, and the step their controls move by. */
export const FONT_RANGES = {
  uiScale: { min: 0.8, max: 1.4, step: 0.05 },
  terminalSize: { min: 8, max: 32, step: 1 },
  terminalLineHeight: { min: 1, max: 2, step: 0.05 },
  terminalWeight: { min: 100, max: 900, step: 100 },
} as const satisfies Record<string, { min: number; max: number; step: number }>;

export type FontNumber = keyof typeof FONT_RANGES;
export type FontFamily = "ui" | "mono" | "terminal";

/** Families worth offering: what macOS ships, then what people commonly install. */
export const UI_FONT_SUGGESTIONS = ["SF Pro Text", "Helvetica Neue", "Avenir Next", "Inter", "Lucida Grande", "Verdana", "Georgia"];
export const MONO_FONT_SUGGESTIONS = [
  "SF Mono",
  "Menlo",
  "Monaco",
  "Courier New",
  "Andale Mono",
  "JetBrains Mono",
  "Fira Code",
  "Source Code Pro",
  "IBM Plex Mono",
  "Cascadia Code",
  "Hack",
  "Iosevka",
  "Berkeley Mono",
  "MesloLGS NF",
];

/** What the user changed; anything absent is the default. This is also the saved section. */
export interface Appearance {
  /** UI colours by token, e.g. `--accent`. */
  colors: Record<string, string>;
  terminal: Partial<Record<TerminalColorId, string>>;
  fonts: Partial<Fonts>;
}

export function emptyAppearance(): Appearance {
  return { colors: {}, terminal: {}, fonts: {} };
}

export function isCustom(a: Appearance): boolean {
  return [a.colors, a.terminal, a.fonts].some((part) => Object.keys(part).length > 0);
}

/** A colour as typed, as `#rrggbb`: takes `#abc`, `abc`, `#aabbcc`, any case. Null when it is none. */
export function parseColor(text: unknown): string | null {
  if (typeof text !== "string") return null;
  const hex = text.trim().replace(/^#/, "").toLowerCase();
  if (/^[0-9a-f]{3}$/.test(hex)) return "#" + [...hex].map((c) => c + c).join("");
  return /^[0-9a-f]{6}$/.test(hex) ? "#" + hex : null;
}

/** How light a `#rrggbb` colour looks, 0 (black) to 1 (white): its relative luminance. */
export function luminance(color: string): number {
  const channel = (i: number) => {
    const c = parseInt(color.slice(i, i + 2), 16) / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
}

/** Names CSS gives a meaning to, which must stay unquoted. */
const KEYWORD_FAMILIES = new Set([
  "serif",
  "sans-serif",
  "monospace",
  "cursive",
  "fantasy",
  "system-ui",
  "ui-serif",
  "ui-sans-serif",
  "ui-monospace",
  "ui-rounded",
  "-apple-system",
  "blinkmacsystemfont",
]);

/** Whether this is a name CSS resolves itself, and no font that could be installed or missing. */
export function isKeywordFamily(name: string): boolean {
  return KEYWORD_FAMILIES.has(name.toLowerCase());
}

const MAX_FAMILY_LENGTH = 80;

/** The families in a comma-separated list as typed, unquoted; whatever is no font name is dropped. */
export function fontFamilies(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const part of text.split(",")) {
    const name = part
      .trim()
      .replace(/^(["'])(.*)\1$/, "$2")
      .replace(/\s+/g, " ")
      .trim();
    if (!name || name.length > MAX_FAMILY_LENGTH || !/^[\p{L}\p{N} ._-]+$/u.test(name)) continue;
    const key = name.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(name);
  }
  return out;
}

/** The CSS `font-family` for what the user typed: their families, then the default's as the fallback. */
export function fontStack(text: string, fallback: string): string {
  const chosen = fontFamilies(text);
  if (chosen.length === 0) return fallback;
  return fontFamilies(`${chosen.join(",")},${fallback}`)
    .map((name) => (isKeywordFamily(name) ? name : `"${name}"`))
    .join(", ");
}

/** A number within its range, on its step; the default when it is no number. */
export function clampFont(key: FontNumber, value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return FONT_DEFAULTS[key];
  const { min, max, step } = FONT_RANGES[key];
  const stepped = Math.round(value / step) * step;
  return Number(Math.min(max, Math.max(min, stepped)).toFixed(2));
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function parseColors<Id extends string>(raw: unknown, known: Set<string>): Partial<Record<Id, string>> {
  const out: Partial<Record<Id, string>> = {};
  if (!isRecord(raw)) return out;
  for (const [id, value] of Object.entries(raw)) {
    const color = parseColor(value);
    if (color && known.has(id)) out[id as Id] = color;
  }
  return out;
}

/** The `appearance` settings section: what is unknown, malformed or the default is dropped. */
export function parseAppearanceSection(raw: unknown): Appearance {
  const r = isRecord(raw) ? raw : {};
  const f = isRecord(r.fonts) ? r.fonts : {};
  const fonts: Partial<Fonts> = {};
  for (const key of ["ui", "mono", "terminal"] as const) {
    const families = typeof f[key] === "string" ? fontFamilies(f[key]).join(", ") : "";
    if (families) fonts[key] = families;
  }
  for (const key of Object.keys(FONT_RANGES) as FontNumber[]) {
    const value = clampFont(key, f[key]);
    if (value !== FONT_DEFAULTS[key]) fonts[key] = value;
  }
  return {
    colors: parseColors<string>(r.colors, UI_COLOR_IDS) as Record<string, string>,
    terminal: parseColors<TerminalColorId>(r.terminal, TERMINAL_COLOR_IDS),
    fonts,
  };
}

/** The defaults of src/lib/theme.css the choices fall back on, as read from the page. */
export interface UiDefaults {
  /** Every token of `UI_COLOR_GROUPS`, as `#rrggbb`. */
  colors: Record<string, string>;
  fontUi: string;
  fontMono: string;
}

/**
 * Every CSS variable Appearance owns, for the root element: its value, or null where the
 * stylesheet's own value stands.
 */
export function cssVariables(a: Appearance, defaults: UiDefaults): Record<string, string | null> {
  const vars: Record<string, string | null> = {};
  for (const id of UI_COLOR_IDS) vars[id] = a.colors[id] ?? null;
  for (const tint of TINTS) vars[tint.id] = a.colors[tint.of] ? a.colors[tint.of] + tint.alpha : null;
  vars[TERMINAL_BACKGROUND_TOKEN] = a.terminal.background ?? null;
  vars["--font-ui"] = a.fonts.ui ? fontStack(a.fonts.ui, defaults.fontUi) : null;
  vars["--font-mono"] = a.fonts.mono ? fontStack(a.fonts.mono, defaults.fontMono) : null;
  vars["--ui-font-scale"] = a.fonts.uiScale === undefined ? null : String(a.fonts.uiScale);
  return vars;
}

/** Whether the browser's own controls (checkboxes, menus, scrollbars) should be light or dark. */
export function colorScheme(a: Appearance, defaults: UiDefaults): "light" | "dark" {
  const surface = parseColor(a.colors["--sidebar-bg"] ?? defaults.colors["--sidebar-bg"]);
  return surface && luminance(surface) > 0.4 ? "light" : "dark";
}

export function terminalColor(a: Appearance, id: TerminalColorId): string {
  return a.terminal[id] ?? (DEFAULT_TERMINAL_LOOK.theme[id] as string);
}

/** The look of every Terminal under these choices. */
export function terminalLook(a: Appearance): TerminalLook {
  const fonts = { ...FONT_DEFAULTS, ...a.fonts };
  return {
    theme: {
      ...DEFAULT_TERMINAL_LOOK.theme,
      ...a.terminal,
      // The character under a block cursor takes the background's colour.
      cursorAccent: terminalColor(a, "background"),
    },
    fontFamily: fontStack(fonts.terminal, DEFAULT_TERMINAL_LOOK.fontFamily),
    fontSize: fonts.terminalSize,
    lineHeight: fonts.terminalLineHeight,
    fontWeight: String(fonts.terminalWeight) as TerminalLook["fontWeight"],
  };
}
