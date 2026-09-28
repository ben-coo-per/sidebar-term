<!-- One agent Tab as a lane: its status icon, Title (bold while unread), a Host chip on a paired
     Host, a "screen only" chip when the agent has no hooks, and where it runs; then its status
     history across the window; then what it is doing and for how long. Hover (or the lane focus)
     swaps that for "Open Tab ↵"; a click opens the Tab. -->
<script lang="ts">
  import type { Lane } from "./state.svelte";
  import { formatDuration, segments } from "./model";
  import { AGENT_NAMES } from "../agentStatus";
  import RobotIcon from "../sidebar/icons/RobotIcon.svelte";
  import SpinnerIcon from "../sidebar/icons/SpinnerIcon.svelte";
  import CheckIcon from "../sidebar/icons/CheckIcon.svelte";
  import Place from "./Place.svelte";

  let {
    lane,
    now,
    span,
    focused,
    onopen,
  }: { lane: Lane; now: number; span: number; focused: boolean; onopen: (lane: Lane) => void } = $props();

  const segs = $derived(segments(lane.history, now, span));
  const age = $derived(formatDuration(now - lane.since));
  const status = $derived(lane.kind === "waiting" ? `waiting ${age}` : lane.kind === "working" ? `working ${age}` : `idle ${age} ago`);
</script>

<div
  class="lane"
  class:focused
  role="button"
  tabindex="-1"
  title="{AGENT_NAMES[lane.agent]}: {status}"
  onclick={() => onopen(lane)}
  onkeydown={(e) => e.key === "Enter" && onopen(lane)}
>
  <span class="label">
    <span class="icon {lane.kind}">
      {#if lane.kind === "working"}
        <SpinnerIcon size={14} />
      {:else if lane.kind === "waiting"}
        <RobotIcon size={14} />
      {:else}
        <CheckIcon size={14} />
      {/if}
    </span>
    <span class="text">
      <span class="title" class:unread={lane.unread}>{lane.title}</span>
      <span class="meta">
        {#if lane.host}<span class="chip host">{lane.host}</span>{/if}
        {#if !lane.hooked}<span class="chip screen">screen only</span>{/if}
        <Place git={lane.git} remote={lane.remote} />
      </span>
    </span>
  </span>

  <div class="track">
    {#each segs as s, i (i)}
      <span class="seg {s.status}" style:left="{s.left}%" style:width="{s.width}%"></span>
    {/each}
    <span class="now-line"></span>
  </div>

  <span class="status {lane.kind}">{status}</span>
  <span class="open">Open Tab<kbd class="hotkey">↵</kbd></span>
</div>

<style>
  .lane {
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr) 110px;
    column-gap: 12px;
    align-items: center;
    height: 38px;
    margin: 0 -16px;
    padding: 0 16px;
    border-bottom: 1px solid var(--sidebar-bg-raised);
    cursor: default;
    user-select: none;
    outline: none;
  }
  .lane:hover,
  .lane.focused {
    background: var(--sidebar-bg-raised);
  }
  .lane.focused {
    box-shadow: inset 2px 0 0 var(--focus-ring);
  }
  .label {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
  }
  .icon {
    flex: none;
    display: flex;
    color: var(--text-tertiary);
  }
  .icon.working {
    color: var(--status-running);
  }
  .icon.waiting {
    color: var(--status-needs-input);
  }
  .text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .title {
    font-size: 12.5px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title.unread {
    font-weight: 600;
    color: var(--text-primary);
  }
  .lane:hover .title,
  .lane.focused .title {
    color: var(--text-primary);
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    overflow: hidden;
  }
  .chip {
    flex: none;
    font-size: 10px;
    line-height: 14px;
    padding: 0 5px;
    border-radius: 4px;
    white-space: nowrap;
  }
  .chip.host {
    border: 1px solid var(--sidebar-border);
    background: var(--sidebar-bg-raised);
    color: var(--text-secondary);
  }
  .chip.screen {
    border: 1px dashed var(--scrollbar-thumb);
    color: var(--text-tertiary);
  }
  .track {
    position: relative;
    height: 12px;
    border-radius: 3px;
    background: var(--sidebar-bg);
  }
  .seg {
    position: absolute;
    top: 0;
    bottom: 0;
    min-width: 3px;
    border-radius: 3px;
  }
  .seg.running {
    background: var(--status-running);
  }
  .seg.needs-input {
    background: var(--status-needs-input);
  }
  .seg.done {
    background: var(--activity-other);
  }
  .now-line {
    position: absolute;
    top: -8px;
    bottom: -8px;
    right: 0;
    width: 1px;
    background: var(--activity-other);
  }
  .status {
    text-align: right;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    color: var(--text-tertiary);
  }
  .status.waiting {
    color: var(--status-needs-input);
  }
  .status.working {
    color: var(--text-secondary);
  }
  .open {
    display: none;
    justify-content: flex-end;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    color: var(--accent-strong);
    white-space: nowrap;
  }
  .lane:hover .status,
  .lane.focused .status {
    display: none;
  }
  .lane:hover .open,
  .lane.focused .open {
    display: flex;
  }
  .hotkey {
    font-family: inherit;
    font-size: 10.5px;
    line-height: 16px;
    padding: 0 5px;
    border: 1px solid var(--sidebar-border);
    border-radius: 4px;
    background: var(--sidebar-bg-raised);
    color: var(--text-secondary);
  }
</style>
