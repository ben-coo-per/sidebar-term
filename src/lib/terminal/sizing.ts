// Whose size a Session's pty has, as one Terminal on the Mac knows it (ADR 0007). The Mac sizes
// the pty to what fits its Terminal; another client that shows the Session (a phone) may take
// the size, and the Mac's Terminal then draws the pty's grid, not what fits, until the Mac is
// used again. Pure: `manager.ts` measures, asks the Host and resizes the Terminal.

export interface Grid {
  cols: number;
  rows: number;
}

export interface Sizing {
  /** The grid the pty was last told to take, or that its Host said it has. */
  pty: Grid;
  /** The pty's grid as its Host last said or granted: what a Terminal goes back to, refused. */
  known: Grid;
  /** Grids asked of the Host and not heard back about, oldest first. */
  asked: Grid[];
  /** Another client has the size: the Terminal keeps the pty's grid until this Mac is used. */
  lent: boolean;
}

/** What the Terminal is to do about a change: take `show` as its grid, or stay as it is (null). */
export interface Change {
  sizing: Sizing;
  show: Grid | null;
}

/** The asks remembered: more than are ever on their way at once. */
const ASKED_MEMORY = 8;

export function sameGrid(a: Grid, b: Grid): boolean {
  return a.cols === b.cols && a.rows === b.rows;
}

/** A pty at `grid`, as its Host said: nothing asked, nothing lent. */
export function sizingAt(grid: Grid): Sizing {
  return { pty: grid, known: grid, asked: [], lent: false };
}

/** This Mac asks the Host for `want`, its Terminal at that grid already. */
export function ask(s: Sizing, want: Grid): Sizing {
  return { ...s, pty: want, asked: [...s.asked, want].slice(-ASKED_MEMORY), lent: false };
}

/** The Host sized the pty to `want`. */
export function granted(s: Sizing, want: Grid): Sizing {
  return { ...s, known: want };
}

/**
 * The Host would not size the pty to `want`: another client shows the Session, and this Mac
 * did not take the size. The Terminal goes back to the pty's grid, unless another grid was
 * asked for since, whose answer decides.
 */
export function refused(s: Sizing, want: Grid): Change {
  const at = s.asked.findLastIndex((g) => sameGrid(g, want));
  const asked = at < 0 ? s.asked : s.asked.filter((_, i) => i !== at);
  if (!sameGrid(s.pty, want)) return { sizing: { ...s, asked }, show: null };
  return { sizing: { ...s, asked, pty: s.known, lent: true }, show: sameGrid(s.known, want) ? null : s.known };
}

/**
 * The Host says the pty is at `grid`. This Mac's own doing, when it asked for that grid: no
 * news, unless what it took for the pty's grid has changed since. Anything else is another
 * client's, which took the size: the Terminal follows.
 */
export function resized(s: Sizing, grid: Grid): Change {
  const at = s.asked.findIndex((g) => sameGrid(g, grid));
  if (at < 0) return { sizing: { ...s, pty: grid, known: grid, lent: true }, show: grid };
  const asked = s.asked.slice(at + 1);
  // Asks made after this one are on their way: theirs is the grid to end at.
  if (asked.length > 0) return { sizing: { ...s, known: grid, asked }, show: null };
  return { sizing: { pty: grid, known: grid, asked, lent: false }, show: sameGrid(s.pty, grid) ? null : grid };
}
