import { describe, expect, it } from "vitest";
import { fontSizeGuess, fontSizeToFit, MAX_FONT_SIZE, MIN_FONT_SIZE } from "./fit";

describe("fontSizeToFit", () => {
  it("scales the measured cell so the Mac's columns fit the width", () => {
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
