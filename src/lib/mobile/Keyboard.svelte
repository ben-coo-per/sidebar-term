<!-- The phone's own keyboard for a Terminal, in place of the phone's: Esc, Tab, Ctrl, Alt and the
     arrows above, what a shell is typed with under them, then letters, numbers or symbols
     (keys.ts). A key acts as the finger lands, and again and again while held where that helps.
     A finger between two keys, or off the end of a row, means the key nearest it: no touch is
     lost to a gap. Nothing here takes the focus from the Terminal. -->
<script lang="ts">
  import { LAYERS, NO_MODS, SYMBOL_ROW, TOP_ROW, keyAt, labelOf, press, type DrawnRow, type Key, type Layer, type Mods } from "./keys";

  let {
    send,
    paste,
    applicationCursor,
  }: {
    send: (sequence: string) => void;
    paste: () => void;
    /** DECCKM: arrows send `\x1bOA` instead of `\x1b[A`. */
    applicationCursor: () => boolean;
  } = $props();

  /** A held key sends again after this long, then this often. */
  const REPEAT_AFTER_MS = 380;
  const REPEAT_EVERY_MS = 55;
  /** Shift tapped again within this long locks. */
  const LOCK_WITHIN_MS = 350;

  let board: HTMLDivElement;
  let layer = $state<Layer>("letters");
  let mods = $state<Mods>(NO_MODS);
  /** The keys under a finger, by where they are. */
  let down = $state<Record<string, true>>({});

  let shiftAt = 0;
  const repeats = new Map<string, ReturnType<typeof setTimeout>>();
  /** The key each finger landed on, by where the key is. */
  const fingers = new Map<number, string>();

  const rows = $derived<{ name: string; keys: Key[]; strip: boolean }[]>([
    { name: "top", keys: TOP_ROW, strip: true },
    { name: "shell", keys: SYMBOL_ROW, strip: true },
    ...LAYERS[layer].map((keys, i) => ({ name: `${layer}-${i}`, keys, strip: false })),
  ]);

  const keys = $derived(new Map<string, Key>(rows.flatMap((row) => row.keys.map((key): [string, Key] => [`${row.name}:${key.id}`, key]))));

  /** A row narrower than the keyboard sits in the middle of it. */
  function inset(keys: Key[]): string {
    const units = keys.reduce((n, k) => n + (k.width ?? 1), 0);
    return `${Math.max(0, (10 - units) / 2) * 10}%`;
  }

  function act(key: Key) {
    const now = Date.now();
    const again = key.action.kind === "shift" && now - shiftAt < LOCK_WITHIN_MS;
    if (key.action.kind === "shift") shiftAt = now;
    const out = press(key, mods, layer, applicationCursor(), again);
    mods = out.mods;
    if (out.layer !== layer) {
      // The keys under the fingers go with their layer, and never hear the finger lift.
      for (const at of Object.keys(down)) release(at);
      layer = out.layer;
    }
    if (out.paste) paste();
    else if (out.send !== null) send(out.send);
  }

  function release(at: string) {
    delete down[at];
    const timer = repeats.get(at);
    if (timer !== undefined) clearTimeout(timer);
    repeats.delete(at);
  }

  /** Where the key a finger means is: the key it landed on, or the nearest as drawn. */
  function under(e: PointerEvent): string | null {
    const on = e.target instanceof Element ? e.target.closest<HTMLElement>(".key") : null;
    if (on?.dataset.at) return on.dataset.at;
    const drawn: DrawnRow<string>[] = [...board.querySelectorAll(".row")].map((row) => {
      const box = row.getBoundingClientRect();
      const drawnKeys = [...row.querySelectorAll<HTMLElement>(".key")].map((el) => {
        const { left, right } = el.getBoundingClientRect();
        return { left, right, key: el.dataset.at ?? "" };
      });
      return { top: box.top, bottom: box.bottom, keys: drawnKeys };
    });
    return keyAt(e.clientX, e.clientY, drawn);
  }

  function land(e: PointerEvent) {
    // Keep the focus where it is, on the Terminal.
    e.preventDefault();
    const at = under(e);
    const key = at === null ? undefined : keys.get(at);
    if (at === null || !key) return;
    try {
      // The lift comes here wherever the pointer has gone by then.
      board.setPointerCapture(e.pointerId);
    } catch {
      /* not a pointer that can be held: its lift is heard if it is over the keyboard */
    }
    release(at);
    act(key);
    if (key.action.kind === "layer") return;
    down[at] = true;
    fingers.set(e.pointerId, at);
    if (!key.repeat) return;
    const again = () => {
      act(key);
      repeats.set(at, setTimeout(again, REPEAT_EVERY_MS));
    };
    repeats.set(at, setTimeout(again, REPEAT_AFTER_MS));
  }

  function armed(key: Key): boolean {
    const kind = key.action.kind;
    return (kind === "shift" && mods.shift !== "off") || (kind === "ctrl" && mods.ctrl) || (kind === "alt" && mods.alt);
  }

  function lift(e: PointerEvent) {
    const at = fingers.get(e.pointerId);
    fingers.delete(e.pointerId);
    if (at !== undefined) release(at);
  }

  $effect(() => {
    const el = board;
    el.addEventListener("pointerdown", land);
    el.addEventListener("pointerup", lift);
    el.addEventListener("pointercancel", lift);
    return () => {
      el.removeEventListener("pointerdown", land);
      el.removeEventListener("pointerup", lift);
      el.removeEventListener("pointercancel", lift);
      for (const timer of repeats.values()) clearTimeout(timer);
      repeats.clear();
      fingers.clear();
    };
  });
</script>

<div class="keyboard" role="group" aria-label="Terminal keyboard" bind:this={board} oncontextmenu={(e) => e.preventDefault()}>
  {#each rows as row (row.name)}
    <div class="row" class:strip={row.strip} class:shell={row.name === "shell"} style:padding-inline={inset(row.keys)}>
      {#each row.keys as key (key.id)}
        {@const at = `${row.name}:${key.id}`}
        {@const label = labelOf(key, mods)}
        <button
          type="button"
          tabindex="-1"
          class="key"
          data-at={at}
          class:quiet={key.quiet}
          class:down={down[at]}
          class:armed={armed(key)}
          class:locked={key.action.kind === "shift" && mods.shift === "lock"}
          class:word={label.length > 1}
          style:flex-grow={key.width ?? 1}
          aria-label={key.title}
          aria-pressed={armed(key)}
        >
          {label}
          {#if down[at] && !row.strip && key.action.kind === "text" && label.trim().length === 1}
            <span class="pop" aria-hidden="true">{label}</span>
          {/if}
        </button>
      {/each}
    </div>
  {/each}
</div>

<style>
  .keyboard {
    --key-height: 42px;
    --strip-height: 34px;
    --gap: 5px;
    flex: none;
    display: flex;
    flex-direction: column;
    gap: var(--gap);
    padding: 6px max(3px, env(safe-area-inset-right)) calc(4px + env(safe-area-inset-bottom)) max(3px, env(safe-area-inset-left));
    background: var(--sidebar-bg);
    border-top: 1px solid var(--sidebar-border);
    touch-action: none;
    user-select: none;
    -webkit-user-select: none;
    -webkit-touch-callout: none;
  }
  .row {
    display: flex;
    gap: var(--gap);
  }
  .key {
    position: relative;
    flex: 1 1 0;
    min-width: 0;
    height: var(--key-height);
    padding: 0;
    border: none;
    border-radius: 6px;
    background: var(--sidebar-bg-active);
    box-shadow: 0 1px 0 #00000066;
    color: var(--text-primary);
    font: inherit;
    font-size: 21px;
    line-height: 1;
    touch-action: none;
  }
  .key.word {
    font-size: 14px;
  }
  .key.quiet {
    background: var(--sidebar-bg-raised);
    color: var(--text-secondary);
  }
  .strip .key {
    height: var(--strip-height);
    font-family: var(--font-mono);
    font-size: 14px;
  }
  .shell .key {
    font-size: 17px;
  }
  .key.down {
    background: var(--text-tertiary);
    color: var(--text-primary);
  }
  .key.armed {
    background: var(--accent-dim);
    color: var(--accent-strong);
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .key.locked {
    background: var(--accent);
    color: var(--text-on-accent);
  }
  .pop {
    position: absolute;
    left: 50%;
    bottom: calc(100% + 4px);
    z-index: 1;
    min-width: 44px;
    padding: 8px 6px;
    transform: translateX(-50%);
    border-radius: 9px;
    background: var(--text-tertiary);
    box-shadow: 0 2px 8px #00000080;
    color: var(--text-primary);
    font-size: 30px;
    text-align: center;
    pointer-events: none;
  }
  /* A phone on its side: every row counts. */
  @media (max-height: 480px) {
    .keyboard {
      --key-height: 30px;
      --strip-height: 28px;
      --gap: 4px;
      padding-top: 4px;
    }
    .shell {
      display: none;
    }
    .key {
      font-size: 17px;
    }
  }
</style>
