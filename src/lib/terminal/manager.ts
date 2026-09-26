// Owns one xterm.js Terminal per Session for the Session's lifetime, on any Host.
// OWNER: terminal agent. CONTRACT (the exported `terminals` API and event names) is owned by
// the tech lead: the app shell and sidebar code call only what is declared here.
// See docs/research/xterm-webview.md and docs/architecture.md ("Webview modules", "Hosts").
//
// A Session is keyed by its Host and id (`SessionKey`, src/lib/host/ids.ts). Where its bytes
// come from and go to is a `SessionTransport`: `localTransport` (below) reaches this Mac's own
// Host in process over IPC; a paired Host's transport (src/lib/host/hosts.svelte.ts) speaks the
// Host protocol over its socket. The Terminal itself is the same either way.
//
// Lifecycle of one Session's Terminal:
// - `attach` builds the Terminal off-DOM for a Session the Host already spawned (with its Tab:
//   at launch, or on `tab_new`) and takes the Session's output from its transport, starting with
//   what it printed before. The Terminal is not opened yet (xterm needs a visible, sized parent
//   for `open`); output is parsed into its buffer while hidden. A remote Host replays its recent
//   output on every attach (the first, and after a reconnect): the grid is cleared before each.
// - `mount` moves the Terminal's host element into the container, opens it the first time, puts
//   the WebGL renderer on it, fits and focuses. Only mounted Terminals hold a WebGL context.
// - `unmount` releases the WebGL context (the DOM renderer takes over) and detaches the host.
//   The Terminal, its scrollback and modes live on.
// - The Session's exit (its shell ended, or its Tab was closed) disposes the Terminal and emits
//   `exit` once.

import { Terminal, type IDisposable } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import "@xterm/xterm/css/xterm.css";
import { attachSession, onSessionExit, pauseSession, resizeSession, resumeSession, writeSession } from "../ipc";
import type { SessionId } from "../types";
import { parseSessionKey, type SessionKey } from "../host/ids";
import { FlowController } from "./flow-control";
import { resolveDroppedPaths, shellEscape } from "./drop";
import { TERMINAL_BACKGROUND, terminalOptions } from "./theme";
import { attachWebgl, type WebglRenderer } from "./webgl";
import { openLinkOnCmdClick, osc52Clipboard, osc8LinkHandler } from "./system";
import { FileLinkProvider } from "./fileLinks";

export interface TerminalEvents {
  /** OSC 0/2 title set by the Foreground process (e.g. Codex/Gemini status titles). "" clears. */
  title: (key: SessionKey, title: string) => void;
  /** BEL received. */
  bell: (key: SessionKey) => void;
  /** Output arrived for this Session (throttled: at most once per ~250 ms per Session). */
  activity: (key: SessionKey) => void;
  /** The Session's shell exited (also fired after `close`). */
  exit: (key: SessionKey, code: number | null) => void;
}

/** Where a Terminal's Session lives: what a transport hands the Terminal. */
export interface TerminalSink {
  /** Output, in order, to feed straight to `terminal.write`. */
  data(bytes: Uint8Array): void;
  /**
   * A replay of the Session's recent output follows, at the Host's grid (a remote Host, on every
   * attach): the Terminal clears its grid first so nothing is shown twice.
   */
  replay(cols: number, rows: number): void;
  /** The Session ended (its Tab is gone with it). */
  exit(code: number | null): void;
}

/** How a Host's Sessions are reached: in process for the local Host, over its socket for a paired one. */
export interface SessionTransport {
  /** Take the Session's output from here on, starting with what it printed before. Rejects for a Session that is gone. */
  attach(id: SessionId, sink: TerminalSink): Promise<void>;
  detach(id: SessionId): void;
  write(id: SessionId, data: string): Promise<void>;
  /** Size the pty; a remote Host refuses while another client shows the Session (ignored). */
  resize(id: SessionId, cols: number, rows: number): Promise<void>;
  /** Flow control; a remote Host's output ring and drop-behind rules stand in (no-ops there). */
  pause(id: SessionId): Promise<void>;
  resume(id: SessionId): Promise<void>;
  /** Paths, on the Session's Host, for files dropped on its Terminal (an upload for a remote Host). */
  dropPaths(id: SessionId, files: File[]): Promise<string[]>;
  /** Whether paths printed in the Terminal name files this Mac can open (the local Host only). */
  opensFiles: boolean;
}

export interface TerminalManager {
  /**
   * A hidden Terminal for a Session the Host spawned, fed its output through `transport` from
   * here on. The Terminal exists (and can be mounted) as soon as this returns; the output
   * follows. A no-op for a Session that already has one.
   */
  attach(key: SessionKey, transport: SessionTransport): void;
  /** The grid of the last fitted Terminal: what a new Session's pty should be sized to. */
  grid(): { cols: number; rows: number };
  /** Show this Session's Terminal inside `el` (replacing whatever was shown), fit, and focus. */
  mount(key: SessionKey, el: HTMLElement): void;
  /** Detach the Terminal from the DOM; the Session and its scrollback live on. */
  unmount(key: SessionKey): void;
  focus(key: SessionKey): void;
  /** Paste text as if from the clipboard (bracket-wrapped when the app asked for it), and focus. */
  paste(key: SessionKey, text: string): void;
  /** Files dropped on the Terminal: paste their paths on the Session's Host, shell-escaped. */
  dropFiles(key: SessionKey, files: File[]): Promise<void>;
  /** Re-fit the mounted Terminal to its container and resize the pty. */
  fit(key: SessionKey): void;
  on<K extends keyof TerminalEvents>(event: K, cb: TerminalEvents[K]): () => void;
}

const ACTIVITY_THROTTLE_MS = 250;

interface Entry {
  key: SessionKey;
  id: SessionId;
  transport: SessionTransport;
  term: Terminal;
  fitAddon: FitAddon;
  /** Stable element the Terminal is opened into; re-parented between containers. */
  host: HTMLDivElement;
  opened: boolean;
  /** The container `host` is mounted in, null while hidden. */
  container: HTMLElement | null;
  webgl: WebglRenderer | null;
  flow: FlowController;
  /** Size the pty was last told about; `resize` only runs when the grid differs. */
  ptyCols: number;
  ptyRows: number;
  /** Pending requestAnimationFrame for a coalesced fit, 0 when none. */
  fitFrame: number;
  lastActivity: number;
  /** Serialises pause/resume/resize for this Session so they reach the Host in order. */
  control: Promise<void>;
  subs: IDisposable[];
}

const entries = new Map<SessionKey, Entry>();
const listeners: { [K in keyof TerminalEvents]: Set<TerminalEvents[K]> } = {
  title: new Set(),
  bell: new Set(),
  activity: new Set(),
  exit: new Set(),
};

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
    /* the Session is gone, or the Host refused (a resize while another client shows it) */
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
    emit("activity", e.key);
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
  control(e, () => e.transport.resize(e.id, cols, rows));
}

function scheduleFit(e: Entry) {
  if (e.fitFrame || !e.container) return;
  e.fitFrame = requestAnimationFrame(() => {
    e.fitFrame = 0;
    fitNow(e);
  });
}

/**
 * A replay is about to arrive at the Host's grid. Hidden, the Terminal takes that grid so the
 * replay lays out as the Host rendered it; mounted, it keeps its own and the fit resizes the pty.
 */
function beforeReplay(e: Entry, cols: number, rows: number) {
  e.term.reset();
  e.ptyCols = cols;
  e.ptyRows = rows;
  if (e.container) fitNow(e);
  else if (cols > 0 && rows > 0) e.term.resize(cols, rows);
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
  entries.delete(e.key);
  hide(e);
  for (const s of e.subs) s.dispose();
  e.term.dispose();
}

function handleExit(key: SessionKey, code: number | null) {
  const e = entries.get(key);
  if (!e) return;
  e.transport.detach(e.id);
  destroy(e);
  emit("exit", key, code);
}

/**
 * Keys. xterm reads input from a hidden textarea; WKWebView gives the page every Cmd-key first
 * and forwards what the page leaves alone to the macOS menu (research: "Cmd-key shortcuts").
 * So every Cmd combination is left to the app's capture-phase shortcuts and the menu:
 * - Cmd-C: Edit > Copy fires a DOM `copy` event, which xterm fills with its selection. WebKit only
 *   enables the Copy item when the DOM selection is a range or a `beforecopy` handler cancels,
 *   and xterm's selection is not a DOM selection, so the host cancels `beforecopy` whenever the
 *   Terminal has a selection (see `attach`).
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
  attach(key, transport) {
    if (entries.has(key)) return;
    const parsed = parseSessionKey(key);
    if (!parsed) return;
    const sid = parsed.id;
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

    const e: Entry = {
      key,
      id: sid,
      transport,
      term,
      fitAddon,
      host,
      opened: false,
      container: null,
      webgl: null,
      flow: new FlowController({
        onPause: () => control(e, () => transport.pause(sid)),
        onResume: () => control(e, () => transport.resume(sid)),
      }),
      ptyCols: cols,
      ptyRows: rows,
      fitFrame: 0,
      lastActivity: 0,
      control: Promise.resolve(),
      subs: [],
    };
    entries.set(key, e);
    // The Session's output, held by the Host since it spawned, then live. A Session that is gone
    // rejects; its exit (or the next layout snapshot) disposes the Terminal.
    const live = () => entries.get(key) === e;
    void transport
      .attach(sid, {
        data: (bytes) => {
          if (live()) feed(e, bytes);
        },
        replay: (c, r) => {
          if (live()) beforeReplay(e, c, r);
        },
        exit: (code) => {
          if (live()) handleExit(key, code);
        },
      })
      .catch(() => {
        if (live()) handleExit(key, null);
      });

    e.subs.push(
      term.onData((data) => void transport.write(sid, data).catch(() => {})),
      // X10 mouse reports arrive as "binary" strings. Writes take a UTF-8 string, so only bytes
      // < 0x80 survive the trip unchanged; drop the rest rather than send garbage.
      term.onBinary((data) => {
        if (/^[\x00-\x7f]*$/.test(data)) void transport.write(sid, data).catch(() => {});
      }),
      term.onTitleChange((title) => emit("title", key, title)),
      term.onBell(() => emit("bell", key)),
    );
    // After the web-links addon's provider, so a URL wins over a path inside it. Only where the
    // paths name files this Mac can open.
    if (transport.opensFiles) e.subs.push(term.registerLinkProvider(new FileLinkProvider(term, sid)));
  },

  grid() {
    return { ...lastGrid };
  },

  mount(key, el) {
    const e = entries.get(key);
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

  unmount(key) {
    const e = entries.get(key);
    if (e) hide(e);
  },

  focus(key) {
    entries.get(key)?.term.focus();
  },

  paste(key, text) {
    const e = entries.get(key);
    if (!e) return;
    e.term.paste(text);
    e.term.focus();
  },

  async dropFiles(key, files) {
    const e = entries.get(key);
    if (!e) return;
    const paths = await e.transport.dropPaths(e.id, files);
    if (paths.length && entries.get(key) === e) terminals.paste(key, paths.map(shellEscape).join(" ") + " ");
  },

  fit(key) {
    const e = entries.get(key);
    if (e) scheduleFit(e);
  },

  on(event, cb) {
    listeners[event].add(cb);
    return () => void listeners[event].delete(cb);
  },
};

// --- The local Host: this Mac's own Sessions, over IPC ----------------------------------------

const localSinks = new Map<SessionId, TerminalSink>();

/**
 * Exits for Sessions no Terminal is attached to: a Session that ended before the layout snapshot
 * naming it was applied (its Tab is already gone with it). Bounded; ids are never reused.
 */
const earlyExits = new Map<SessionId, number | null>();
const EARLY_EXIT_MEMORY = 32;

void onSessionExit(({ sessionId, code }) => {
  const sink = localSinks.get(sessionId);
  if (sink) {
    localSinks.delete(sessionId);
    sink.exit(code);
    return;
  }
  earlyExits.set(sessionId, code);
  if (earlyExits.size > EARLY_EXIT_MEMORY) {
    earlyExits.delete(earlyExits.keys().next().value as SessionId);
  }
});

export const localTransport: SessionTransport = {
  async attach(id, sink) {
    if (earlyExits.has(id)) {
      earlyExits.delete(id);
      throw new Error(`Session ${id} ended already`); // its Tab went with it
    }
    localSinks.set(id, sink);
    try {
      await attachSession(id, (bytes) => {
        if (localSinks.get(id) === sink) sink.data(bytes);
      });
    } catch (e) {
      if (localSinks.get(id) === sink) localSinks.delete(id);
      throw e;
    }
  },
  detach(id) {
    localSinks.delete(id);
  },
  write: writeSession,
  resize: resizeSession,
  pause: pauseSession,
  resume: resumeSession,
  dropPaths: (_id, files) => resolveDroppedPaths(files),
  opensFiles: true,
};
