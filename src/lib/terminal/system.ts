// Bridges from a Terminal to macOS: OSC 52 clipboard writes and opening links.

import type { IClipboardProvider } from "@xterm/addon-clipboard";
import type { ILinkHandler } from "@xterm/xterm";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { inTauri } from "../ipc";

/**
 * OSC 52 provider for `@xterm/addon-clipboard`. The addon's default provider uses
 * `navigator.clipboard`, which WebKit rejects outside a user gesture (an OSC 52 write comes from
 * the program, never from a gesture), so writes go through the Tauri clipboard plugin.
 *
 * Write-only by design: an OSC 52 query (`?`) would hand the clipboard to whatever runs in the
 * Session, including remote hosts. We answer every query with an empty clipboard; programs get
 * a reply instead of waiting for one. macOS has a single clipboard, so every selection
 * (`c`, `p`, `s`, ...) writes to it.
 */
export const osc52Clipboard: IClipboardProvider = {
  readText: () => "",
  async writeText(_selection, text) {
    try {
      if (inTauri) await writeText(text);
      else await navigator.clipboard?.writeText(text);
    } catch (err) {
      console.warn("[terminal] OSC 52 clipboard write failed", err);
    }
  },
};

/**
 * Open a link from the terminal on Cmd-click only. A plain click stays with the Foreground
 * process (it may have mouse tracking on) and with text selection.
 */
export function openLinkOnCmdClick(event: MouseEvent, uri: string): void {
  if (!event.metaKey) return;
  event.preventDefault();
  if (inTauri) {
    openUrl(uri).catch((err) => console.warn("[terminal] opening link failed", uri, err));
  } else {
    window.open(uri, "_blank", "noopener,noreferrer");
  }
}

/**
 * Handler for OSC 8 hyperlinks (e.g. from `ls --hyperlink`, gh, agents). xterm's default asks with
 * `window.confirm` and then `window.open`s; this uses the same Cmd-click rule as plain URLs.
 */
export const osc8LinkHandler: ILinkHandler = {
  activate: (event, text) => openLinkOnCmdClick(event, text),
  allowNonHttpProtocols: false,
};
