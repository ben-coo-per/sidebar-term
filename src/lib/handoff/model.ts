// Pure rules of Handoff (docs/architecture.md "Handoff"): where a Tab lands on a Host, what is
// typed into the new Tab there, and what the confirmation says. The Resume rule for "what to
// type" (src/lib/resume/model.ts) is reused with the cwd mapped onto the Host's checkout. No
// state, no sockets: src/lib/handoff/handoff.svelte.ts runs the flow.

import { KILL_LINE, resumeLine, shellQuote } from "../resume/model";
import type { GitStatus, ResumeEntry } from "../types";

/** Cursor left, one cell (CSI D): puts the cursor inside a typed quote for the user to finish. */
const CURSOR_LEFT = "\x1b[D";

/** Where a Tab lands on a Host. */
export type Landing =
  /** The repo's checkout on the Host: the Tab opens there. */
  | { kind: "checkout"; cwd: string }
  /** The Host has no checkout at `path`: the Tab opens at `cwd` (the Checkout root, else home) and `git clone` is typed. */
  | { kind: "clone"; cwd: string | null; path: string }
  /** No repo, or no map for this Host: the Tab opens at the Host's home (null: the Host's own default). */
  | { kind: "home"; cwd: string | null };

/**
 * Where a Tab lands: at `repoPath` (the repo's mapped checkout) when it exists on the Host; a
 * clone into it otherwise, typed from the Checkout root when that exists, else from home; the
 * Host's home when the Tab is in no repo or the Host has no map for it.
 */
export function landing(opts: {
  repoPath: string | null;
  repoExists: boolean;
  root: string | null;
  rootExists: boolean;
  home: string | null;
}): Landing {
  if (!opts.repoPath) return { kind: "home", cwd: opts.home };
  if (opts.repoExists) return { kind: "checkout", cwd: opts.repoPath };
  return { kind: "clone", cwd: opts.root && opts.rootExists ? opts.root : opts.home, path: opts.repoPath };
}

/** A local path inside the Worktree at `localRoot`, at the same place under the Host's checkout. */
export function mapPath(localPath: string, localRoot: string, hostCheckout: string): string {
  if (localPath === localRoot) return hostCheckout;
  if (localPath.startsWith(`${localRoot}/`)) return `${hostCheckout}${localPath.slice(localRoot.length)}`;
  return hostCheckout;
}

/** `git switch <branch>`, typed before the command on the Host (the same branch as the Badge). */
export function switchLine(branch: string): string {
  return `git switch ${shellQuote(branch)}`;
}

/** `git clone <url> <path>`. With no URL known, the space for it is left with the cursor in it. */
export function cloneTyped(url: string | null, path: string): string {
  if (url) return `git clone ${shellQuote(url)} ${shellQuote(path)}`;
  const rest = ` ${shellQuote(path)}`;
  return `git clone ${rest}${CURSOR_LEFT.repeat(rest.length)}`;
}

/**
 * The line of a Claude Code Resume entry (`claude <flags> --resume <id>`) as a fresh `claude`
 * with `note` as its first prompt, quoted, for the user to finish: the fallback when the
 * conversation could not be moved.
 */
export function freshClaudeLine(entry: Pick<ResumeEntry, "line">, note: string): string {
  const flags = entry.line.replace(/\s--resume\s+\S+/, "").replace(/^claude/, "").trim();
  return `claude${flags ? ` ${flags}` : ""} '${note.replace(/'/g, `'\\''`)}'`;
}

/** The one-line note a fresh `claude` on the Host starts with, for the user to complete. */
export function handoffNote(opts: { repo: string | null; branch: string | null; id: string; reason: string }): string {
  const where = opts.repo ? ` in ${opts.repo}${opts.branch ? ` on ${opts.branch}` : ""}` : "";
  return `Handed off from my Mac${where}. The Claude Code conversation ${opts.id} stayed there (${opts.reason}). Where we left off: `;
}

export interface HandoffInputOptions {
  landing: Landing;
  /** What the Tab was running; null for nothing (a New Tab, or a shell at its prompt). */
  entry: ResumeEntry | null;
  /** The local Worktree root, to put `entry.cwd` at the same place under the Host's checkout. */
  localRoot: string | null;
  /** The local Badge's branch: `git switch` is typed first when the Host has the checkout. */
  branch: string | null;
  /** `origin`'s URL, for the clone. */
  cloneUrl: string | null;
  /** For a Claude Code entry: the conversation is on the Host, so `--resume` will find it. */
  conversationMoved: boolean;
  /** For a Claude Code entry that did not move: the note the fresh `claude` starts with. */
  note: string;
}

/**
 * What to type into the new Tab on the Host once its shell is up, on a cleared prompt:
 * - at the checkout: `git switch <branch> && <line>`, Enter (the line at the entry's place under
 *   the checkout; just the switch when nothing was running; nothing when neither);
 * - a Claude Code conversation that did not move: a fresh `claude` with the note as its prompt,
 *   the cursor left inside the quote and no Enter, for the user to finish;
 * - no checkout on the Host: one `git clone <url> <path>` line, with the switch and the line
 *   after it (`&& cd -- <path> && …`), typed but not run.
 */
export function handoffInput(opts: HandoffInputOptions): string {
  const { landing, entry, branch } = opts;
  const fresh = entry?.kind === "claude" && !opts.conversationMoved;
  const line = entry ? (fresh ? freshClaudeLine(entry, opts.note) : entry.line) : null;
  const enter = fresh ? CURSOR_LEFT : "\r";

  if (landing.kind === "clone") {
    const steps = [cloneTyped(opts.cloneUrl, landing.path)];
    if (line || branch) steps.push(`cd -- ${shellQuote(landing.path)}`);
    if (branch) steps.push(switchLine(branch));
    if (line) steps.push(line);
    const typed = steps.join(" && ");
    return `${KILL_LINE}${typed}${fresh ? CURSOR_LEFT : ""}`;
  }

  const before = landing.kind === "checkout" && branch ? switchLine(branch) : null;
  if (!line) return before ? `${KILL_LINE}${before}\r` : "";
  const cwd =
    landing.kind === "checkout" && entry?.cwd && opts.localRoot
      ? mapPath(entry.cwd, opts.localRoot, landing.cwd)
      : landing.kind === "checkout"
        ? landing.cwd
        : null;
  return `${KILL_LINE}${resumeLine({ line, cwd }, landing.cwd, { cwd, before })}${enter}`;
}

/** What `git push -u origin HEAD` types, not run, for the user to check first. */
export function pushTyped(): string {
  return `${KILL_LINE}git push -u origin HEAD`;
}

/** What would not move with the Tab, in words; null when the checkout is clean and pushed. */
export function unpushedSummary(git: GitStatus | null): string | null {
  if (!git) return null;
  const parts: string[] = [];
  if (git.changes > 0) parts.push(`${git.changes} uncommitted change${git.changes === 1 ? "" : "s"}`);
  if (git.ahead > 0) parts.push(`${git.ahead} unpushed commit${git.ahead === 1 ? "" : "s"}`);
  if (git.branch && !git.upstream) parts.push(`a branch never pushed (${git.branch})`);
  if (parts.length === 0) return null;
  return `The checkout has ${parts.join(" and ")}; they stay on this Mac.`;
}

/** The confirmation's explanation of a Move, one line each. */
export function moveDetail(opts: {
  hostName: string;
  landing: Landing;
  entry: ResumeEntry | null;
  branch: string | null;
  git: GitStatus | null;
  mapped: boolean;
  inRepo: boolean;
}): string[] {
  const lines: string[] = [];
  const { landing } = opts;
  if (landing.kind === "checkout") lines.push(`Lands in ${landing.cwd} on ${opts.hostName}.`);
  else if (landing.kind === "clone")
    lines.push(`${opts.hostName} has no checkout at ${landing.path}: git clone is typed there for you to run.`);
  else if (!opts.inRepo) lines.push(`Not in a repo: lands in ${opts.hostName}'s home.`);
  else if (!opts.mapped) lines.push(`No Checkout root is set for ${opts.hostName} (Settings > Hosts): lands in its home.`);
  if (opts.entry?.kind === "claude") lines.push("The Claude Code conversation moves with it and is resumed there.");
  else if (opts.entry) lines.push(`Reruns there: ${opts.entry.line}`);
  else lines.push("Nothing is running in it.");
  if (opts.branch && landing.kind !== "home") lines.push(`git switch ${opts.branch} is typed first.`);
  const unpushed = unpushedSummary(opts.git);
  if (unpushed) lines.push(unpushed);
  return lines;
}
