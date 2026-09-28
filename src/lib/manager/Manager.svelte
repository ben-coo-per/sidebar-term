<!-- Manager: the full window, for running several agents at once (docs/architecture.md "Manager").
     Under the window bar (src/lib/window/WindowBar.svelte: Tabs / Manager, the zoom, Usage, the
     Tray), top to bottom: every agent Tab as a lane across the window of time, then two columns:
     the narrow one what needs the user (questions answered in place, the answers just sent,
     finished work not looked at), the wide one the selected Tab's Terminal (SelectedTab.svelte),
     to type into as in Tabs mode. A click on a lane or a finished row selects its Tab, as does
     "Answer in its Terminal" on the card of an agent whose question Manager cannot answer;
     "Open Tab", on a lane, a card, a finished row or the Terminal's bar, opens it in Tabs mode.

     Keys (none with a modifier; the Hotkeys keep those), while the Terminal does not have the
     focus: 1-9 answer the focused card, Tab / Shift-Tab move the card focus and the selection
     with it, ↑ / ↓ select a lane, ↵ opens the selected Tab, else the focused card's, Esc drops
     the selection. A click selects and gives the Terminal the focus: the keys are then the
     Session's, Esc too, until a click outside it. -->
<script lang="ts">
  import { tick } from "svelte";
  import { layout, managerTab, showInManager, tabSessionKey, type Tab } from "../layout.svelte";
  import { settingsPage } from "../settings/visibility.svelte";
  import { terminals } from "../terminal/manager";
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

  let root: HTMLDivElement | undefined = $state();
  let lanesEl: HTMLDivElement | undefined = $state();
  let needsEl: HTMLDivElement | undefined = $state();

  /** The keys are Manager's: taken from whatever had the focus, a Terminal just mounted too. */
  async function takeKeys() {
    await tick();
    (document.activeElement as HTMLElement | null)?.blur?.();
    root?.focus({ preventScroll: true });
  }

  // Keys come here, not to a Terminal, until the user clicks into one; the columns scroll where
  // they were.
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
</script>

<svelte:window {onkeydown} />

<div class="manager" bind:this={root} tabindex="-1" role="application" aria-label="Manager">
  <section
    class="lanes"
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

  <div class="split">
    <div class="column" bind:this={needsEl}>
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

    <div class="viewer">
      {#if selected}
        <SelectedTab tab={selected} lane={selectedLane} now={manager.now} onopen={() => openTab(selected)} onclose={() => select(null)} />
      {:else}
        <span class="nothing">Select an agent to see its Terminal here, and type into it.</span>
      {/if}
    </div>
  </div>
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
    max-height: 55%;
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
    min-height: 0;
    overflow-y: auto;
    margin: 0 -16px;
    padding: 0 16px;
  }
  .rows::-webkit-scrollbar,
  .column::-webkit-scrollbar {
    width: 8px;
  }
  .rows::-webkit-scrollbar-thumb,
  .column::-webkit-scrollbar-thumb {
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
    border-radius: 2px;
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
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(300px, 34%) minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    border-top: 1px solid var(--sidebar-border);
  }
  .column {
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 16px;
  }
  .viewer {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--sidebar-divider);
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
