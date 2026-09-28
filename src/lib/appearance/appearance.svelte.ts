// The live Appearance: the user's colours and fonts (./model.ts), set on the root element over
// src/lib/theme.css and given to every Terminal. Saved (the changes only) as the `appearance`
// section of the settings (../settings/store.ts); chosen on the Settings page.

import { loadSection, saveSection } from "../settings/store";
import { terminals } from "../terminal/manager";
import {
  clampFont,
  colorScheme,
  cssVariables,
  emptyAppearance,
  fontFamilies,
  FONT_DEFAULTS,
  parseAppearanceSection,
  parseColor,
  terminalColor,
  terminalLook,
  UI_COLOR_GROUPS,
  type Appearance,
  type FontFamily,
  type FontNumber,
  type TerminalColorId,
  type UiDefaults,
} from "./model";

export const appearance = $state<Appearance>(emptyAppearance());

/** What src/lib/theme.css says, read before anything is set over it. */
export const uiDefaults = $state<UiDefaults>({ colors: {}, fontUi: "", fontMono: "" });

/** A colour picker fires on every move of the pointer: save once it rests. */
const SAVE_DELAY_MS = 400;
let saveTimer: ReturnType<typeof setTimeout> | null = null;

function root(): HTMLElement {
  return document.documentElement;
}

function readDefaults(): void {
  // Anything set over the stylesheet (by a run before a hot reload) would be read as the default.
  for (const name of Object.keys(cssVariables(appearance, uiDefaults))) root().style.removeProperty(name);
  const style = getComputedStyle(root());
  const read = (name: string) => style.getPropertyValue(name).trim();
  for (const group of UI_COLOR_GROUPS) {
    for (const { id } of group.colors) uiDefaults.colors[id] = parseColor(read(id)) ?? "#000000";
  }
  uiDefaults.fontUi = read("--font-ui");
  uiDefaults.fontMono = read("--font-mono");
}

function apply(): void {
  for (const [name, value] of Object.entries(cssVariables(appearance, uiDefaults))) {
    if (value === null) root().style.removeProperty(name);
    else root().style.setProperty(name, value);
  }
  root().style.colorScheme = colorScheme(appearance, uiDefaults);
  terminals.restyle(terminalLook(appearance));
}

function changed(): void {
  apply();
  if (saveTimer !== null) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    saveTimer = null;
    saveSection("appearance", $state.snapshot(appearance));
  }, SAVE_DELAY_MS);
}

export async function initAppearance(): Promise<void> {
  // Awaited first: what follows reads and writes the state, and must not run inside the effect
  // that called, which would then run again on every change.
  const saved = await loadSection("appearance");
  readDefaults();
  Object.assign(appearance, parseAppearanceSection(saved));
  apply();
}

/** Set a UI colour (a token of theme.css); null, or its default, goes back to the default. */
export function setColor(id: string, color: string | null): void {
  if (color === null || color === uiDefaults.colors[id]) delete appearance.colors[id];
  else appearance.colors[id] = color;
  changed();
}

export function setTerminalColor(id: TerminalColorId, color: string | null): void {
  if (color === null || color === terminalColor(emptyAppearance(), id)) delete appearance.terminal[id];
  else appearance.terminal[id] = color;
  changed();
}

/** Set a font to the families typed, comma-separated; "" goes back to the default. */
export function setFontFamily(key: FontFamily, text: string): void {
  const families = fontFamilies(text).join(", ");
  if (families) appearance.fonts[key] = families;
  else delete appearance.fonts[key];
  changed();
}

export function setFontNumber(key: FontNumber, value: number | null): void {
  const clamped = clampFont(key, value);
  if (clamped === FONT_DEFAULTS[key]) delete appearance.fonts[key];
  else appearance.fonts[key] = clamped;
  changed();
}

export function resetColors(): void {
  appearance.colors = {};
  appearance.terminal = {};
  changed();
}

export function resetFonts(): void {
  appearance.fonts = {};
  changed();
}

/**
 * Whether a font of this name is installed: text set in it measures differently from the same
 * text in the fallback alone. Tried against two fallbacks, as the font may match one by chance.
 */
export function fontInstalled(family: string): boolean {
  const ctx = document.createElement("canvas").getContext("2d");
  if (!ctx) return true;
  const sample = "mmmmmmmmmmlliWQ@0O 1234567890";
  const width = (font: string) => {
    ctx.font = `72px ${font}`;
    return ctx.measureText(sample).width;
  };
  return ["monospace", "serif"].some((fallback) => width(`"${family}", ${fallback}`) !== width(fallback));
}
