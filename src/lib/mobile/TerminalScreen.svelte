<!-- One Tab's Session on the phone: an xterm.js Terminal at the Mac's grid (the pty is never
     resized from here), font scaled to fit the width, fed by the Remote connection; typing and
     the key bar go back as input. The whole screen tracks the visual viewport so the key bar
     sits right above the on-screen keyboard. -->
<script lang="ts">
  import { Terminal } from "@xterm/xterm";
  import { Unicode11Addon } from "@xterm/addon-unicode11";
  import "@xterm/xterm/css/xterm.css";
  import { terminalOptions, TERMINAL_BACKGROUND } from "../terminal/theme";
  import { attachWebgl, type WebglRenderer } from "../terminal/webgl";
  import { AGENT_NAMES } from "../agentStatus";
  import KeyBar from "./KeyBar.svelte";
  import StatusBanner from "./StatusBanner.svelte";
  import { fontSizeGuess, fontSizeToFit } from "./fit";
  import type { SidebarTab } from "./protocol";
  import { closeTerminal, remoteClient } from "./store.svelte";

  let { tab }: { tab: SidebarTab } = $props();

  let screen: HTMLDivElement;
  let scroller: HTMLDivElement;
  let host: HTMLDivElement;
  let term: Terminal | null = null;
  let ended = $state(false);
  let ctrl = $state(false);
  let grid = $state({ cols: 0, rows: 0 });

  const subtitle = $derived(
    ended
      ? "Session ended"
      : tab.agent
        ? `${AGENT_NAMES[tab.agent]} · ${tab.status === "running" ? "working" : tab.status === "needs-input" ? "needs input" : "idle"}`
        : grid.cols
          ? `${grid.cols}×${grid.rows}`
          : "",
  );

  /** Ctrl-<key>: the control code of a letter or of @ [ \ ] ^ _ ; anything else passes through. */
  function withCtrl(data: string): string {
    if (data.length !== 1) return data;
    const code = data.toUpperCase().charCodeAt(0);
    return code >= 64 && code <= 95 ? String.fromCharCode(code - 64) : data;
  }

  /** Pick the font size at which the Mac's columns fit the width, from a measured cell. */
  function refit(t: Terminal) {
    const screenEl = host.querySelector<HTMLElement>(".xterm-screen");
    const width = scroller.clientWidth;
    if (!screenEl || !width || !t.cols) return;
    const cell = screenEl.clientWidth / t.cols;
    const size = fontSizeToFit(width, t.cols, t.options.fontSize ?? 13, cell);
    if (size !== t.options.fontSize) t.options.fontSize = size;
  }

  function nearBottom(): boolean {
    return scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 48;
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
    const sessionId = tab.sessionId;
    const client = remoteClient();
    if (sessionId === null || !client) {
      ended = true;
      return;
    }
    const t = new Terminal({
      ...terminalOptions,
      fontSize: fontSizeGuess(scroller.clientWidth, 80),
      cursorBlink: false,
      scrollback: 5000,
      cols: 80,
      rows: 24,
    });
    t.loadAddon(new Unicode11Addon());
    t.unicode.activeVersion = "11";
    t.open(host);
    term = t;
    let webgl: WebglRenderer | null = attachWebgl(t, () => refit(t));

    const offs = [
      client.on("attached", (id, cols, rows) => {
        if (id !== sessionId) return;
        // A replay follows (also after a reconnect): start from a clean grid.
        t.reset();
        t.resize(cols, rows);
        grid = { cols, rows };
        ended = false;
        requestAnimationFrame(() => refit(t));
      }),
      client.on("resized", (id, cols, rows) => {
        if (id !== sessionId) return;
        t.resize(cols, rows);
        grid = { cols, rows };
        requestAnimationFrame(() => refit(t));
      }),
      client.on("output", (id, bytes) => {
        if (id !== sessionId) return;
        const follow = nearBottom();
        t.write(bytes, () => {
          if (follow) scroller.scrollTop = scroller.scrollHeight;
        });
      }),
      client.on("exit", (id) => {
        if (id === sessionId) ended = true;
      }),
    ];
    const data = t.onData((d) => {
      if (ended) return;
      if (ctrl) {
        ctrl = false;
        client.input(sessionId, withCtrl(d));
      } else {
        client.input(sessionId, d);
      }
    });
    // Text the keyboard inserts without a keypress (iOS autocorrect, predictive text, dictation)
    // reaches xterm as an `input` event. xterm consumes it (and stops propagation) unless the
    // keydown before it was one it handled itself, e.g. Return; what it leaves lands here.
    const onInput = (ev: Event) => {
      const ie = ev as InputEvent;
      if (ended || ie.inputType !== "insertText" || !ie.data || ie.isComposing) return;
      client.input(sessionId, ctrl ? withCtrl(ie.data) : ie.data);
      ctrl = false;
      (ie.target as HTMLTextAreaElement).value = "";
    };
    host.addEventListener("input", onInput);
    const ro = new ResizeObserver(() => refit(t));
    ro.observe(scroller);
    client.attach(sessionId);

    return () => {
      host.removeEventListener("input", onInput);
      ro.disconnect();
      for (const off of offs) off();
      data.dispose();
      client.detach(sessionId);
      webgl?.release();
      webgl = null;
      t.dispose();
      term = null;
    };
  });

  function send(sequence: string) {
    if (ended) return;
    term?.input(sequence, true);
    term?.focus();
  }

  function toggleCtrl() {
    ctrl = !ctrl;
    term?.focus();
  }

  function focusTerminal() {
    term?.focus();
  }
</script>

<div class="screen" bind:this={screen}>
  <header>
    <button type="button" class="back" onclick={closeTerminal} aria-label="Back to the Tabs">‹</button>
    <div class="titles">
      <span class="title">{tab.title}</span>
      {#if subtitle}
        <span class="subtitle" class:ended>{subtitle}</span>
      {/if}
    </div>
    <button type="button" class="kbd" onclick={focusTerminal} aria-label="Show the keyboard">⌨</button>
  </header>
  <StatusBanner />
  <div class="scroller" bind:this={scroller} style:--terminal-bg={TERMINAL_BACKGROUND}>
    <div class="host" bind:this={host}></div>
    {#if ended}
      <div class="ended-note">This Session has ended.</div>
    {/if}
  </div>
  <KeyBar {send} {ctrl} onCtrl={toggleCtrl} applicationCursor={() => term?.modes.applicationCursorKeysMode ?? false} />
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
    gap: 6px;
    padding: env(safe-area-inset-top) 6px 0;
    min-height: calc(44px + env(safe-area-inset-top));
    border-bottom: 1px solid var(--sidebar-border);
    background: var(--sidebar-bg);
  }
  .back,
  .kbd {
    flex: none;
    appearance: none;
    width: 40px;
    height: 40px;
    border: none;
    background: transparent;
    color: var(--accent-strong);
    font: inherit;
    font-size: 30px;
    line-height: 1;
  }
  .kbd {
    font-size: 20px;
    color: var(--text-secondary);
  }
  .titles {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
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
  .scroller {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: 4px 0 0 4px;
    background: var(--terminal-bg);
    -webkit-overflow-scrolling: touch;
  }
  .host {
    width: max-content;
    min-width: 100%;
  }
  .ended-note {
    position: sticky;
    left: 0;
    bottom: 0;
    padding: 6px 10px;
    font-size: 13px;
    background: var(--danger-dim);
    color: var(--text-primary);
  }
</style>
