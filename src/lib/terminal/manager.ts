// Owns one xterm.js Terminal per Session for the Session's lifetime.
// OWNER: terminal agent. CONTRACT (the exported `terminals` API and event names) is owned by
// the tech lead: the app shell and sidebar code call only what is declared here.
// See docs/research/xterm-webview.md and docs/architecture.md.

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

export const terminals: TerminalManager = {
  async create() {
    throw new Error("terminal manager not implemented");
  },
  mount() {},
  unmount() {},
  focus() {},
  fit() {},
  async close() {},
  on() {
    return () => {};
  },
};
