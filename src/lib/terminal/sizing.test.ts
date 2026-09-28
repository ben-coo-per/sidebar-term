import { describe, expect, it } from "vitest";
import { ask, granted, refused, resized, sizingAt, type Grid } from "./sizing";

const MAC: Grid = { cols: 212, rows: 60 };
const WIDER: Grid = { cols: 220, rows: 60 };
const PHONE: Grid = { cols: 63, rows: 32 };

describe("a Mac alone", () => {
  it("takes the Host's word for its own resize as no news", () => {
    let s = ask(sizingAt({ cols: 80, rows: 24 }), MAC);
    expect(s).toMatchObject({ pty: MAC, asked: [MAC], lent: false });
    s = granted(s, MAC);
    expect(resized(s, MAC)).toEqual({ sizing: { pty: MAC, known: MAC, asked: [], lent: false }, show: null });
  });

  it("stays at the last grid it asked for while its window is dragged", () => {
    let s = ask(ask(sizingAt({ cols: 80, rows: 24 }), MAC), WIDER);
    const first = resized(s, MAC);
    expect(first.show).toBeNull();
    expect(first.sizing).toMatchObject({ pty: WIDER, asked: [WIDER], lent: false });
    s = first.sizing;
    expect(resized(s, WIDER)).toEqual({ sizing: { pty: WIDER, known: WIDER, asked: [], lent: false }, show: null });
  });

  it("remembers no more asks than are ever on their way", () => {
    let s = sizingAt(MAC);
    for (let cols = 1; cols <= 20; cols++) s = ask(s, { cols, rows: 24 });
    expect(s.asked).toHaveLength(8);
    expect(s.asked.at(-1)).toEqual({ cols: 20, rows: 24 });
  });
});

describe("a phone that takes the size", () => {
  const held = resized(granted(ask(sizingAt(MAC), MAC), MAC), MAC).sizing;

  it("has the Mac's Terminal follow the pty", () => {
    expect(resized(held, PHONE)).toEqual({ sizing: { pty: PHONE, known: PHONE, asked: [], lent: true }, show: PHONE });
  });

  it("gives the size back, which the Terminal follows too", () => {
    const taken = resized(held, PHONE).sizing;
    expect(resized(taken, MAC)).toMatchObject({ sizing: { pty: MAC, known: MAC }, show: MAC });
  });

  it("loses it to the Mac once the Mac is used", () => {
    const taken = resized(held, PHONE).sizing;
    const back = ask(taken, MAC);
    expect(back).toMatchObject({ pty: MAC, lent: false });
    expect(resized(granted(back, MAC), MAC)).toEqual({ sizing: { pty: MAC, known: MAC, asked: [], lent: false }, show: null });
  });

  it("takes it while an ask of the Mac's is on its way, and the Mac's ask still ends at its grid", () => {
    const asking = ask(held, WIDER);
    const taken = resized(asking, PHONE);
    expect(taken).toMatchObject({ sizing: { pty: PHONE, lent: true, asked: [WIDER] }, show: PHONE });
    expect(resized(taken.sizing, WIDER)).toEqual({ sizing: { pty: WIDER, known: WIDER, asked: [], lent: false }, show: WIDER });
  });
});

describe("a Host that refuses", () => {
  it("has the Terminal go back to the pty's grid", () => {
    const s = ask(sizingAt(PHONE), MAC);
    expect(refused(s, MAC)).toEqual({ sizing: { pty: PHONE, known: PHONE, asked: [], lent: true }, show: PHONE });
  });

  it("leaves the Terminal to a grid asked for since", () => {
    const s = ask(ask(sizingAt(PHONE), MAC), WIDER);
    const first = refused(s, MAC);
    expect(first).toMatchObject({ sizing: { pty: WIDER, asked: [WIDER], lent: false }, show: null });
    expect(refused(first.sizing, WIDER)).toEqual({ sizing: { pty: PHONE, known: PHONE, asked: [], lent: true }, show: PHONE });
  });

  it("is not asked again until the Mac is used, and then gives way", () => {
    const lent = refused(ask(sizingAt(PHONE), MAC), MAC).sizing;
    const taking = ask(lent, MAC);
    expect(taking.lent).toBe(false);
    expect(resized(granted(taking, MAC), MAC).sizing).toEqual({ pty: MAC, known: MAC, asked: [], lent: false });
  });
});
