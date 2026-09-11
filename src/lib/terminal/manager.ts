// Owns one xterm.js Terminal per Session for the Session's lifetime.
// OWNER: terminal agent. CONTRACT (the exported `terminals` API and event names) is owned by
// the tech lead: the app shell and sidebar code call only what is declared here.
// See docs/research/xterm-webview.md and docs/architecture.md.
//
// Lifecycle of one Session's Terminal:
// - `create` spawns the pty and builds the Terminal off-DOM. It is not opened yet (xterm needs a
//   visible, sized parent for `open`); output is parsed into its buffer while hidden.
// - `mount` moves the Terminal's host element into the container, opens it the first time, puts
//   the WebGL renderer on it, fits and focuses. Only mounted Terminals hold a WebGL context.
// - `unmount` releases the WebGL context (the DOM renderer takes over) and detaches the host.
//   The Terminal, its scrollback and modes live on.
// - Shell exit or `close` disposes the Terminal and emits `exit` once.

import { Terminal, type IDisposable } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import "@xterm/xterm/css/xterm.css";
import {
  killSession,
  onSessionExit,
  pauseSession,
  resizeSession,
  resumeSession,
  spawnSession,
  writeSession,
} from "../ipc";
import type { SessionId } from "../types";
import { FlowController } from "./flow-control";
import { TERMINAL_BACKGROUND, terminalOptions } from "./theme";
import { attachWebgl, type WebglRenderer } from "./webgl";
import { openLinkOnCmdClick, osc52Clipboard, osc8LinkHandler } from "./system";
import { FileLinkProvider } from "./fileLinks";

export interface TerminalEvents {
  /** OSC 0/2 title set by the Foreground process (e.g. Codex/Gemini status titles). "" clears. */
  title: (sessionId: SessionId, title: string) => void;
  /** BEL received. */
  bell: (sessionId: SessionId) => void;
  /** Output arrived for this Session (throttled: at most once per ~250 ms per Session). */
  activity: (sessionId: SessionId) => void;
  /** The Session's shell exited (also fired after `close`). */
  exit: (sessionId: SessionId, code: number | null) => void;
}

export interface TerminalManager {
  /**
   * Spawn a Session (shell on a pty) with a hidden Terminal. Resolves once the pty exists.
   * `resumeKey` (the Tab id) names the Session in Resume entries.
   */
  create(opts?: { cwd?: string | null; resumeKey?: string | null }): Promise<SessionId>;
  /** Show this Session's Terminal inside `el` (replacing whatever was shown), fit, and focus. */
  mount(sessionId: SessionId, el: HTMLElement): void;
  /** Detach the Terminal from the DOM; the Session and its scrollback live on. */
  unmount(sessionId: SessionId): void;
  focus(sessionId: SessionId): void;
  /** Paste text as if from the clipboard (bracket-wrapped when the app asked for it), and focus. */
  paste(sessionId: SessionId, text: string): void;
  /** Re-fit the mounted Terminal to its container and resize the pty. */
  fit(sessionId: SessionId): void;
  /** Kill the Session and dispose its Terminal. */
  close(sessionId: SessionId): Promise<void>;
  on<K extends keyof TerminalEvents>(event: K, cb: TerminalEvents[K]): () => void;
}

const ACTIVITY_THROTTLE_MS = 250;

interface Entry {
  id: SessionId;
  term: Terminal;
  fitAddon: FitAddon;
  /** Stable element the Terminal is opened into; re-parented between containers. */
  host: HTMLDivElement;
  opened: boolean;
  /** The container `host` is mounted in, null while hidden. */
  container: HTMLElement | null;
  webgl: WebglRenderer | null;
  flow: FlowController;
  /** Size the pty was last told about; `resizeSession` only runs when the grid differs. */
  ptyCols: number;
  ptyRows: number;
  /** Pending requestAnimationFrame for a coalesced fit, 0 when none. */
  fitFrame: number;
  lastActivity: number;
  /** Serialises pause/resume/resize IPC for this Session so they reach Rust in order. */
  control: Promise<void>;
  subs: IDisposable[];
}

const entries = new Map<SessionId, Entry>();
const listeners: { [K in keyof TerminalEvents]: Set<TerminalEvents[K]> } = {
  title: new Set(),
  bell: new Set(),
  activity: new Set(),
  exit: new Set(),
};

/**
 * Exits for Sessions this module does not know: a shell that died before `create` registered it,
 * or the `session-exit` that follows `close` (already reported). Bounded; ids are never reused.
 */
const earlyExits = new Map<SessionId, number | null>();
const EARLY_EXIT_MEMORY = 32;

/** Grid of the last fitted Terminal: new Sessions spawn at this size to skip a SIGWINCH redraw. */
let lastGrid = { cols: 80, rows: 24 };

function emit<K extends keyof TerminalEvents>(event: K, ...args: Parameters<TerminalEvents[K]>) {
  for (const cb of listeners[event]) {
    try {
      (cb as (...a: Parameters<TerminalEvents[K]>) => void)(...args);
    } catch (err) {
      console.error(`[terminal] "${event}" listener threw`, err);
    }
  }
}

function control(e: Entry, op: () => Promise<void>) {
  e.control = e.control.then(op).catch(() => {
    /* the Session is gone; exit handling cleans up */
  });
}

/** Hand pty output to xterm with back-pressure, and report activity. */
function feed(e: Entry, bytes: Uint8Array) {
  const n = bytes.length;
  e.flow.written(n);
  e.term.write(bytes, () => e.flow.processed(n));
  const now = performance.now();
  if (now - e.lastActivity >= ACTIVITY_THROTTLE_MS) {
    e.lastActivity = now;
    emit("activity", e.id);
  }
}

function cancelFit(e: Entry) {
  if (e.fitFrame) cancelAnimationFrame(e.fitFrame);
  e.fitFrame = 0;
}

/** Fit the mounted Terminal to its host now, and tell the pty if the grid changed. */
function fitNow(e: Entry) {
  cancelFit(e);
  if (!e.opened || !e.container || !e.host.isConnected) return;
  // A collapsed or display:none container would fit to 1 row; wait for a real size instead.
  if (e.host.clientWidth === 0 || e.host.clientHeight === 0) return;
  e.fitAddon.fit();
  const { cols, rows } = e.term;
  lastGrid = { cols, rows };
  if (cols === e.ptyCols && rows === e.ptyRows) return;
  e.ptyCols = cols;
  e.ptyRows = rows;
  control(e, () => resizeSession(e.id, cols, rows));
}

function scheduleFit(e: Entry) {
  if (e.fitFrame || !e.container) return;
  e.fitFrame = requestAnimationFrame(() => {
    e.fitFrame = 0;
    fitNow(e);
  });
}

function hide(e: Entry) {
  cancelFit(e);
  // Release while still attached; the addon's dispose swaps the DOM renderer back in.
  e.webgl?.release();
  e.webgl = null;
  e.host.remove();
  e.container = null;
}

function destroy(e: Entry) {
  entries.delete(e.id);
  hide(e);
  for (const s of e.subs) s.dispose();
  e.term.dispose();
}

function handleExit(sessionId: SessionId, code: number | null) {
  const e = entries.get(sessionId);
  if (!e) return;
  destroy(e);
  emit("exit", sessionId, code);
}

void onSessionExit(({ sessionId, code }) => {
  if (entries.has(sessionId)) {
    handleExit(sessionId, code);
    return;
  }
  earlyExits.set(sessionId, code);
  if (earlyExits.size > EARLY_EXIT_MEMORY) {
    earlyExits.delete(earlyExits.keys().next().value as SessionId);
  }
});

/**
 * Keys. xterm reads input from a hidden textarea; WKWebView gives the page every Cmd-key first
 * and forwards what the page leaves alone to the macOS menu (research: "Cmd-key shortcuts").
 * So every Cmd combination is left to the app's capture-phase shortcuts and the menu:
 * - Cmd-C: Edit > Copy fires a DOM `copy` event, which xterm fills with its selection. WebKit only
 *   enables the Copy item when the DOM selection is a range or a `beforecopy` handler cancels,
 *   and xterm's selection is not a DOM selection, so the host cancels `beforecopy` whenever the
 *   Terminal has a selection (see `create`).
 * - Cmd-V: Edit > Paste fires `paste` on the focused textarea; xterm bracket-wraps it when the
 *   Foreground process enabled bracketed paste and turns newlines into CR.
 * - Cmd-A: the menu's Select All would select the (empty) textarea, so select the buffer here.
 *
 * Known limitation (not shimmed in v1): WKWebView mishandles dead keys and some IMEs with
 * xterm.js (#5894 dead key + non-combining char sends the dead char twice, #6144 first
 * full-width punctuation dropped with Pinyin, #5887/#6045/#6078 keyCode 229 duplicates). Fixing
 * these needs a capture-phase `beforeinput`/`keydown` shim around xterm's textarea; until then
 * v1 is reliable with a US layout only.
 */
function handleKey(term: Terminal, ev: KeyboardEvent): boolean {
  if (!ev.metaKey) return true;
  if (
    ev.type === "keydown" &&
    !ev.shiftKey &&
    !ev.altKey &&
    !ev.ctrlKey &&
    ev.key.toLowerCase() === "a"
  ) {
    term.selectAll();
    ev.preventDefault();
  }
  return false;
}

export const terminals: TerminalManager = {
  async create(opts) {
    const { cols, rows } = lastGrid;
    const term = new Terminal({ ...terminalOptions, cols, rows, linkHandler: osc8LinkHandler });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.loadAddon(new WebLinksAddon(openLinkOnCmdClick));
    term.loadAddon(new ClipboardAddon(undefined, osc52Clipboard));
    term.attachCustomKeyEventHandler((ev) => handleKey(term, ev));

    const host = document.createElement("div");
    host.className = "sidebar-term-host";
    host.style.width = "100%";
    host.style.height = "100%";
    host.style.background = TERMINAL_BACKGROUND;
    // Enables Edit > Copy (and so Cmd-C) in WebKit while the Terminal has a selection.
    host.addEventListener("beforecopy", (ev) => {
      if (term.hasSelection()) ev.preventDefault();
    });

    let entry: Entry | null = null;
    const early: Uint8Array[] = [];
    let sid: SessionId;
    try {
      sid = await spawnSession({
        cwd: opts?.cwd ?? null,
        resumeKey: opts?.resumeKey ?? null,
        cols,
        rows,
        onData: (bytes) => (entry ? feed(entry, bytes) : early.push(bytes)),
      });
    } catch (err) {
      term.dispose();
      throw err;
    }

    const e: Entry = {
      id: sid,
      term,
      fitAddon,
      host,
      opened: false,
      container: null,
      webgl: null,
      flow: new FlowController({
        onPause: () => control(e, () => pauseSession(sid)),
        onResume: () => control(e, () => resumeSession(sid)),
      }),
      ptyCols: cols,
      ptyRows: rows,
      fitFrame: 0,
      lastActivity: 0,
      control: Promise.resolve(),
      subs: [],
    };
    entries.set(sid, e);
    entry = e;
    for (const bytes of early) feed(e, bytes);

    e.subs.push(
      term.onData((data) => void writeSession(sid, data).catch(() => {})),
      // X10 mouse reports arrive as "binary" strings. `session_write` takes a UTF-8 string, so
      // only bytes < 0x80 survive the trip unchanged; drop the rest rather than send garbage.
      term.onBinary((data) => {
        if (/^[\x00-\x7f]*$/.test(data)) void writeSession(sid, data).catch(() => {});
      }),
      term.onTitleChange((title) => emit("title", sid, title)),
      term.onBell(() => emit("bell", sid)),
      // After the web-links addon's provider, so a URL wins over a path inside it.
      term.registerLinkProvider(new FileLinkProvider(term, sid)),
    );

    if (earlyExits.has(sid)) {
      const code = earlyExits.get(sid) ?? null;
      earlyExits.delete(sid);
      // After the caller has the id from this promise.
      setTimeout(() => handleExit(sid, code), 0);
    }
    return sid;
  },

  mount(sessionId, el) {
    const e = entries.get(sessionId);
    if (!e) return;
    for (const other of entries.values()) {
      if (other !== e && other.container === el) hide(other);
    }
    if (e.host.parentElement !== el || el.childNodes.length !== 1) el.replaceChildren(e.host);
    e.container = el;
    if (!e.opened) {
      e.term.open(e.host);
      e.opened = true;
    }
    if (!e.webgl) {
      e.webgl = attachWebgl(e.term, () => {
        e.webgl = null;
        fitNow(e);
      });
    }
    // After the renderer is in place: WebGL and DOM measure cells differently.
    fitNow(e);
    e.term.focus();
  },

  unmount(sessionId) {
    const e = entries.get(sessionId);
    if (e) hide(e);
  },

  focus(sessionId) {
    entries.get(sessionId)?.term.focus();
  },

  paste(sessionId, text) {
    const e = entries.get(sessionId);
    if (!e) return;
    e.term.paste(text);
    e.term.focus();
  },

  fit(sessionId) {
    const e = entries.get(sessionId);
    if (e) scheduleFit(e);
  },

  async close(sessionId) {
    const e = entries.get(sessionId);
    if (!e) return;
    destroy(e);
    await killSession(sessionId).catch(() => {});
    emit("exit", sessionId, null);
  },

  on(event, cb) {
    listeners[event].add(cb);
    return () => void listeners[event].delete(cb);
  },
};
