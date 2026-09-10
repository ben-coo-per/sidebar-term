<!-- The Activity view: CPU and memory meters split between Tabs and everything else, and the
     busiest processes. A Session's processes show in its Tab's colour (click one to go to that
     Tab); every other process is muted grey. See docs/architecture.md "Panel". -->
<script lang="ts">
  import { activity } from "./activity.svelte";
  import { formatBytes, formatCpu, machineCpu, meterSegments, sortProcesses, type Segment, type SortKey } from "./model";
  import { activateTab, layout, tabIdForSession, type Tab } from "../../layout.svelte";
  import { sessionState, tabTitle } from "../../sessions.svelte";
  import { tabColorVar } from "../../sidebar/repoColor";
  import type { ActivityProcess, ActivitySnapshot, SessionId } from "../../types";

  let sortKey = $state<SortKey>("cpu");

  const snapshot = $derived(activity.snapshot);
  const rows = $derived(snapshot ? sortProcesses(snapshot.processes, sortKey) : []);
  /** Sessions in sidebar order, so meter segments line up with the Tabs above. */
  const order = $derived(
    layout.groups
      .flatMap((g) => g.tabIds)
      .map((id) => layout.tabs[id]?.sessionId ?? null)
      .filter((id): id is SessionId => id !== null),
  );

  function tabOf(sessionId: SessionId): Tab | null {
    const tabId = tabIdForSession(sessionId);
    return tabId ? (layout.tabs[tabId] ?? null) : null;
  }

  function colorOf(sessionId: SessionId): string {
    return tabColorVar(sessionState(sessionId)?.info?.git?.commonDir ?? null);
  }

  function tabLabel(sessionId: SessionId): string {
    const tab = tabOf(sessionId);
    return tab ? `“${tabTitle(tab)}”` : "a closed Tab";
  }

  function segmentTitle(snap: ActivitySnapshot, seg: Segment, measure: SortKey): string {
    const format = (v: number) => (measure === "cpu" ? `${formatCpu(v)}%` : formatBytes(v));
    if (seg.sessionId === null) {
      const inTabs = snap.sessions.reduce((sum, s) => sum + s[measure], 0);
      const all = measure === "cpu" ? snap.cpuTotal : snap.memUsed;
      return `Everything else: ${format(Math.max(0, all - inTabs))}`;
    }
    const s = snap.sessions.find((x) => x.sessionId === seg.sessionId);
    const count = s ? ` in ${s.processes} process${s.processes === 1 ? "" : "es"}` : "";
    return `${tabLabel(seg.sessionId)}: ${format(s?.[measure] ?? 0)}${count}`;
  }

  function rowTitle(p: ActivityProcess): string {
    const base = `${p.name} (pid ${p.pid})`;
    return p.sessionId === null ? base : `${base} in ${tabLabel(p.sessionId)}. Click to go to it.`;
  }

  function goToTab(p: ActivityProcess) {
    if (p.sessionId === null) return;
    const tabId = tabIdForSession(p.sessionId);
    if (tabId) activateTab(tabId);
  }

  function onRowKeydown(e: KeyboardEvent, p: ActivityProcess) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      goToTab(p);
    }
  }
</script>

{#snippet meter(snap: ActivitySnapshot, measure: SortKey, label: string, value: string, title: string)}
  <div class="meter" {title}>
    <span class="meter-label">{label}</span>
    <span class="track">
      {#each meterSegments(snap, measure, order) as seg (seg.sessionId ?? "other")}
        <span
          class="segment"
          style:width="{seg.fraction * 100}%"
          style:background={seg.sessionId === null ? "var(--activity-other)" : colorOf(seg.sessionId)}
          title={segmentTitle(snap, seg, measure)}
        ></span>
      {/each}
    </span>
    <span class="meter-value">{value}</span>
  </div>
{/snippet}

{#snippet sortHeader(key: SortKey, label: string)}
  <button
    type="button"
    class="col num sort"
    class:sorted={sortKey === key}
    aria-pressed={sortKey === key}
    tabindex="-1"
    onclick={() => (sortKey = key)}
    title="Sort by {label}"
  >
    {label}{#if sortKey === key}<span class="caret" aria-hidden="true">▾</span>{/if}
  </button>
{/snippet}

<div class="activity">
  {#if !snapshot}
    <p class="waiting">Sampling…</p>
  {:else}
    <div class="meters">
      {@render meter(
        snapshot,
        "cpu",
        "CPU",
        `${Math.round(machineCpu(snapshot))}%`,
        `CPU load across ${snapshot.cpuCount} cores`,
      )}
      {@render meter(
        snapshot,
        "mem",
        "Mem",
        formatBytes(snapshot.memUsed),
        `Memory used, of ${formatBytes(snapshot.memTotal)}`,
      )}
    </div>

    <div class="columns">
      <span class="col name">Process</span>
      {@render sortHeader("cpu", "CPU")}
      {@render sortHeader("mem", "Mem")}
    </div>

    <div class="list" role="list">
      {#each rows as p (p.pid)}
        {#if p.sessionId !== null}
          <div
            class="row mine"
            role="button"
            tabindex="-1"
            style:--row-color={colorOf(p.sessionId)}
            title={rowTitle(p)}
            onclick={() => goToTab(p)}
            onkeydown={(e) => onRowKeydown(e, p)}
          >
            <span class="col name"><span class="dot"></span><span class="label">{p.name}</span></span>
            <span class="col num">{formatCpu(p.cpu)}</span>
            <span class="col num">{formatBytes(p.mem)}</span>
          </div>
        {:else}
          <div class="row" role="listitem" title={rowTitle(p)}>
            <span class="col name"><span class="dot"></span><span class="label">{p.name}</span></span>
            <span class="col num">{formatCpu(p.cpu)}</span>
            <span class="col num">{formatBytes(p.mem)}</span>
          </div>
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .activity {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .waiting {
    margin: 8px 12px;
    color: var(--text-tertiary);
  }
  .meters {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 2px 12px 8px;
  }
  .meter {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .meter-label {
    flex: none;
    width: 26px;
    font-size: 10px;
    color: var(--text-tertiary);
  }
  .track {
    flex: 1 1 auto;
    display: flex;
    gap: 1px;
    height: 6px;
    border-radius: 3px;
    overflow: hidden;
    background: var(--meter-track);
  }
  .segment {
    flex: none;
    height: 100%;
    transition: width var(--duration-medium) var(--ease-standard);
  }
  .meter-value {
    flex: none;
    min-width: 50px;
    text-align: right;
    color: var(--text-secondary);
  }
  .columns,
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 40px 54px;
    column-gap: 6px;
    align-items: center;
    padding: 0 12px;
  }
  .columns {
    flex: none;
    height: 20px;
    border-bottom: 1px solid var(--sidebar-divider);
    color: var(--text-tertiary);
    font-size: 10px;
  }
  .sort {
    appearance: none;
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: inherit;
    cursor: default;
  }
  .sort:hover,
  .sort.sorted {
    color: var(--text-secondary);
  }
  .caret {
    margin-left: 1px;
  }
  .list {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 2px 0 6px;
  }
  .list::-webkit-scrollbar {
    width: 8px;
  }
  .list::-webkit-scrollbar-thumb {
    background: var(--scrollbar-thumb);
    border-radius: 4px;
  }
  .row {
    height: 19px;
    color: var(--activity-other-text);
  }
  .row.mine {
    color: var(--row-color);
    cursor: pointer;
  }
  .row.mine:hover {
    background: var(--sidebar-bg-raised);
  }
  .row.mine .num {
    color: var(--text-secondary);
  }
  .col.name {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .num {
    text-align: right;
    white-space: nowrap;
  }
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }
  .row.mine .dot {
    background: var(--row-color);
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
