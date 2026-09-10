// Owns one xterm.js Terminal per Session for the Session's lifetime.
// OWNER: terminal agent. CONTRACT (the exported `terminals` API and event names) is owned by
// the tech lead: the app shell and sidebar code call only what is declared here.
// See docs/research/xterm-webview.md and docs/architecture.md.
//
// v0 (tech lead): minimal working implementation so UI work is unblocked. The terminal agent
// replaces the internals (WebGL lifecycle, flow control, clipboard, key routing, theming).

import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { killSession, onSessionExit, resizeSession, spawnSession, writeSession } from "../ipc";
import type { SessionId } from "../types";

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
  /** Spawn a Session (shell on a pty) with a hidden Terminal. Resolves once the pty exists. */
  create(opts?: { cwd?: string | null }): Promise<SessionId>;
  /** Show this Session's Terminal inside `el` (replacing whatever was shown), fit, and focus. */
  mount(sessionId: SessionId, el: HTMLElement): void;
  /** Detach the Terminal from the DOM; the Session and its scrollback live on. */
  unmount(sessionId: SessionId): void;
  focus(sessionId: SessionId): void;
  /** Re-fit the mounted Terminal to its container and resize the pty. */
  fit(sessionId: SessionId): void;
  /** Kill the Session and dispose its Terminal. */
  close(sessionId: SessionId): Promise<void>;
  on<K extends keyof TerminalEvents>(event: K, cb: TerminalEvents[K]): () => void;
}

interface Entry {
  term: Terminal;
  fit: FitAddon;
  host: HTMLDivElement;
  opened: boolean;
  lastActivity: number;
}

const entries = new Map<SessionId, Entry>();
const listeners: { [K in keyof TerminalEvents]: Set<TerminalEvents[K]> } = {
  title: new Set(),
  bell: new Set(),
  activity: new Set(),
  exit: new Set(),
};

function emit<K extends keyof TerminalEvents>(event: K, ...args: Parameters<TerminalEvents[K]>) {
  for (const cb of listeners[event]) (cb as (...a: Parameters<TerminalEvents[K]>) => void)(...args);
}

function dispose(id: SessionId) {
  const e = entries.get(id);
  if (!e) return;
  entries.delete(id);
  e.term.dispose();
  e.host.remove();
}

void onSessionExit(({ sessionId, code }) => {
  if (!entries.has(sessionId)) return;
  dispose(sessionId);
  emit("exit", sessionId, code);
});

export const terminals: TerminalManager = {
  async create(opts) {
    const term = new Terminal({
      fontFamily: 'Menlo, "SF Mono", Monaco, monospace',
      fontSize: 13,
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 10000,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    const host = document.createElement("div");
    host.style.width = "100%";
    host.style.height = "100%";
    const pending: Uint8Array[] = [];
    let id: SessionId | null = null;
    const entry: Entry = { term, fit, host, opened: false, lastActivity: 0 };
    id = await spawnSession({
      cwd: opts?.cwd ?? null,
      cols: 80,
      rows: 24,
      onData: (bytes) => {
        if (id === null) {
          pending.push(bytes);
          return;
        }
        term.write(bytes);
        const now = performance.now();
        if (now - entry.lastActivity > 250) {
          entry.lastActivity = now;
          emit("activity", id);
        }
      },
    });
    const sid = id;
    entries.set(sid, entry);
    pending.forEach((b) => term.write(b));
    term.onData((data) => void writeSession(sid, data));
    term.onTitleChange((t) => emit("title", sid, t));
    term.onBell(() => emit("bell", sid));
    return sid;
  },
  mount(sessionId, el) {
    const e = entries.get(sessionId);
    if (!e) return;
    if (e.host.parentElement !== el) {
      el.replaceChildren(e.host);
    }
    if (!e.opened) {
      e.term.open(e.host);
      e.opened = true;
    }
    this.fit(sessionId);
    e.term.focus();
  },
  unmount(sessionId) {
    entries.get(sessionId)?.host.remove();
  },
  focus(sessionId) {
    entries.get(sessionId)?.term.focus();
  },
  fit(sessionId) {
    const e = entries.get(sessionId);
    if (!e || !e.opened || !e.host.isConnected) return;
    e.fit.fit();
    void resizeSession(sessionId, e.term.cols, e.term.rows);
  },
  async close(sessionId) {
    if (!entries.has(sessionId)) return;
    dispose(sessionId);
    await killSession(sessionId).catch(() => {});
    emit("exit", sessionId, null);
  },
  on(event, cb) {
    listeners[event].add(cb);
    return () => void listeners[event].delete(cb);
  },
};
