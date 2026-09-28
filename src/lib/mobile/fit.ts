// How a Session is laid out on the phone. The text is a size the phone can read, and the grid
// is what fits the screen at that size: the phone asks its Host to size the pty to it, which the
// Host does while no other client shows the Session. Refused, the phone keeps the Host's grid at
// the readable size and pans across it, or shrinks the font until its columns fit the width.
// Pure maths; the component measures and calls.

/** Smallest font the fit to the width goes down to; below it the grid pans sideways. */
export const MIN_FONT_SIZE = 6;
export const MAX_FONT_SIZE = 15;

/** The text size the phone reads at, until the user picks another. */
export const DEFAULT_TEXT_SIZE = 12;
export const MIN_TEXT_SIZE = 9;
export const MAX_TEXT_SIZE = 22;

/** The smallest grid asked of a Host: less is a sliver no program lays out in. */
export const MIN_COLS = 20;
export const MIN_ROWS = 6;

export interface Grid {
  cols: number;
  rows: number;
}

/**
 * The font size at which `cols` cells fit in `widthPx`, given that a cell of the font at
 * `measuredFontSize` is `measuredCellWidth` wide (cell width scales linearly with font size).
 * Clamped to [MIN_FONT_SIZE, MAX_FONT_SIZE]; the result is whole pixels, which xterm renders
 * crisply, so the grid may end a little short of the edge.
 */
export function fontSizeToFit(widthPx: number, cols: number, measuredFontSize: number, measuredCellWidth: number): number {
  if (cols <= 0 || widthPx <= 0 || measuredCellWidth <= 0 || measuredFontSize <= 0) return MIN_FONT_SIZE;
  const perPx = measuredCellWidth / measuredFontSize; // cell width per font px
  const ideal = Math.floor(widthPx / (cols * perPx));
  return Math.max(MIN_FONT_SIZE, Math.min(MAX_FONT_SIZE, ideal));
}

/** A first guess before anything is measured: monospace cells are ~0.6 em wide. */
export function fontSizeGuess(widthPx: number, cols: number): number {
  return fontSizeToFit(widthPx, cols, 10, 6);
}

/** A text size the user may pick: whole pixels in [MIN_TEXT_SIZE, MAX_TEXT_SIZE]; the default for nonsense. */
export function clampTextSize(size: number): number {
  if (!Number.isFinite(size)) return DEFAULT_TEXT_SIZE;
  return Math.max(MIN_TEXT_SIZE, Math.min(MAX_TEXT_SIZE, Math.round(size)));
}

/**
 * The grid that fits `widthPx` by `heightPx` with cells of `cellWidth` by `cellHeight`: whole
 * cells only, and never less than the smallest grid. Null while nothing is measured.
 */
export function gridToFit(widthPx: number, heightPx: number, cellWidth: number, cellHeight: number): Grid | null {
  if (widthPx <= 0 || heightPx <= 0 || cellWidth <= 0 || cellHeight <= 0) return null;
  return {
    cols: Math.max(MIN_COLS, Math.floor(widthPx / cellWidth)),
    rows: Math.max(MIN_ROWS, Math.floor(heightPx / cellHeight)),
  };
}

export function sameGrid(a: Grid | null, b: Grid | null): boolean {
  return a !== null && b !== null && a.cols === b.cols && a.rows === b.rows;
}

/** What a vertical drag moved: the screen's own scroll, and lines of scrollback. */
export interface Dragged {
  /** Where the screen's scroll ends up. */
  scrollTop: number;
  /** Lines to scroll the scrollback by: negative towards older output. */
  lines: number;
  /** What is left of the drag, less than a line, for the next one. */
  carry: number;
}

/**
 * Share a vertical drag of `dy` pixels (positive: the finger goes down, towards older output)
 * between the screen's own scroll and the scrollback, so the two read as one. Going up the
 * output, the screen scrolls to its top first, then the scrollback; coming back, the scrollback
 * first (`linesBelow`: how far it is from its end), then the screen.
 */
export function shareDrag(
  dy: number,
  scrollTop: number,
  maxScrollTop: number,
  linesBelow: number,
  cellHeight: number,
  carry: number,
): Dragged {
  if (cellHeight <= 0) return { scrollTop, lines: 0, carry: 0 };
  if (dy >= 0) {
    const byScreen = Math.min(dy, scrollTop);
    const rest = dy - byScreen + Math.max(0, carry);
    const lines = Math.floor(rest / cellHeight);
    return { scrollTop: scrollTop - byScreen, lines: lines === 0 ? 0 : -lines, carry: rest - lines * cellHeight };
  }
  const want = -dy - Math.min(0, carry);
  const lines = Math.min(linesBelow, Math.floor(want / cellHeight));
  const rest = want - lines * cellHeight;
  if (lines < linesBelow) return { scrollTop, lines, carry: -rest };
  // The scrollback is at its end: the rest goes to the screen.
  return { scrollTop: Math.min(maxScrollTop, scrollTop + rest), lines, carry: 0 };
}
