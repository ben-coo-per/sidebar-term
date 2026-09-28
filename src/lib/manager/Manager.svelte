<!-- Manager: the full window, for running several agents at once (docs/architecture.md "Manager").
     Under the window bar (src/lib/window/WindowBar.svelte: Tabs / Manager, the zoom, Usage, the
     Tray), top to bottom: every agent Tab as a lane across the window of time, then two columns:
     the narrow one what needs the user (questions answered in place, the answers just sent,
     finished work not looked at) with Usage held at its bottom (the Panel's view, every limit's
     bar: the window bar shows no summary in Manager), the wide one the selected Tab's Terminal
     (SelectedTab.svelte), to type into as in Tabs mode. A click on a lane or a finished row selects its Tab, as does
     "Answer in its Terminal" on the card of an agent whose question Manager cannot answer;
     "Open Tab", on a lane, a card, a finished row or the Terminal's bar, opens it in Tabs mode.
     The edges between the three drag: the one under the lanes sizes them, the one beside Needs
     you sizes that column, and the Terminal takes the rest. A double click on an edge gives the
     size back to Manager (the lanes as tall as they are, Needs you a share of the window).

     Keys (none with a modifier; the Hotkeys keep those), while the Terminal does not have the
     focus: 1-9 answer the focused card, Tab / Shift-Tab move the card focus and the selection
     with it, ↑ / ↓ select a lane, ↵ opens the selected Tab, else the focused card's, Esc drops
     the selection. A click selects and gives the Terminal the focus: the keys are then the
     Session's, Esc too, until a click outside it. -->
<script lang="ts">
  import { tick } from "svelte";
  import {
    layout,
    managerTab,
    setManagerLanesHeight,
    setManagerNeedsWidth,
    showInManager,
    tabSessionKey,
    MAX_MANAGER_LANES_HEIGHT,
    MAX_MANAGER_NEEDS_WIDTH,
    MIN_MANAGER_LANES_HEIGHT,
    MIN_MANAGER_NEEDS_WIDTH,
    type Tab,
  } from "../layout.svelte";
  import { settingsPage } from "../settings/visibility.svelte";
  import { terminals } from "../terminal/manager";
  import UsageView from "../panel/usage/UsageView.svelte";
  import { watch as watchUsage } from "../panel/usage/usage.svelte";
  import { usageSettings } from "../panel/usage/settings.svelte";
  import CheckIcon from "../sidebar/icons/CheckIcon.svelte";
  import LaneRow from "./LaneRow.svelte";
  import SelectedTab from "./SelectedTab.svelte";
  import SentRow from "./SentRow.svelte";
  import QuestionCard from "./QuestionCard.svelte";
  import {
    answer,
    finished,
    focusedCard,
    lanes,
    manager,
    moveCardFocus,
    moveSelection,
    openTab,
    rememberOrder,
    runClock,
    select,
    waiting,
    type Lane,
  } from "./state.svelte";
  import { formatDuration, laneStart, ticks, windowSpan } from "./model";

  const shown = $derived(lanes());
  const span = $derived(
    windowSpan(
      layout.managerZoom,
      shown.map((l) => laneStart(l.history) ?? manager.now),
      manager.now,
    ),
  );
  const tickLabels = $derived(ticks(manager.now, span));
  const cards = $derived(waiting());
  const focused = $derived(focusedCard(cards));
  const done = $derived(finished());
  const selected = $derived(managerTab());
  // The selected Tab's lane; none once its agent left (its Terminal stays).
  const selectedLane = $derived(selected ? (shown.find((l) => l.tab.id === selected.id) ?? null) : null);

  // A selected Tab that closed is no longer selected.
  $effect(() => {
    if (layout.managerTabId && !selected) showInManager(null);
  });

  $effect(() => rememberOrder(shown.map((l) => l.tab.id)));
  $effect(() => runClock());
  $effect(() => {
    if (usageSettings.ready) return watchUsage([...usageSettings.agents]);
  });

  /** Until dragged, the lanes are as tall as they are, up to this share of Manager's height. */
  const AUTO_LANES_SHARE = 0.55;
  /** Dragged, they never take more than this share, so the Terminal keeps room. */
  const MAX_LANES_SHARE = 0.75;
  /** Nor Needs you more than this share of its width. */
  const MAX_NEEDS_SHARE = 0.6;

  let root: HTMLDivElement | undefined = $state();
  let lanesSection: HTMLElement | undefined = $state();
  let lanesEl: HTMLDivElement | undefined = $state();
  let columnEl: HTMLDivElement | undefined = $state();
  let needsEl: HTMLDivElement | undefined = $state();

  /** The keys are Manager's: taken from whatever had the focus, a Terminal just mounted too. */
  async function takeKeys() {
    await tick();
    (document.activeElement as HTMLElement | null)?.blur?.();
    root?.focus({ preventScroll: true });
  }

  // Keys come here, not to a Terminal, until the user clicks into one; the lanes and Needs you
  // scroll where they were.
  $effect(() => {
    void takeKeys();
    if (lanesEl) lanesEl.scrollTop = manager.scroll.lanes;
    if (needsEl) needsEl.scrollTop = manager.scroll.needs;
    return () => {
      manager.scroll = { lanes: lanesEl?.scrollTop ?? 0, needs: needsEl?.scrollTop ?? 0 };
    };
  });

  function typing(target: EventTarget | null): boolean {
    const el = target as HTMLElement | null;
    return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey || settingsPage.open || typing(e.target)) return;
    if (/^[1-9]$/.test(e.key)) {
      if (focused?.pending) {
        e.preventDefault();
        void answer(focused, Number(e.key) - 1);
      }
    } else if (e.key === "Tab") {
      e.preventDefault();
      moveCardFocus(cards, e.shiftKey ? -1 : 1);
      void takeKeys();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      moveSelection(shown, e.key === "ArrowDown" ? 1 : -1);
      void takeKeys();
    } else if (e.key === "Enter") {
      const tab = selected ?? focused?.tab;
      if (tab) {
        e.preventDefault();
        openTab(tab);
      }
    } else if (e.key === "Escape" && selected) {
      select(null);
    }
  }

  /** Select with the pointer: the Tab's Terminal takes the keys, as on going to a Tab. */
  async function pick(tab: Tab) {
    select(tab.id);
    await tick();
    const key = tabSessionKey(tab);
    if (key) terminals.focus(key);
  }

  function open(lane: Lane) {
    openTab(lane.tab);
  }

  type Edge = "lanes" | "needs";
  /** Two presses of one edge within this, the first not a drag, are a double click. */
  const DOUBLE_CLICK_MS = 400;

  /** The edge being dragged, if any. */
  let resizing = $state<Edge | null>(null);
  /** The edge last pressed and left where it was, and when. */
  let pressed: { edge: Edge; at: number } | null = null;

  function resize(edge: Edge, size: number | null) {
    if (edge === "lanes") setManagerLanesHeight(size);
    else setManagerNeedsWidth(size);
  }

  /**
   * Drag an edge: the lanes' bottom one sizes them, Needs you's right one sizes the column. A
   * double click gives the size back to Manager. It is told from the presses, not `dblclick`:
   * the shield takes the pointer while an edge is held, so the edge never gets the clicks.
   */
  function startResize(e: PointerEvent, edge: Edge) {
    const el = edge === "lanes" ? lanesSection : columnEl;
    if (e.button !== 0 || !el || !root) return;
    e.preventDefault();
    if (pressed?.edge === edge && e.timeStamp - pressed.at < DOUBLE_CLICK_MS) {
      pressed = null;
      resize(edge, null);
      return;
    }
    pressed = { edge, at: e.timeStamp };
    resizing = edge;
    const at = (ev: PointerEvent) => (edge === "lanes" ? ev.clientY : ev.clientX);
    const start = at(e);
    const box = el.getBoundingClientRect();
    const from = edge === "lanes" ? box.height : box.width;
    const max = edge === "lanes" ? root.clientHeight * MAX_LANES_SHARE : root.clientWidth * MAX_NEEDS_SHARE;

    function onMove(ev: PointerEvent) {
      if (at(ev) === start && pressed) return;
      pressed = null;
      resize(edge, Math.min(max, from + (at(ev) - start)));
    }
    function onUp() {
      resizing = null;
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
    }
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  }
</script>

<svelte:window {onkeydown} />

<div class="manager" bind:this={root} tabindex="-1" role="application" aria-label="Manager">
  <section
    class="lanes"
    style:height={layout.managerLanesHeight === null ? null : `${layout.managerLanesHeight}px`}
    style:max-height="{(layout.managerLanesHeight === null ? AUTO_LANES_SHARE : MAX_LANES_SHARE) * 100}%"
    bind:this={lanesSection}
    aria-label="Agents"
    onpointerenter={() => (manager.hovering = true)}
    onpointerleave={() => (manager.hovering = false)}
  >
    <div class="grid header">
      <span class="heading">Agents<span class="count">({shown.length})</span></span>
      <div class="ticks">
        {#each tickLabels as t (t.percent)}<span class="tick" style:left="{t.percent}%">{t.label}</span>{/each}
        <span class="tick now">now</span>
      </div>
      <span></span>
    </div>
    <div class="rows" bind:this={lanesEl}>
      {#each shown as lane (lane.tab.id)}
        <LaneRow {lane} now={manager.now} {span} selected={selectedLane === lane} onselect={(l) => void pick(l.tab)} onopen={open} />
      {:else}
        <p class="empty">No agents running. Start Claude Code, Codex or Gemini in a Tab and it shows here.</p>
      {/each}
    </div>
    <div class="legend">
      <span><span class="swatch running"></span>Working</span>
      <span><span class="swatch needs-input"></span>Waiting on you</span>
      <span><span class="swatch done"></span>Idle at prompt</span>
    </div>
  </section>

  <div
    class="split"
    style:grid-template-columns={layout.managerNeedsWidth === null
      ? null
      : `min(${layout.managerNeedsWidth}px, ${MAX_NEEDS_SHARE * 100}%) minmax(0, 1fr)`}
  >
    <div
      class="resize-handle rows"
      class:active={resizing === "lanes"}
      role="separator"
      aria-orientation="horizontal"
      aria-label="Agents height"
      aria-valuemin={MIN_MANAGER_LANES_HEIGHT}
      aria-valuemax={MAX_MANAGER_LANES_HEIGHT}
      aria-valuenow={layout.managerLanesHeight ?? undefined}
      title="Drag to resize; double-click to fit the lanes"
      tabindex="-1"
      onpointerdown={(e) => startResize(e, "lanes")}
    ></div>
    <div class="column" bind:this={columnEl}>
      <div class="needs" bind:this={needsEl}>
        <span class="heading" class:waiting={cards.length > 0}>Needs you<span class="count">({cards.length})</span></span>

        {#each manager.sent as s (s.pendingId)}
          <SentRow sent={s} />
        {/each}

        {#if cards.length === 0}
          <span class="nothing">Nothing is waiting on you.</span>
        {/if}

        {#each cards as lane (lane.tab.id)}
          <QuestionCard
            {lane}
            now={manager.now}
            focused={focused === lane}
            onanswer={(i) => void answer(lane, i)}
            onshow={() => void pick(lane.tab)}
            onopen={() => openTab(lane.tab)}
          />
        {/each}

        {#if done.length > 0}
          <span class="heading finished-heading">Finished, not looked at<span class="count">({done.length})</span></span>
          {#each done as f (f.tab.id)}
            <div
              class="finished"
              role="button"
              tabindex="-1"
              onclick={() => void pick(f.tab)}
              onkeydown={(e) => {
                if (e.key !== "Enter") return;
                e.preventDefault();
                openTab(f.tab);
              }}
            >
              <span class="finished-icon"><CheckIcon size={14} /></span>
              <span class="finished-title" title={f.title}>{f.project}</span>
              {#if f.description}<span class="finished-description">{f.description}</span>{/if}
              <span class="finished-summary">{f.summary}</span>
              <span class="finished-since">{formatDuration(manager.now - f.since)} ago</span>
              <button
                type="button"
                class="link"
                tabindex="-1"
                onclick={(e) => {
                  e.stopPropagation();
                  openTab(f.tab);
                }}
              >
                Open Tab
              </button>
            </div>
          {/each}
        {/if}
      </div>

      <section class="usage" aria-label="Usage">
        <span class="heading">Usage</span>
        <UsageView />
      </section>
    </div>

    <div class="viewer">
      <div
        class="resize-handle columns"
        class:active={resizing === "needs"}
        role="separator"
        aria-orientation="vertical"
        aria-label="Needs you width"
        aria-valuemin={MIN_MANAGER_NEEDS_WIDTH}
        aria-valuemax={MAX_MANAGER_NEEDS_WIDTH}
        aria-valuenow={layout.managerNeedsWidth ?? undefined}
        title="Drag to resize; double-click for the usual width"
        tabindex="-1"
        onpointerdown={(e) => startResize(e, "needs")}
      ></div>
      {#if selected}
        <SelectedTab tab={selected} lane={selectedLane} now={manager.now} onopen={() => openTab(selected)} onclose={() => select(null)} />
      {:else}
        <span class="nothing">Select an agent to see its Terminal here, and type into it.</span>
      {/if}
    </div>
  </div>

  {#if resizing}
    <!-- Over everything while an edge drags: the pointer keeps the resize cursor, and the
         Terminal under it gets none of the movement. -->
    <div class="drag-shield" class:rows={resizing === "lanes"}></div>
  {/if}
</div>

<style>
  .manager {
    position: relative;
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    background: var(--term-bg);
    color: var(--text-primary);
    font-family:
      -apple-system,
      BlinkMacSystemFont,
      "SF Pro Text",
      sans-serif;
    outline: none;
  }
  .lanes {
    flex: none;
    display: flex;
    flex-direction: column;
    padding: 10px 16px 8px;
    min-height: 0;
  }
  .grid {
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr) 110px;
    column-gap: 12px;
  }
  .header {
    align-items: end;
    height: 22px;
  }
  .heading {
    display: flex;
    align-items: baseline;
    gap: 4px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    color: var(--text-tertiary);
  }
  .heading.waiting {
    color: var(--status-needs-input);
  }
  .count {
    font-weight: 400;
    letter-spacing: 0;
    text-transform: none;
    font-variant-numeric: tabular-nums;
    color: var(--text-tertiary);
  }
  .ticks {
    position: relative;
    height: 16px;
    font-size: 10.5px;
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
  }
  .tick {
    position: absolute;
    top: 0;
  }
  .tick.now {
    right: 0;
    color: var(--text-secondary);
  }
  .rows {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    margin: 0 -16px;
    padding: 0 16px;
  }
  .rows::-webkit-scrollbar,
  .needs::-webkit-scrollbar {
    width: 8px;
  }
  .rows::-webkit-scrollbar-thumb,
  .needs::-webkit-scrollbar-thumb {
    background: var(--scrollbar-thumb);
    border-radius: 4px;
  }
  .empty {
    margin: 10px 0;
    font-size: 12.5px;
    color: var(--text-tertiary);
  }
  .legend {
    flex: none;
    display: flex;
    gap: 16px;
    padding: 8px 0 0 232px;
    font-size: 11px;
    color: var(--text-tertiary);
  }
  .legend > span {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .swatch {
    width: 10px;
    height: 8px;
  }
  .swatch.running {
    background: var(--status-running);
  }
  .swatch.needs-input {
    background: var(--status-needs-input);
  }
  .swatch.done {
    background: var(--activity-other);
  }
  .split {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(300px, 34%) minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    border-top: 1px solid var(--sidebar-border);
  }
  .column {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .needs {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 16px;
  }
  /* As tall as its bars, at the bottom of the column; Needs you scrolls above it. The view
     brings 12 px of its own at the sides. */
  .usage {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 45%;
    padding: 10px 4px 4px;
    border-top: 1px solid var(--sidebar-divider);
  }
  .usage > .heading {
    padding: 0 12px;
  }
  /* Square, as the lanes' bars. */
  .usage :global(.track),
  .usage :global(.fill) {
    border-radius: 0;
  }
  .viewer {
    position: relative;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--sidebar-divider);
  }
  .resize-handle {
    position: absolute;
    z-index: 10;
  }
  .resize-handle.rows {
    top: -3px;
    left: 0;
    width: 100%;
    height: 6px;
    cursor: row-resize;
  }
  .resize-handle.columns {
    top: 0;
    left: -3px;
    width: 6px;
    height: 100%;
    cursor: col-resize;
  }
  .resize-handle:hover,
  .resize-handle.active {
    background: var(--accent-dim);
  }
  .drag-shield {
    position: fixed;
    inset: 0;
    z-index: 20;
    cursor: col-resize;
  }
  .drag-shield.rows {
    cursor: row-resize;
  }
  .viewer > .nothing {
    margin: auto;
    padding: 16px;
  }
  .nothing {
    padding: 2px 0 6px;
    font-size: 12.5px;
    color: var(--text-tertiary);
  }
  .finished-heading {
    margin-top: 6px;
  }
  .finished {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    padding: 4px 8px;
    margin: -4px -8px 0;
    border-radius: var(--radius-sm);
    cursor: default;
  }
  .finished:hover {
    background: var(--sidebar-bg-raised);
  }
  .finished-icon {
    flex: none;
    display: flex;
    color: var(--status-finished);
  }
  .finished-title {
    flex: none;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--text-primary);
  }
  .finished-description {
    flex: 0 2 auto;
    min-width: 0;
    font-size: 12.5px;
    color: var(--text-tertiary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .finished-summary {
    flex: 1 1 auto;
    min-width: 0;
    font-size: 11.5px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .finished-since {
    flex: none;
    font-size: 10.5px;
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
  }
  .link {
    appearance: none;
    flex: none;
    padding: 0;
    border: none;
    background: none;
    font: inherit;
    font-size: 11.5px;
    color: var(--accent-strong);
    cursor: default;
  }
  .link:hover {
    text-decoration: underline;
  }
</style>
