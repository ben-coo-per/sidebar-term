// How this phone shows a Terminal: the text size, which keyboard, and whether it is up. Kept in
// localStorage, like the pairing: they are this phone's, not a Host's.

import { clampTextSize, DEFAULT_TEXT_SIZE } from "./fit";

const TEXT_SIZE_KEY = "sidebar-term:phone-text-size";
const KEYBOARD_KEY = "sidebar-term:phone-keyboard";
const KEYBOARD_UP_KEY = "sidebar-term:phone-keyboard-up";

/** `keys`: the page's own keyboard (keys.ts). `system`: the phone's, under the key bar. */
export type KeyboardKind = "keys" | "system";

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* private mode: kept for this page */
  }
}

function readTextSize(): number {
  const raw = read(TEXT_SIZE_KEY);
  return raw === null ? DEFAULT_TEXT_SIZE : clampTextSize(Number(raw));
}

export const prefs = $state<{
  textSize: number;
  keyboard: KeyboardKind;
  /** The keyboard is on screen; down, the Terminal has the whole height to read. */
  keyboardUp: boolean;
}>({
  textSize: readTextSize(),
  keyboard: read(KEYBOARD_KEY) === "system" ? "system" : "keys",
  keyboardUp: read(KEYBOARD_UP_KEY) !== "0",
});

export function setTextSize(size: number): void {
  prefs.textSize = clampTextSize(size);
  write(TEXT_SIZE_KEY, String(prefs.textSize));
}

export function setKeyboard(kind: KeyboardKind): void {
  prefs.keyboard = kind;
  write(KEYBOARD_KEY, kind);
}

export function setKeyboardUp(up: boolean): void {
  prefs.keyboardUp = up;
  write(KEYBOARD_UP_KEY, up ? "1" : "0");
}
