// The phone renders a Session at the Mac's grid (it never resizes the pty, so attaching disturbs
// nothing on the Mac) and picks the font size that fits that many columns across the screen.
// Pure maths; the component measures and calls.

/** Smallest font that is still legible on a phone; below it the grid scrolls sideways. */
export const MIN_FONT_SIZE = 6;
export const MAX_FONT_SIZE = 15;

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
