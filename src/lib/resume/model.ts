// Pure rules of Resume in the webview: what to type into a Tab's shell to start an entry again.
// The entries themselves come from Rust (src-tauri/core/src/detect/resume.rs). Handoff reuses
// the same rule for another Host, with the cwd mapped to the Host's checkout
// (src/lib/handoff/model.ts). See docs/architecture.md "Resume" and "Handoff".

import type { ResumeEntry } from "../types";

/** Ctrl-U: drops whatever is already typed at the prompt (zsh `kill-whole-line`). */
export const KILL_LINE = "\x15";

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
 * after a `cd` when the shell is elsewhere, on a cleared prompt, then Enter. `cwd` replaces the
 * entry's own directory (Handoff: the same entry, at the Host's checkout); `before` is a command
 * the line follows with `&&` (Handoff: `git switch <branch>`), so the line runs only if it worked.
 */
export function resumeInput(
  entry: Pick<ResumeEntry, "line" | "cwd">,
  shellCwd: string | null,
  opts: { cwd?: string | null; before?: string | null } = {},
): string {
  return `${KILL_LINE}${resumeLine(entry, shellCwd, opts)}\r`;
}

/** The command line `resumeInput` types, without the prompt clearing and the Enter. */
export function resumeLine(
  entry: Pick<ResumeEntry, "line" | "cwd">,
  shellCwd: string | null,
  opts: { cwd?: string | null; before?: string | null } = {},
): string {
  const cwd = opts.cwd === undefined ? entry.cwd : opts.cwd;
  const cd = cwd && cwd !== shellCwd ? `cd -- ${shellQuote(cwd)} && ` : "";
  const before = opts.before ? `${opts.before} && ` : "";
  return `${cd}${before}${entry.line}`;
}
