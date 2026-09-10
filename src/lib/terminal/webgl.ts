// WebGL renderer for the mounted Terminal, with the DOM renderer as fallback.
// See docs/research/xterm-webview.md, "Renderer: WebGL vs DOM in WKWebView":
// - WebKit keeps at most 16 live WebGL contexts and recycles the oldest one (which fires
//   context loss on a Terminal that may still be on screen).
// - `WebglAddon.dispose()` removes the canvas but leaves the context alive until GC (#6068), so we
//   lose it explicitly on release.
// - Load failure and `onContextLoss` fall back to the DOM renderer (the addon's dispose restores it).

import { WebglAddon } from "@xterm/addon-webgl";
import type { Terminal } from "@xterm/xterm";

/** Escape hatch until there is a settings UI: `localStorage["sidebar-term:renderer"] = "dom"`. */
const FORCE_DOM_KEY = "sidebar-term:renderer";
/** After this many context losses in one app run, stop trying WebGL (a broken GPU/WebKit build). */
const MAX_CONTEXT_LOSSES = 3;

let webglDisabled = forcedDom();
let contextLosses = 0;

function forcedDom(): boolean {
  try {
    return globalThis.localStorage?.getItem(FORCE_DOM_KEY) === "dom";
  } catch {
    return false;
  }
}

export interface WebglRenderer {
  /** Dispose the addon (the DOM renderer takes over) and release its GL context now. */
  release(): void;
}

/**
 * Put the WebGL renderer on an opened Terminal. Returns null when WebGL is disabled or fails to
 * load; the Terminal keeps the DOM renderer. `onFallback` runs if the context is lost later, after
 * the DOM renderer is back: the caller must re-fit (renderer swaps change cell widths, #6015).
 */
export function attachWebgl(term: Terminal, onFallback: () => void): WebglRenderer | null {
  const root = term.element;
  if (webglDisabled || !root) return null;

  const canvasesBefore = new Set(root.querySelectorAll("canvas"));
  let addon: WebglAddon | undefined;
  try {
    addon = new WebglAddon();
    term.loadAddon(addon);
  } catch (err) {
    console.warn("[terminal] WebGL renderer unavailable; using the DOM renderer", err);
    webglDisabled = true;
    try {
      addon?.dispose();
    } catch {
      /* already half-torn-down */
    }
    return null;
  }
  const loaded = addon;

  // The addon keeps its context private. Every canvas it added already owns a context, so
  // getContext("webgl2") returns the renderer's own context (or null for its 2D link layer).
  const contexts: WebGL2RenderingContext[] = [];
  for (const canvas of root.querySelectorAll("canvas")) {
    if (canvasesBefore.has(canvas)) continue;
    const gl = canvas.getContext("webgl2");
    if (gl) contexts.push(gl);
  }

  let released = false;
  const release = (loseContext: boolean) => {
    if (released) return;
    released = true;
    lossSub.dispose();
    try {
      loaded.dispose();
    } catch (err) {
      console.warn("[terminal] disposing the WebGL renderer failed", err);
    }
    if (loseContext) {
      for (const gl of contexts) gl.getExtension("WEBGL_lose_context")?.loseContext();
    }
  };

  const lossSub = loaded.onContextLoss(() => {
    contextLosses += 1;
    if (contextLosses >= MAX_CONTEXT_LOSSES) {
      webglDisabled = true;
      console.warn(`[terminal] ${contextLosses} WebGL context losses; using the DOM renderer from now on`);
    } else {
      console.warn("[terminal] WebGL context lost; falling back to the DOM renderer");
    }
    release(false);
    onFallback();
  });

  return { release: () => release(true) };
}
