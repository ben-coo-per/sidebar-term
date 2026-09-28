<!-- One Tab's Session on the phone: an xterm.js Terminal at a text size the phone can read, fed
     by the connection to its Host; the keyboard (the page's own, or the phone's under the key
     bar) goes back as input. The phone asks the Host to size the pty to what fits the screen at
     that size, and gives the size back when it leaves; while another client shows the Session
     the Host refuses, and the phone pans across the Host's grid instead (fit.ts). Drags scroll
     the screen and the scrollback as one, two fingers change the text size. The whole screen
     tracks the visual viewport so the key bar sits right above the phone's keyboard. -->
<script lang="ts">
  import { untrack } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { Unicode11Addon } from "@xterm/addon-unicode11";
  import "@xterm/xterm/css/xterm.css";
  import { terminalOptions, TERMINAL_BACKGROUND } from "../terminal/theme";
  import { attachWebgl, type WebglRenderer } from "../terminal/webgl";
  import { AGENT_NAMES } from "../agentStatus";
  import KeyBar from "./KeyBar.svelte";
  import Keyboard from "./Keyboard.svelte";
  import StatusBanner from "./StatusBanner.svelte";
  import { MAX_TEXT_SIZE, MIN_TEXT_SIZE, clampTextSize, fontSizeToFit, gridToFit, sameGrid, shareDrag, type Grid } from "./fit";
  import { withCtrl } from "./keys";
  import { prefs, setKeyboard, setKeyboardUp, setTextSize } from "./prefs.svelte";
  import type { TabRow } from "./rows";
  import { clientOf, closeTerminal } from "./store.svelte";

  let { tab }: { tab: TabRow } = $props();

  /** Around the grid, inside the scroller. */
  const PAD = 4;
  /** Rows kept in view under the cursor: what a program draws beneath its prompt. */
  const ROWS_BELOW_CURSOR = 3;
  /** A finger that moved less than this tapped. */
  const SLOP_PX = 8;
  /** How often a refused size is asked for again: the other client may have left. */
  const ASK_AGAIN_MS = 15_000;
  const NOTICE_MS = 3_000;
  /** The most lines one move of a finger wheels a program by. */
  const MOST_WHEEL_LINES = 12;

  let screen: HTMLDivElement;
  let scroller: HTMLDivElement;
  let host: HTMLDivElement;
  let term = $state.raw<Terminal | null>(null);
  let ended = $state(false);
  /** The key bar's Ctrl, armed for the next key of the phone's keyboard. */
  let ctrl = $state(false);
  /** The pty's size, as its Host says. */
  let grid = $state<Grid>({ cols: 0, rows: 0 });
  /** The Host would not size the pty for this phone: another client shows the Session. */
  let refused = $state(false);
  /** Refused: shrink the text until the Host's columns fit the width, rather than pan. */
  let fitWidth = $state(false);
  let options = $state(false);
  let notice = $state<string | null>(null);

  /** One cell of the grid, in CSS pixels, as last measured. */
  let cell: { w: number; h: number } | null = null;
  /** The view keeps the cursor in sight until a drag takes it away. */
  let follow = true;
  /** Asks the Host for a grid; set while a Session is attached. */
  let ask: ((want: Grid) => void) | null = null;
  /** Two fingers are changing the text size: the Host is asked once they lift. */
  let pinching = false;
  let frame = 0;
  let fontChanges = 0;
  let noticeTimer: ReturnType<typeof setTimeout> | null = null;

  // The row is made anew whenever its Host says anything: the Terminal follows what it is of.
  const sessionId = $derived(tab.sessionId);
  const hostId = $derived(tab.host);

  const what = $derived(
    ended
      ? "Session ended"
      : tab.agent
        ? `${AGENT_NAMES[tab.agent]} · ${tab.status === "running" ? "working" : tab.status === "needs-input" ? "needs input" : "idle"}`
        : grid.cols
          ? `${grid.cols}×${grid.rows}`
          : "",
  );
  const subtitle = $derived(tab.hostName && !ended ? [tab.hostName, what].filter(Boolean).join(" · ") : what);

  function say(message: string) {
    notice = message;
    if (noticeTimer) clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), NOTICE_MS);
  }

  function measure(t: Terminal): { w: number; h: number } | null {
    const el = host.querySelector<HTMLElement>(".xterm-screen");
    if (!el || !t.cols || !t.rows || !el.clientWidth || !el.clientHeight) return null;
    return { w: el.clientWidth / t.cols, h: el.clientHeight / t.rows };
  }

  /** Where the screen's own scroll shows the cursor, with the rows under it. */
  function cursorTop(t: Terminal, c: { h: number }): number {
    const max = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
    const top = (t.buffer.active.cursorY + 1 + ROWS_BELOW_CURSOR) * c.h + PAD - scroller.clientHeight;
    return Math.max(0, Math.min(max, top));
  }

  /** Bring the cursor into view: down the screen while the view follows, and across it after typing. */
  function reveal(across = false) {
    const t = term;
    if (!t || !cell) return;
    if (follow) scroller.scrollTop = cursorTop(t, cell);
    if (!across) return;
    const x = t.buffer.active.cursorX * cell.w;
    if (x < scroller.scrollLeft) scroller.scrollLeft = Math.max(0, x - 2 * cell.w);
    else if (x + 2 * cell.w > scroller.scrollLeft + scroller.clientWidth) scroller.scrollLeft = x + 4 * cell.w - scroller.clientWidth;
  }

  /** Set the text size, then ask the Host for the grid that fits at the size the phone reads. */
  function layoutNow() {
    const t = term;
    if (!t || !grid.cols || document.visibilityState === "hidden") return;
    const width = scroller.clientWidth - 2 * PAD;
    const height = scroller.clientHeight - PAD;
    const measured = measure(t);
    if (!measured || width <= 0 || height <= 0) return;
    const size = t.options.fontSize ?? prefs.textSize;
    const font = refused && fitWidth ? fontSizeToFit(width, grid.cols, size, measured.w) : prefs.textSize;
    // A cell rounds to whole device pixels, so a fit may hop between two sizes: settle on one.
    if (font !== size && fontChanges < 3) {
      fontChanges += 1;
      t.options.fontSize = font;
      layout();
      return;
    }
    fontChanges = 0;
    cell = measured;
    const scale = prefs.textSize / size;
    const want = gridToFit(width, height, measured.w * scale, measured.h * scale);
    if (want && !sameGrid(want, grid)) {
      if (!pinching) ask?.(want);
    } else if (want) refused = false;
    reveal();
  }

  function layout() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      layoutNow();
    });
  }

  // The visual viewport shrinks when the keyboard shows; size the screen to it.
  $effect(() => {
    const vv = window.visualViewport;
    if (!vv) return;
    const apply = () => {
      screen.style.height = `${vv.height}px`;
      screen.style.top = `${vv.offsetTop}px`;
      window.scrollTo(0, 0);
    };
    apply();
    vv.addEventListener("resize", apply);
    vv.addEventListener("scroll", apply);
    return () => {
      vv.removeEventListener("resize", apply);
      vv.removeEventListener("scroll", apply);
    };
  });

  $effect(() => {
    const id = sessionId;
    const client = clientOf(hostId);
    if (id === null || !client) {
      ended = true;
      return;
    }
    const t = new Terminal({
      ...terminalOptions,
      fontSize: untrack(() => prefs.textSize),
      cursorBlink: false,
      // With the keyboard down the Terminal has no focus: the cursor stays as it was.
      cursorInactiveStyle: "block",
      scrollback: 5000,
      cols: 80,
      rows: 24,
    });
    t.loadAddon(new Unicode11Addon());
    t.unicode.activeVersion = "11";
    t.open(host);
    term = t;
    follow = true;
    let webgl: WebglRenderer | null = attachWebgl(t, layout);

    /** The pty's size before this phone sized it: given back when the phone leaves. */
    let found: Grid | null = null;
    let asking = false;
    let queued = false;
    /** The grid the Host last refused, and when: not asked for again at once. */
    let denied: { want: Grid; at: number } | null = null;
    const size = (want: Grid) => client.command({ t: "resize", sessionId: id, cols: want.cols, rows: want.rows });
    ask = (want) => {
      if (asking) {
        queued = true;
        return;
      }
      if (denied && sameGrid(denied.want, want) && Date.now() - denied.at < ASK_AGAIN_MS - 1000) return;
      asking = true;
      const before = { ...grid };
      size(want)
        .then(() => {
          found ??= before;
          denied = null;
          refused = false;
        })
        // Shown by another client (or the connection dropped, and the next attach asks again).
        .catch(() => {
          denied = { want, at: Date.now() };
          refused = true;
        })
        .finally(() => {
          asking = false;
          if (queued) layout();
          queued = false;
        });
    };
    /** Give the pty the size it had; the phone is not looking. */
    const giveBack = () => {
      if (!found || ended) return;
      const to = found;
      found = null;
      size(to).catch(() => {});
    };
    const onVisibility = () => {
      if (document.visibilityState === "hidden") giveBack();
      else layout();
    };
    document.addEventListener("visibilitychange", onVisibility);
    const again = setInterval(() => {
      if (refused) layout();
    }, ASK_AGAIN_MS);

    const sized = (cols: number, rows: number) => {
      t.resize(cols, rows);
      grid = { cols, rows };
      ended = false;
      layout();
    };
    const offs = [
      client.on("attached", (sid, cols, rows) => {
        if (sid !== id) return;
        // A replay follows (also after a reconnect): start from a clean grid.
        t.reset();
        follow = true;
        denied = null;
        sized(cols, rows);
      }),
      client.on("resized", (sid, cols, rows) => {
        if (sid === id) sized(cols, rows);
      }),
      client.on("output", (sid, bytes) => {
        if (sid === id) t.write(bytes, () => reveal());
      }),
      client.on("exit", (sid) => {
        if (sid === id) ended = true;
      }),
    ];
    const data = t.onData((d) => {
      if (ended) return;
      if (ctrl) {
        ctrl = false;
        client.input(id, withCtrl(d));
      } else {
        client.input(id, d);
      }
      follow = true;
      reveal(true);
    });
    // Text the keyboard inserts without a keypress (iOS autocorrect, predictive text, dictation)
    // reaches xterm as an `input` event. xterm consumes it (and stops propagation) unless the
    // keydown before it was one it handled itself, e.g. Return; what it leaves lands here.
    const onInput = (ev: Event) => {
      const ie = ev as InputEvent;
      if (ended || ie.inputType !== "insertText" || !ie.data || ie.isComposing) return;
      client.input(id, ctrl ? withCtrl(ie.data) : ie.data);
      ctrl = false;
      (ie.target as HTMLTextAreaElement).value = "";
    };
    host.addEventListener("input", onInput);
    const ro = new ResizeObserver(layout);
    ro.observe(scroller);
    client.attach(id);

    return () => {
      giveBack();
      clearInterval(again);
      document.removeEventListener("visibilitychange", onVisibility);
      host.removeEventListener("input", onInput);
      ro.disconnect();
      for (const off of offs) off();
      data.dispose();
      client.detach(id);
      ask = null;
      if (frame) cancelAnimationFrame(frame);
      frame = 0;
      cell = null;
      webgl?.release();
      webgl = null;
      t.dispose();
      term = null;
    };
  });

  // The text size, or how a refused grid is shown, changed.
  $effect(() => {
    void [prefs.textSize, fitWidth, refused];
    untrack(layout);
  });

  // Which keyboard: the phone's comes up only for a focused Terminal that takes text. Under the
  // page's own keyboard the Terminal has the focus too (it draws the cursor), and takes none.
  $effect(() => {
    const t = term;
    const area = t?.textarea;
    if (!t || !area) return;
    area.inputMode = prefs.keyboard === "system" && prefs.keyboardUp ? "text" : "none";
    if (prefs.keyboardUp) t.focus();
    else t.blur();
  });

  // --- Fingers ----------------------------------------------------------------------------------

  /**
   * A drag in an alternate screen has no scrollback to move: the program gets a wheel, a line
   * at a time (negative: up). xterm makes of it what the program asked for: mouse reports, or arrows.
   */
  function wheel(lines: number) {
    const el = host.querySelector<HTMLElement>(".xterm-screen");
    if (!el) return;
    const box = el.getBoundingClientRect();
    const at = { clientX: box.left + box.width / 2, clientY: box.top + Math.min(box.height, scroller.clientHeight) / 2 };
    const deltaY = Math.sign(lines);
    for (let i = Math.min(Math.abs(lines), MOST_WHEEL_LINES); i > 0; i--) {
      el.dispatchEvent(new WheelEvent("wheel", { deltaY, deltaMode: WheelEvent.DOM_DELTA_LINE, bubbles: true, cancelable: true, ...at }));
    }
  }

  let carry = 0;

  /** Move the view by a drag: `dx` across the grid, `dy` down the output (positive: towards older). */
  function drag(dx: number, dy: number) {
    const t = term;
    if (!t || !cell) return;
    if (dx) scroller.scrollLeft -= dx;
    if (!dy) return;
    const max = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
    const buffer = t.buffer.active;
    if (buffer.type === "alternate") {
      const top = Math.max(0, Math.min(max, scroller.scrollTop - dy));
      carry += dy - (scroller.scrollTop - top);
      scroller.scrollTop = top;
      const lines = Math.trunc(carry / cell.h);
      carry -= lines * cell.h;
      if (lines) wheel(-lines);
      follow = top >= cursorTop(t, cell) - 1;
      return;
    }
    const moved = shareDrag(dy, scroller.scrollTop, max, buffer.baseY - buffer.viewportY, cell.h, carry);
    carry = moved.carry;
    scroller.scrollTop = moved.scrollTop;
    if (moved.lines) t.scrollLines(moved.lines);
    follow = t.buffer.active.viewportY >= t.buffer.active.baseY && moved.scrollTop >= cursorTop(t, cell) - 1;
  }

  $effect(() => {
    const el = scroller;
    let finger: { x: number; y: number; at: number; axis: "x" | "y" | null; vx: number; vy: number } | null = null;
    let pinch: { apart: number; size: number } | null = null;
    let glide = 0;

    const apart = (e: TouchEvent) => Math.hypot(e.touches[0].clientX - e.touches[1].clientX, e.touches[0].clientY - e.touches[1].clientY);
    const stop = () => {
      if (glide) cancelAnimationFrame(glide);
      glide = 0;
    };
    const unpinch = () => {
      if (!pinch) return;
      pinch = null;
      pinching = false;
      layout();
    };

    const start = (e: TouchEvent) => {
      stop();
      if (e.touches.length === 2) {
        finger = null;
        pinch = { apart: apart(e), size: prefs.textSize };
        pinching = true;
        e.preventDefault();
        return;
      }
      unpinch();
      carry = 0;
      const p = e.touches[0];
      finger = { x: p.clientX, y: p.clientY, at: e.timeStamp, axis: null, vx: 0, vy: 0 };
    };

    const move = (e: TouchEvent) => {
      if (pinch && e.touches.length === 2) {
        e.preventDefault();
        if (pinch.apart > 0) setTextSize(clampTextSize((pinch.size * apart(e)) / pinch.apart));
        return;
      }
      if (!finger || e.touches.length !== 1) return;
      const p = e.touches[0];
      const dx = p.clientX - finger.x;
      const dy = p.clientY - finger.y;
      if (!finger.axis) {
        if (Math.abs(dx) < SLOP_PX && Math.abs(dy) < SLOP_PX) return;
        finger.axis = Math.abs(dx) > Math.abs(dy) ? "x" : "y";
      }
      e.preventDefault();
      const dt = Math.max(1, e.timeStamp - finger.at);
      const along = finger.axis === "x" ? dx : dy;
      if (finger.axis === "x") finger.vx = 0.7 * (along / dt) + 0.3 * finger.vx;
      else finger.vy = 0.7 * (along / dt) + 0.3 * finger.vy;
      finger.x = p.clientX;
      finger.y = p.clientY;
      finger.at = e.timeStamp;
      drag(finger.axis === "x" ? dx : 0, finger.axis === "y" ? dy : 0);
    };

    const end = (e: TouchEvent) => {
      if (pinch) {
        if (e.touches.length < 2) unpinch();
        e.preventDefault();
        return;
      }
      const was = finger;
      finger = null;
      if (!was) return;
      if (!was.axis) {
        tapped();
        return;
      }
      // A drag is not a click: nothing reaches the program under the finger.
      e.preventDefault();
      // Lifted after a pause, the view stays where it was put.
      if (e.timeStamp - was.at > 80) return;
      let vx = was.vx;
      let vy = was.vy;
      let last = performance.now();
      const step = (now: number) => {
        const dt = Math.min(48, now - last);
        last = now;
        drag(vx * dt, vy * dt);
        const slow = Math.pow(0.95, dt / 16);
        vx *= slow;
        vy *= slow;
        glide = Math.abs(vx) + Math.abs(vy) > 0.03 ? requestAnimationFrame(step) : 0;
      };
      glide = requestAnimationFrame(step);
    };

    const cancel = () => {
      finger = null;
      unpinch();
    };

    el.addEventListener("touchstart", start, { passive: false });
    el.addEventListener("touchmove", move, { passive: false });
    el.addEventListener("touchend", end, { passive: false });
    el.addEventListener("touchcancel", cancel);
    return () => {
      stop();
      pinching = false;
      el.removeEventListener("touchstart", start);
      el.removeEventListener("touchmove", move);
      el.removeEventListener("touchend", end);
      el.removeEventListener("touchcancel", cancel);
    };
  });

  // --- Keys -------------------------------------------------------------------------------------

  /** A tap on the Terminal brings the keyboard up. */
  function tapped() {
    options = false;
    if (ended) return;
    if (!prefs.keyboardUp) setKeyboardUp(true);
    if (prefs.keyboard === "system") focusSystem();
  }

  /** Raise the phone's keyboard. Only a tap may: the phone ignores a focus it did not see asked for. */
  function focusSystem() {
    const area = term?.textarea;
    if (!area) return;
    area.inputMode = "text";
    term?.focus();
  }

  function toggleKeyboard() {
    options = false;
    setKeyboardUp(!prefs.keyboardUp);
    if (prefs.keyboardUp && prefs.keyboard === "system") focusSystem();
  }

  function chooseKeyboard(kind: "keys" | "system") {
    setKeyboard(kind);
    setKeyboardUp(true);
    if (kind === "system") focusSystem();
  }

  function send(sequence: string) {
    if (ended) return;
    term?.input(sequence, true);
    term?.focus();
  }

  function toggleCtrl() {
    ctrl = !ctrl;
    term?.focus();
  }

  async function paste() {
    if (ended) return;
    try {
      const text = await navigator.clipboard.readText();
      if (text) term?.paste(text);
      else say("Nothing to paste.");
    } catch {
      say("The phone did not let this page read what was copied.");
    }
  }
</script>

<div class="screen" bind:this={screen}>
  <header>
    <button type="button" class="back" onclick={closeTerminal} aria-label="Back to the lists">‹</button>
    <div class="titles">
      <span class="title">{tab.title}</span>
      {#if subtitle}
        <span class="subtitle" class:ended>{subtitle}</span>
      {/if}
    </div>
    <button type="button" class="tool" class:on={options} onclick={() => (options = !options)} aria-label="Text size and keyboard" aria-expanded={options}>
      Aa
    </button>
    <button type="button" class="tool" class:on={prefs.keyboardUp} onclick={toggleKeyboard} aria-label={prefs.keyboardUp ? "Hide the keyboard" : "Show the keyboard"} aria-pressed={prefs.keyboardUp}>
      ⌨
    </button>
  </header>
  <StatusBanner host={tab.host} />
  {#if refused && !ended}
    <div class="shared" role="status">
      {grid.cols}×{grid.rows}, as another client shows it.
      {fitWidth ? "Shrunk to fit." : "Drag sideways to read."}
    </div>
  {/if}
  <div class="scroller" bind:this={scroller} style:--terminal-bg={TERMINAL_BACKGROUND} style:--pad="{PAD}px">
    <div class="host" bind:this={host}></div>
    {#if ended}
      <div class="ended-note">This Session has ended.</div>
    {/if}
  </div>
  {#if options}
    <div class="options">
      <div class="option">
        <span>Text size</span>
        <div class="stepper">
          <button type="button" onclick={() => setTextSize(prefs.textSize - 1)} disabled={prefs.textSize <= MIN_TEXT_SIZE} aria-label="Smaller text">−</button>
          <output>{prefs.textSize}</output>
          <button type="button" onclick={() => setTextSize(prefs.textSize + 1)} disabled={prefs.textSize >= MAX_TEXT_SIZE} aria-label="Larger text">+</button>
        </div>
      </div>
      <div class="option">
        <span>Keyboard</span>
        <div class="choice">
          <button type="button" class:chosen={prefs.keyboard === "keys"} onclick={() => chooseKeyboard("keys")}>Terminal</button>
          <button type="button" class:chosen={prefs.keyboard === "system"} onclick={() => chooseKeyboard("system")}>Phone's</button>
        </div>
      </div>
      {#if refused}
        <div class="option">
          <span>{grid.cols} columns</span>
          <div class="choice">
            <button type="button" class:chosen={!fitWidth} onclick={() => (fitWidth = false)}>Readable</button>
            <button type="button" class:chosen={fitWidth} onclick={() => (fitWidth = true)}>Fit the width</button>
          </div>
        </div>
      {/if}
    </div>
  {/if}
  {#if notice}
    <div class="notice" role="status"><span>{notice}</span></div>
  {/if}
  {#if prefs.keyboardUp && !ended}
    {#if prefs.keyboard === "keys"}
      <Keyboard {send} {paste} applicationCursor={() => term?.modes.applicationCursorKeysMode ?? false} />
    {:else}
      <KeyBar {send} {ctrl} onCtrl={toggleCtrl} applicationCursor={() => term?.modes.applicationCursorKeysMode ?? false} />
    {/if}
  {/if}
</div>

<style>
  .screen {
    position: fixed;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--term-bg);
  }
  header {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: env(safe-area-inset-top) max(6px, env(safe-area-inset-right)) 0 max(6px, env(safe-area-inset-left));
    min-height: calc(44px + env(safe-area-inset-top));
    border-bottom: 1px solid var(--sidebar-border);
    background: var(--sidebar-bg);
  }
  .back,
  .tool {
    flex: none;
    appearance: none;
    width: 40px;
    height: 40px;
    padding: 0;
    border: none;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--accent-strong);
    font: inherit;
    font-size: 30px;
    line-height: 1;
    touch-action: manipulation;
  }
  .tool {
    font-size: 17px;
    font-weight: 600;
    color: var(--text-secondary);
  }
  .tool.on {
    color: var(--accent-strong);
  }
  .titles {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    /* The two tools on the right weigh more than the one on the left: keep the title centred. */
    padding-left: 42px;
  }
  .title {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 15px;
    font-weight: 600;
  }
  .subtitle {
    font-size: 11px;
    color: var(--text-tertiary);
  }
  .subtitle.ended {
    color: var(--danger);
  }
  /* Over the Terminal, not above it: the grid stays as it is while they show. */
  .options {
    position: absolute;
    top: calc(45px + env(safe-area-inset-top));
    left: 0;
    right: 0;
    z-index: 2;
    box-shadow: 0 8px 16px #00000066;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px max(16px, env(safe-area-inset-right)) 12px max(16px, env(safe-area-inset-left));
    border-bottom: 1px solid var(--sidebar-border);
    background: var(--sidebar-bg-raised);
    font-size: 15px;
  }
  .option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .option > span {
    color: var(--text-secondary);
  }
  .stepper,
  .choice {
    display: flex;
    align-items: center;
    overflow: hidden;
    border: 1px solid var(--sidebar-border);
    border-radius: var(--radius-md);
    background: var(--sidebar-bg);
  }
  .stepper button,
  .choice button {
    appearance: none;
    min-width: 48px;
    height: 36px;
    padding: 0 12px;
    border: none;
    background: transparent;
    color: var(--text-primary);
    font: inherit;
    touch-action: manipulation;
  }
  .stepper button {
    font-size: 20px;
  }
  .stepper button:disabled {
    color: var(--text-tertiary);
  }
  .stepper output {
    min-width: 32px;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }
  .choice button.chosen {
    background: var(--accent-dim);
    color: var(--accent-strong);
  }
  .shared,
  .notice {
    flex: none;
    padding: 6px max(12px, env(safe-area-inset-right)) 6px max(12px, env(safe-area-inset-left));
    font-size: 12px;
    background: var(--sidebar-bg-raised);
    color: var(--text-secondary);
  }
  .notice {
    height: 0;
    padding-block: 0;
    overflow: visible;
    color: var(--text-primary);
  }
  .notice span {
    display: block;
    width: max-content;
    max-width: 100%;
    margin: 0 auto;
    padding: 8px 12px;
    transform: translateY(calc(-100% - 8px));
    border-radius: var(--radius-md);
    background: var(--sidebar-bg-active);
    box-shadow: 0 2px 8px #00000080;
  }
  .scroller {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    /* Fingers move the view (drag): the screen and the scrollback scroll as one. */
    overflow: hidden;
    touch-action: none;
    padding: var(--pad) 0 0 var(--pad);
    background: var(--terminal-bg);
  }
  .host {
    width: max-content;
    min-width: 100%;
  }
  .ended-note {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    padding: 6px 10px;
    font-size: 13px;
    background: var(--danger-dim);
    color: var(--text-primary);
  }
</style>
