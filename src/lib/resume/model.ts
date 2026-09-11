// Pure rules of Resume in the webview: what to type into a Tab's shell to start an entry again.
// The entries themselves come from Rust (src-tauri/src/detect/resume.rs). See docs/architecture.md
// "Resume".

import type { ResumeEntry } from "../types";

/** Ctrl-U: drops whatever is already typed at the prompt (zsh `kill-whole-line`). */
const KILL_LINE = "\x15";

/**
 * `word` as one zsh/bash word: bare when it is plain, else in single quotes. A leading `=` stays
 * quoted (zsh expands `=cmd`). Mirrors `quote` in src-tauri/src/detect/resume.rs.
 */
export function shellQuote(word: string): string {
  if (/^[A-Za-z0-9\-_./:,+@][A-Za-z0-9\-_./:,+@=]*$/.test(word)) return word;
  return `'${word.replace(/'/g, `'\\''`)}'`;
}

/**
 * What to write to a Tab's pty to start `entry` again, given its shell's cwd: the entry's line,
 * after a `cd` when the shell is elsewhere, on a cleared prompt, then Enter.
 */
export function resumeInput(entry: ResumeEntry, shellCwd: string | null): string {
  const cd = entry.cwd && entry.cwd !== shellCwd ? `cd -- ${shellQuote(entry.cwd)} && ` : "";
  return `${KILL_LINE}${cd}${entry.line}\r`;
}
