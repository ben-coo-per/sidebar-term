import { describe, expect, it } from "vitest";
import {
  clampFont,
  colorScheme,
  cssVariables,
  emptyAppearance,
  fontFamilies,
  fontStack,
  isCustom,
  luminance,
  parseAppearanceSection,
  parseColor,
  terminalLook,
  type Appearance,
  type UiDefaults,
} from "./model";
import { DEFAULT_TERMINAL_LOOK } from "../terminal/theme";

const DEFAULTS: UiDefaults = {
  colors: { "--sidebar-bg": "#15171c", "--accent": "#5b93f5" },
  fontUi: '-apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif',
  fontMono: '"SF Mono", ui-monospace, Menlo, monospace',
};

function custom(part: Partial<Appearance>): Appearance {
  return { ...emptyAppearance(), ...part };
}

describe("parseColor", () => {
  it("reads six and three digits, with or without the hash, in any case", () => {
    expect(parseColor("#5B93F5")).toBe("#5b93f5");
    expect(parseColor("5b93f5")).toBe("#5b93f5");
    expect(parseColor(" #abc ")).toBe("#aabbcc");
  });

  it("refuses what is no opaque hex colour", () => {
    for (const bad of ["", "#12", "#12345", "#5b93f533", "red", "#gggggg", "rgb(0,0,0)", 5, null, undefined]) {
      expect(parseColor(bad)).toBeNull();
    }
  });
});

describe("luminance", () => {
  it("runs from black to white", () => {
    expect(luminance("#000000")).toBe(0);
    expect(luminance("#ffffff")).toBeCloseTo(1);
    expect(luminance("#15171c")).toBeLessThan(0.02);
  });
});

describe("fontFamilies", () => {
  it("splits a list, unquotes and drops repeats", () => {
    expect(fontFamilies(`"JetBrains Mono",  'Fira  Code', menlo, Menlo, monospace`)).toEqual([
      "JetBrains Mono",
      "Fira Code",
      "menlo",
      "monospace",
    ]);
  });

  it("drops what could not be a font name", () => {
    expect(fontFamilies("Menlo; color: red, a{b}, url(x), ,")).toEqual([]);
    expect(fontFamilies("x".repeat(81))).toEqual([]);
    expect(fontFamilies("")).toEqual([]);
  });
});

describe("fontStack", () => {
  it("is the default when nothing is chosen", () => {
    expect(fontStack("", DEFAULTS.fontMono)).toBe(DEFAULTS.fontMono);
    expect(fontStack(";;", DEFAULTS.fontMono)).toBe(DEFAULTS.fontMono);
  });

  it("puts the chosen families first and keeps the default's as the fallback", () => {
    expect(fontStack("JetBrains Mono", DEFAULTS.fontMono)).toBe('"JetBrains Mono", "SF Mono", ui-monospace, "Menlo", monospace');
    expect(fontStack("Menlo", DEFAULTS.fontMono)).toBe('"Menlo", "SF Mono", ui-monospace, monospace');
  });

  it("leaves the names CSS knows unquoted", () => {
    expect(fontStack("Inter", DEFAULTS.fontUi)).toBe('"Inter", -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif');
  });
});

describe("clampFont", () => {
  it("keeps a number within its range and on its step", () => {
    expect(clampFont("terminalSize", 14)).toBe(14);
    expect(clampFont("terminalSize", 14.4)).toBe(14);
    expect(clampFont("terminalSize", 2)).toBe(8);
    expect(clampFont("terminalSize", 500)).toBe(32);
    expect(clampFont("uiScale", 1.13)).toBe(1.15);
    expect(clampFont("terminalLineHeight", 1.2)).toBe(1.2);
    expect(clampFont("terminalWeight", 449)).toBe(400);
  });

  it("is the default for what is no number", () => {
    expect(clampFont("terminalSize", "14")).toBe(13);
    expect(clampFont("uiScale", NaN)).toBe(1);
    expect(clampFont("terminalLineHeight", null)).toBe(1);
  });
});

describe("parseAppearanceSection", () => {
  it("is empty for nothing, or for something else", () => {
    for (const raw of [undefined, null, 3, "x", [], {}, { colors: [], terminal: 1, fonts: "Menlo" }]) {
      expect(parseAppearanceSection(raw)).toEqual(emptyAppearance());
    }
  });

  it("keeps known colours and drops the rest", () => {
    const parsed = parseAppearanceSection({
      colors: { "--accent": "#FF0000", "--accent-dim": "#ff0000", "--nope": "#ff0000", "--danger": "red" },
      terminal: { background: "#fff", cursorAccent: "#000000", brightBlack: "#777777", blue: 4 },
    });
    expect(parsed.colors).toEqual({ "--accent": "#ff0000" });
    expect(parsed.terminal).toEqual({ background: "#ffffff", brightBlack: "#777777" });
  });

  it("keeps fonts that differ from the default, within their ranges", () => {
    const parsed = parseAppearanceSection({
      fonts: { ui: " Inter ", mono: "", terminal: "a{b}", uiScale: 9, terminalSize: 13, terminalLineHeight: 1.2, terminalWeight: "bold" },
    });
    expect(parsed.fonts).toEqual({ ui: "Inter", uiScale: 1.4, terminalLineHeight: 1.2 });
  });

  it("reads back what it gave", () => {
    const a: Appearance = {
      colors: { "--accent": "#ff0000", "--repo-color-3": "#00ff00" },
      terminal: { foreground: "#eeeeee" },
      fonts: { terminal: "Fira Code, Menlo", terminalSize: 15, terminalWeight: 500 },
    };
    expect(parseAppearanceSection(JSON.parse(JSON.stringify(a)))).toEqual(a);
  });
});

describe("isCustom", () => {
  it("is whether anything was changed", () => {
    expect(isCustom(emptyAppearance())).toBe(false);
    expect(isCustom(custom({ fonts: { terminalSize: 15 } }))).toBe(true);
    expect(isCustom(custom({ terminal: { red: "#ff0000" } }))).toBe(true);
  });
});

describe("cssVariables", () => {
  it("leaves every token to the stylesheet when nothing is changed", () => {
    const vars = cssVariables(emptyAppearance(), DEFAULTS);
    expect(Object.values(vars).every((v) => v === null)).toBe(true);
    for (const name of ["--accent", "--accent-dim", "--term-bg", "--font-ui", "--font-mono", "--ui-font-scale", "--repo-color-7"]) {
      expect(vars).toHaveProperty(name);
    }
  });

  it("sets a colour and the tints that follow it", () => {
    const vars = cssVariables(custom({ colors: { "--accent": "#ff0000", "--status-needs-input": "#00ff00" } }), DEFAULTS);
    expect(vars["--accent"]).toBe("#ff0000");
    expect(vars["--accent-dim"]).toBe("#ff000033");
    expect(vars["--status-needs-input-dim"]).toBe("#00ff001a");
    expect(vars["--status-needs-input-border"]).toBe("#00ff0040");
    expect(vars["--danger"]).toBeNull();
    expect(vars["--danger-dim"]).toBeNull();
  });

  it("puts the Terminal's background behind the app", () => {
    expect(cssVariables(custom({ terminal: { background: "#101010" } }), DEFAULTS)["--term-bg"]).toBe("#101010");
  });

  it("sets the fonts", () => {
    const vars = cssVariables(custom({ fonts: { ui: "Inter", mono: "Fira Code", uiScale: 1.1 } }), DEFAULTS);
    expect(vars["--font-ui"]).toBe('"Inter", -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif');
    expect(vars["--font-mono"]).toBe('"Fira Code", "SF Mono", ui-monospace, "Menlo", monospace');
    expect(vars["--ui-font-scale"]).toBe("1.1");
  });
});

describe("colorScheme", () => {
  it("follows the sidebar's surface", () => {
    expect(colorScheme(emptyAppearance(), DEFAULTS)).toBe("dark");
    expect(colorScheme(custom({ colors: { "--sidebar-bg": "#f4f4f4" } }), DEFAULTS)).toBe("light");
  });
});

describe("terminalLook", () => {
  it("is the default look when nothing is changed", () => {
    expect(terminalLook(emptyAppearance())).toEqual(DEFAULT_TERMINAL_LOOK);
  });

  it("takes the colours and fonts chosen", () => {
    const look = terminalLook(
      custom({
        terminal: { background: "#ffffff", red: "#ff0000" },
        fonts: { terminal: "Fira Code", terminalSize: 15, terminalLineHeight: 1.2, terminalWeight: 500 },
      }),
    );
    expect(look.theme.background).toBe("#ffffff");
    expect(look.theme.cursorAccent).toBe("#ffffff");
    expect(look.theme.red).toBe("#ff0000");
    expect(look.theme.green).toBe(DEFAULT_TERMINAL_LOOK.theme.green);
    expect(look.fontFamily).toBe('"Fira Code", "SF Mono", ui-monospace, "Menlo", "Monaco", monospace');
    expect(look.fontSize).toBe(15);
    expect(look.lineHeight).toBe(1.2);
    expect(look.fontWeight).toBe("500");
  });
});
