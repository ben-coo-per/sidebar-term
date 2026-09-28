<!-- Manager: the full window, for running several agents at once (docs/architecture.md "Manager").
     Top to bottom: the top bar (Tabs / Manager, the zoom, the Usage summary, the Tray; the window's
     drag region), every agent Tab as a lane across the window of time, then two columns: what
     needs the user (questions answered in place, the answers just sent, finished work not looked
     at) and every agent's events. A lane or card opens its Tab in Tabs mode.

     Keys (none with a modifier; the Hotkeys keep those): 1-9 answer the focused card, Tab /
     Shift-Tab move the card focus, ↑ / ↓ move a lane focus, ↵ opens the focused lane's Tab, else
     the focused card's, Esc drops the lane focus. -->
<script lang="ts">
  import { layout, setManagerZoom } from "../layout.svelte";
  import { MANAGER_ZOOMS, type ManagerZoom } from "../sidebar/settings";
  import { settingsPage } from "../settings/visibility.svelte";
  import Tray from "../tray/Tray.svelte";
  import UsageSummary from "../panel/usage/UsageSummary.svelte";
  import { watch as watchUsage } from "../panel/usage/usage.svelte";
  import { usageSettings } from "../panel/usage/settings.svelte";
  import CheckIcon from "../sidebar/icons/CheckIcon.svelte";
  import SpinnerIcon from "../sidebar/icons/SpinnerIcon.svelte";
  import ModeSwitch from "./ModeSwitch.svelte";
  import Segmented from "./Segmented.svelte";
  import LaneRow from "./LaneRow.svelte";
  import QuestionCard from "./QuestionCard.svelte";
  import AgentFeed from "./AgentFeed.svelte";
  import {
    answer,
    finished,
    focusedCard,
    lanes,
    manager,
    moveCardFocus,
    moveLaneFocus,
    openTab,
    rememberOrder,
    runClock,
    waiting,
    type Lane,
  } from "./manager.svelte";
  import { formatDuration, laneStart, ticks, windowSpan, ZOOM_LABELS } from "./model";

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
  const zoomItems = MANAGER_ZOOMS.map((id) => ({ id, label: ZOOM_LABELS[id] }));

  $effect(() => rememberOrder(shown.map((l) => l.tab.id)));
  $effect(() => runClock());
  $effect(() => {
    if (usageSettings.ready) return watchUsage([...usageSettings.agents]);
  });

  let root: HTMLDivElement | undefined = $state();
  let lanesEl: HTMLDivElement | undefined = $state();
  let needsEl: HTMLDivElement | undefined = $state();
  let feedEl: HTMLDivElement | undefined = $state();

  // Keys come here, not to the Terminal that had the focus; the columns scroll where they were.
  $effect(() => {
    (document.activeElement as HTMLElement | null)?.blur?.();
    root?.focus({ preventScroll: true });
    if (lanesEl) lanesEl.scrollTop = manager.scroll.lanes;
    if (needsEl) needsEl.scrollTop = manager.scroll.needs;
    if (feedEl) feedEl.scrollTop = manager.scroll.feed;
    return () => {
      manager.scroll = { lanes: lanesEl?.scrollTop ?? 0, needs: needsEl?.scrollTop ?? 0, feed: feedEl?.scrollTop ?? 0 };
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
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      moveLaneFocus(shown, e.key === "ArrowDown" ? 1 : -1);
    } else if (e.key === "Enter") {
      const lane = shown.find((l) => l.tab.id === manager.laneFocus) ?? focused;
      if (lane) {
        e.preventDefault();
        openTab(lane.tab);
      }
    } else if (e.key === "Escape" && manager.laneFocus) {
      manager.laneFocus = null;
    }
  }

  function open(lane: Lane) {
    openTab(lane.tab);
  }
</script>

<svelte:window {onkeydown} />

<div class="manager" bind:this={root} tabindex="-1" role="application" aria-label="Manager">
  <div class="topbar" data-tauri-drag-region>
    <ModeSwitch />
    <Segmented
      label="Zoom"
      compact
      items={zoomItems}
      selected={layout.managerZoom}
      onselect={(z: ManagerZoom) => setManagerZoom(z)}
    />
    <span class="spacer" data-tauri-drag-region></span>
    <UsageSummary />
    <Tray />
  </div>

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
        <LaneRow {lane} now={manager.now} {span} focused={manager.laneFocus === lane.tab.id} onopen={open} />
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
        <div class="sent" class:fading={s.fading}>
          <span class="sent-icon"><CheckIcon size={14} /></span>
          <span class="sent-text">Sent <span class="choice">“{s.choice}”</span> to {s.title}</span>
          {#if !s.resumed}<span class="resuming"><SpinnerIcon size={14} />resuming</span>{/if}
        </div>
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
          onopen={() => openTab(lane.tab)}
        />
      {/each}

      {#if done.length > 0}
        <span class="heading finished-heading">Finished, not looked at<span class="count">({done.length})</span></span>
        {#each done as f (f.tab.id)}
          <div class="finished" role="button" tabindex="-1" onclick={() => openTab(f.tab)} onkeydown={(e) => e.key === "Enter" && openTab(f.tab)}>
            <span class="finished-icon"><CheckIcon size={14} /></span>
            <span class="finished-title">{f.title}</span>
            <span class="finished-summary">{f.summary}</span>
            <span class="finished-since">{formatDuration(manager.now - f.since)} ago</span>
            <span class="link">Open Tab</span>
          </div>
        {/each}
      {/if}
    </div>

    <div class="column feed-column" bind:this={feedEl}>
      <span class="heading">Activity<span class="sub">· every agent, newest first</span></span>
      <AgentFeed />
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
  .topbar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    height: var(--titlebar-inset);
    padding: 0 6px 0 var(--traffic-lights-width);
    background: var(--sidebar-bg);
    border-bottom: 1px solid var(--sidebar-border);
    -webkit-app-region: drag;
  }
  .spacer {
    flex: 1 1 auto;
    align-self: stretch;
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
  .count,
  .sub {
    font-weight: 400;
    letter-spacing: 0;
    text-transform: none;
    font-variant-numeric: tabular-nums;
    color: var(--text-tertiary);
  }
  .sub {
    margin-left: 2px;
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
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
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
  .feed-column {
    gap: 0;
    border-left: 1px solid var(--sidebar-divider);
  }
  .feed-column > .heading {
    margin-bottom: 8px;
  }
  .nothing {
    padding: 2px 0 6px;
    font-size: 12.5px;
    color: var(--text-tertiary);
  }
  .sent {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 12px;
    border: 1px dashed var(--scrollbar-thumb);
    border-radius: var(--radius-md);
    font-size: 12px;
    color: var(--text-secondary);
    transition: opacity var(--duration-medium) var(--ease-standard);
  }
  .sent.fading {
    opacity: 0;
  }
  .sent-icon {
    display: flex;
  }
  .sent-text {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .choice {
    color: var(--text-primary);
  }
  .resuming {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--status-running);
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
    flex: none;
    font-size: 11.5px;
    color: var(--accent-strong);
  }
</style>
