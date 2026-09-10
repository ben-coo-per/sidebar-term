// Look of every Terminal. v1 is dark only. OWNER: terminal agent.

import type { ITerminalInitOnlyOptions, ITerminalOptions, ITheme } from "@xterm/xterm";

export const TERMINAL_BACKGROUND = "#0f1115";
export const TERMINAL_FOREGROUND = "#d8dee9";

/** Nord-derived palette, brightened a step so it holds contrast on the deeper background. */
export const terminalTheme: ITheme = {
  background: TERMINAL_BACKGROUND,
  foreground: TERMINAL_FOREGROUND,
  cursor: TERMINAL_FOREGROUND,
  cursorAccent: TERMINAL_BACKGROUND,
  selectionBackground: "#34405a",
  selectionInactiveBackground: "#262d3b",

  black: "#3b4252",
  red: "#bf616a",
  green: "#a3be8c",
  yellow: "#ebcb8b",
  blue: "#81a1c1",
  magenta: "#b48ead",
  cyan: "#88c0d0",
  white: "#e5e9f0",

  // Bright black is what zsh autosuggestions and most "dim" UI use: keep it readable (~3.8:1).
  brightBlack: "#616e88",
  brightRed: "#d08088",
  brightGreen: "#b8d0a4",
  brightYellow: "#f0d8a8",
  brightBlue: "#9ab8d8",
  brightMagenta: "#c8a6c2",
  brightCyan: "#9fd0dc",
  brightWhite: "#eceff4",
};

/**
 * "SF Mono" is only installed system-wide when the user added it (macOS ships it inside
 * Terminal.app); `ui-monospace` is WebKit's name for SF Mono, so it keeps the intended face in
 * WKWebView either way. Metrics come from the first family that resolves.
 */
export const TERMINAL_FONT_FAMILY = '"SF Mono", ui-monospace, Menlo, Monaco, monospace';

export const terminalOptions: ITerminalOptions & ITerminalInitOnlyOptions = {
  theme: terminalTheme,
  fontFamily: TERMINAL_FONT_FAMILY,
  fontSize: 13,
  cursorBlink: true,
  scrollback: 10000,
  // Set before open(): transparency costs performance and triggers WebGL ghosting (#5847).
  allowTransparency: false,
  // Box drawing, blocks, braille, Powerline drawn by the WebGL renderer (the DOM one uses the font).
  customGlyphs: true,
  // Option-click selects even when the Foreground process has mouse tracking on.
  macOptionClickForcesSelection: true,
  // Needed for `terminal.unicode` (Unicode 11 widths).
  allowProposedApi: true,
};
