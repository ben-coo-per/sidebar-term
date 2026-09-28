import { describe, expect, it } from "vitest";
import { clampTextSize, DEFAULT_TEXT_SIZE, fontSizeGuess, fontSizeToFit, gridToFit, MAX_FONT_SIZE, MAX_TEXT_SIZE, MIN_COLS, MIN_FONT_SIZE, MIN_ROWS, MIN_TEXT_SIZE, sameGrid, shareDrag } from "./fit";

describe("fontSizeToFit", () => {
  it("scales the measured cell so the Host's columns fit the width", () => {
    // A 13px font measured 7.8px wide per cell: 0.6 px of cell per px of font.
    expect(fontSizeToFit(390, 80, 13, 7.8)).toBe(8); // 390 / (80 * 0.6) = 8.125
    expect(fontSizeToFit(844, 80, 13, 7.8)).toBe(15); // landscape: 17.6, capped
    expect(fontSizeToFit(390, 200, 13, 7.8)).toBe(MIN_FONT_SIZE); // 3.25: too small, scroll
  });

  it("never returns nonsense for degenerate input", () => {
    expect(fontSizeToFit(0, 80, 13, 7.8)).toBe(MIN_FONT_SIZE);
    expect(fontSizeToFit(390, 0, 13, 7.8)).toBe(MIN_FONT_SIZE);
    expect(fontSizeToFit(390, 80, 0, 7.8)).toBe(MIN_FONT_SIZE);
    expect(fontSizeToFit(390, 80, 13, 0)).toBe(MIN_FONT_SIZE);
    expect(fontSizeToFit(100_000, 1, 13, 7.8)).toBe(MAX_FONT_SIZE);
  });

  it("the first guess assumes 0.6 em cells", () => {
    expect(fontSizeGuess(390, 80)).toBe(8);
  });
});

describe("clampTextSize", () => {
  it("keeps a size the phone can read, in whole pixels", () => {
    expect(clampTextSize(12.4)).toBe(12);
    expect(clampTextSize(3)).toBe(MIN_TEXT_SIZE);
    expect(clampTextSize(90)).toBe(MAX_TEXT_SIZE);
    expect(clampTextSize(Number.NaN)).toBe(DEFAULT_TEXT_SIZE);
  });
});

describe("gridToFit", () => {
  it("counts the whole cells that fit", () => {
    // A phone in portrait at 12px: cells of 7.2 by 14.
    expect(gridToFit(382, 430, 7.2, 14)).toEqual({ cols: 53, rows: 30 });
    expect(gridToFit(836, 200, 7.2, 14)).toEqual({ cols: 116, rows: 14 });
  });

  it("never asks for a sliver, nor for anything before it has measured", () => {
    expect(gridToFit(50, 30, 7.2, 14)).toEqual({ cols: MIN_COLS, rows: MIN_ROWS });
    expect(gridToFit(0, 430, 7.2, 14)).toBeNull();
    expect(gridToFit(382, 430, 0, 14)).toBeNull();
  });
});

describe("sameGrid", () => {
  it("compares two grids, and nothing is the same as no grid", () => {
    expect(sameGrid({ cols: 53, rows: 30 }, { cols: 53, rows: 30 })).toBe(true);
    expect(sameGrid({ cols: 53, rows: 30 }, { cols: 53, rows: 31 })).toBe(false);
    expect(sameGrid(null, null)).toBe(false);
  });
});

describe("shareDrag", () => {
  const cell = 14;

  it("towards older output, scrolls the screen to its top, then the scrollback", () => {
    expect(shareDrag(30, 100, 400, 0, cell, 0)).toEqual({ scrollTop: 70, lines: 0, carry: 0 });
    // 20 px of screen left, then 36 px of scrollback: two lines and 8 px over.
    expect(shareDrag(56, 20, 400, 0, cell, 0)).toEqual({ scrollTop: 0, lines: -2, carry: 8 });
    expect(shareDrag(7, 0, 400, 2, cell, 8)).toEqual({ scrollTop: 0, lines: -1, carry: 1 });
  });

  it("coming back, scrolls the scrollback to its end, then the screen", () => {
    expect(shareDrag(-30, 0, 400, 10, cell, 0)).toEqual({ scrollTop: 0, lines: 2, carry: -2 });
    expect(shareDrag(-13, 0, 400, 8, cell, -2)).toEqual({ scrollTop: 0, lines: 1, carry: -1 });
    // One line of scrollback left: the other 16 px move the screen.
    expect(shareDrag(-30, 0, 400, 1, cell, 0)).toEqual({ scrollTop: 16, lines: 1, carry: 0 });
    expect(shareDrag(-500, 390, 400, 0, cell, 0)).toEqual({ scrollTop: 400, lines: 0, carry: 0 });
  });

  it("forgets what is left of a drag the other way", () => {
    expect(shareDrag(10, 0, 0, 0, cell, -9)).toEqual({ scrollTop: 0, lines: 0, carry: 10 });
    expect(shareDrag(-10, 0, 0, 5, cell, 9)).toEqual({ scrollTop: 0, lines: 0, carry: -10 });
  });

  it("does nothing before a cell is measured", () => {
    expect(shareDrag(50, 10, 400, 3, 0, 0)).toEqual({ scrollTop: 10, lines: 0, carry: 0 });
  });
});
