// File paths printed in a Terminal become links: `src/lib/foo.ts:12:5`, `~/notes.md`, `README.md`.
// Text that looks like a path is only a link once Rust confirms it names a file or directory,
// resolved against the Session's cwd (see src-tauri/src/paths.rs). Double-click (or Cmd-click,
// as for URLs) opens it in its default app.

import type { IBuffer, IBufferCell, ILink, ILinkProvider, Terminal } from "@xterm/xterm";
import { resolvePaths } from "../ipc";
import type { SessionId } from "../types";
import { openPathOnDoubleClick } from "./system";

export interface PathMatch {
  /** The path as printed, without a `:line:col` suffix or trailing punctuation. */
  text: string;
  /** Index of `text` in the line. */
  index: number;
}

/** Runs of text a path can span: whitespace, quotes, brackets and box-drawing end one. */
const RUN = /[^\s"'`()[\]{}<>|,;=*\u2500-\u257f]+/g;
/** Trailing punctuation or a `:line` / `:line:col` suffix. Stripped until none is left. */
const SUFFIX = /(?:[.,:;!?]+|(?::\d+)+)$/;
/** `name.ext` or `.dotfile` with no slash: a file in the cwd. The extension has a letter. */
const BARE_NAME = /^(?:[\w@+-][\w.@+-]*\.[A-Za-z][\w-]*|\.\w[\w.-]*)$/;
const MAX_PATH_LENGTH = 1024;
/** Paths looked up per line, so a wall of text costs one bounded lookup. */
const MAX_PER_LINE = 64;

function looksLikePath(text: string): boolean {
  if (text.length > MAX_PATH_LENGTH) return false;
  if (text.includes("/")) return /\w/.test(text);
  return BARE_NAME.test(text);
}

/** Text in `line` that looks like a file path. URLs are left to the web-links addon. */
export function findPaths(line: string): PathMatch[] {
  const found: PathMatch[] = [];
  for (const m of line.matchAll(RUN)) {
    let text = m[0];
    if (text.includes("://")) continue;
    for (let prev = ""; prev !== text; ) {
      prev = text;
      text = text.replace(SUFFIX, "");
    }
    if (!looksLikePath(text)) continue;
    found.push({ text, index: m.index });
    if (found.length === MAX_PER_LINE) break;
  }
  return found;
}

/** Buffer position (0-based) of one cell, and its width (2 for a wide character). */
interface CellPos {
  x: number;
  y: number;
  width: number;
}

/** Rows a soft-wrapped line is read across, above and below the hovered row. */
const MAX_WRAP_ROWS = 16;

/**
 * The text of the soft-wrapped line through row `y` (0-based), and the cell each UTF-16 unit
 * of it came from. Empty cells read as spaces.
 */
function readLine(buffer: IBuffer, y: number): { text: string; cells: CellPos[] } {
  let top = y;
  while (top > 0 && y - top < MAX_WRAP_ROWS && buffer.getLine(top)?.isWrapped) top--;
  let bottom = y;
  while (bottom - y < MAX_WRAP_ROWS && buffer.getLine(bottom + 1)?.isWrapped) bottom++;

  let text = "";
  const cells: CellPos[] = [];
  let cell: IBufferCell | undefined;
  for (let row = top; row <= bottom; row++) {
    const line = buffer.getLine(row);
    if (!line) break;
    for (let x = 0; x < line.length; x++) {
      cell = line.getCell(x, cell);
      if (!cell) break;
      const width = cell.getWidth();
      if (width === 0) continue; // the right half of a wide character
      const chars = cell.getChars() || " ";
      text += chars;
      for (let i = 0; i < chars.length; i++) cells.push({ x, y: row, width });
    }
  }
  return { text, cells };
}

/** Links for the existing files and directories printed in one Session's Terminal. */
export class FileLinkProvider implements ILinkProvider {
  constructor(
    private readonly term: Terminal,
    private readonly sessionId: SessionId,
  ) {}

  provideLinks(y: number, callback: (links: ILink[] | undefined) => void): void {
    const { text, cells } = readLine(this.term.buffer.active, y - 1);
    const matches = findPaths(text);
    if (matches.length === 0) return callback(undefined);
    resolvePaths(
      this.sessionId,
      matches.map((m) => m.text),
    ).then(
      (resolved) => {
        const links: ILink[] = [];
        matches.forEach((m, i) => {
          const path = resolved[i];
          if (!path) return;
          const start = cells[m.index];
          const end = cells[m.index + m.text.length - 1];
          links.push({
            // 1-based, end inclusive.
            range: {
              start: { x: start.x + 1, y: start.y + 1 },
              end: { x: end.x + end.width, y: end.y + 1 },
            },
            text: m.text,
            activate: (event) => openPathOnDoubleClick(event, path),
          });
        });
        callback(links.length ? links : undefined);
      },
      () => callback(undefined),
    );
  }
}
