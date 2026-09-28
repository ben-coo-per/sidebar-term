<!-- One agent in the phone's Manager, working or idle: its status icon, its project and a few
     words on what it is at, since when; under them its agent, its Host's chip and what it last
     did (or, idle, what its last turn changed); then its status over the last hour. Tap it to
     open its Terminal. -->
<script lang="ts">
  import { AGENT_NAMES } from "../agentStatus";
  import { formatDuration, segments } from "../manager/model";
  import CheckIcon from "../sidebar/icons/CheckIcon.svelte";
  import SpinnerIcon from "../sidebar/icons/SpinnerIcon.svelte";
  import type { Lane } from "./lanes";

  /** The stretch of time the lane shows. */
  const SPAN_MS = 60 * 60_000;

  let { lane, now, onopen }: { lane: Lane; now: number; onopen: () => void } = $props();

  const segs = $derived(segments(lane.history, now, SPAN_MS));
  const age = $derived(formatDuration(now - lane.since));
  const status = $derived(lane.kind === "working" ? `working ${age}` : `idle ${age}`);
  const note = $derived(lane.kind === "working" ? lane.doing : lane.summary);
</script>

<button type="button" class="lane" class:away={!lane.row.reachable} disabled={lane.row.sessionId === null || !lane.row.reachable} onclick={onopen}>
  <span class="top">
    <span class="icon {lane.kind}">
      {#if lane.kind === "working"}
        <SpinnerIcon size={16} />
      {:else}
        <CheckIcon size={16} />
      {/if}
    </span>
    <span class="name">
      <span class="project">{lane.project}</span>
      {#if lane.description}<span class="description">{lane.description}</span>{/if}
    </span>
    <span class="status {lane.kind}">{status}</span>
  </span>
  <span class="meta">
    <span class="agent">{AGENT_NAMES[lane.agent]}</span>
    {#if lane.row.hostName}<span class="chip">{lane.row.hostName}</span>{/if}
    {#if !lane.hooked}<span class="chip dashed">screen only</span>{/if}
    {#if note}<span class="note">{note}</span>{/if}
  </span>
  <span class="track" aria-hidden="true">
    {#each segs as s, i (i)}
      <span class="seg {s.status}" style:left="{s.left}%" style:width="{s.width}%"></span>
    {/each}
  </span>
</button>

<style>
  .lane {
    appearance: none;
    display: flex;
    flex-direction: column;
    gap: 5px;
    width: 100%;
    padding: 10px 20px;
    border: none;
    border-bottom: 1px solid var(--sidebar-divider);
    background: transparent;
    color: var(--text-primary);
    font: inherit;
    text-align: left;
  }
  .lane:active {
    background: var(--sidebar-bg-raised);
  }
  .lane:disabled {
    opacity: 0.4;
  }
  .top,
  .meta {
    display: flex;
    align-items: center;
    gap: 8px;
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
  .name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 16px;
  }
  .project {
    font-weight: 600;
  }
  .description {
    margin-left: 6px;
    color: var(--text-tertiary);
  }
  .status {
    flex: none;
    font-size: 12px;
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
  }
  .status.working {
    color: var(--status-running);
  }
  .meta {
    gap: 6px;
    padding-left: 24px;
    font-size: 12px;
    color: var(--text-tertiary);
  }
  .agent {
    flex: none;
  }
  .chip {
    flex: none;
    padding: 0 6px;
    border: 1px solid var(--sidebar-border);
    border-radius: 4px;
    font-size: 11px;
    line-height: 16px;
    color: var(--text-secondary);
    white-space: nowrap;
  }
  .chip.dashed {
    border-style: dashed;
    color: var(--text-tertiary);
  }
  .note {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 11.5px;
  }
  .track {
    position: relative;
    height: 4px;
    margin-left: 24px;
    border-radius: 2px;
    background: var(--meter-track);
    overflow: hidden;
  }
  .seg {
    position: absolute;
    top: 0;
    bottom: 0;
    background: var(--status-finished);
    opacity: 0.45;
  }
  .seg.running {
    background: var(--status-running);
    opacity: 1;
  }
  .seg.needs-input {
    background: var(--status-needs-input);
    opacity: 1;
  }
</style>
