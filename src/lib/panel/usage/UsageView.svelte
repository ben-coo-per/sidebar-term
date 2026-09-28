<!-- The Usage view: each chosen coding agent's usage limits (5-hour window, week...) as thin bars,
     with the time left until each resets. Agents are chosen on the Settings page.
     See docs/architecture.md "Panel". -->
<script lang="ts">
  import { usage } from "./usage.svelte";
  import { usageSettings } from "./settings.svelte";
  import {
    currentPercent,
    fillFraction,
    formatPercent,
    formatResetsIn,
    freshnessNote,
    usageLevel,
    waitingNote,
  } from "./model";
  import { AGENT_NAMES } from "../../agentStatus";
  import { openSettings } from "../../settings/visibility.svelte";
  import type { AgentUsage, UsageWindow } from "../../types";

  const snapshot = $derived(usage.snapshot);
  const now = $derived(usage.now);

  function barTitle(a: AgentUsage, w: UsageWindow): string {
    const used = `${AGENT_NAMES[a.agent]} ${w.label}: ${formatPercent(currentPercent(w, now))} used`;
    if (w.resetsAt === null) return used;
    const at = new Date(w.resetsAt).toLocaleString(undefined, { weekday: "short", hour: "numeric", minute: "2-digit" });
    return w.resetsAt <= now ? `${used}; reset ${at}` : `${used}; resets ${at}`;
  }
</script>

<div class="usage">
  {#if usageSettings.agents.length === 0}
    <p class="empty">
      No agents chosen.
      <button type="button" class="link" onclick={() => openSettings("usage")}>Choose in Settings</button>
    </p>
  {:else if !snapshot}
    <p class="empty">Reading…</p>
  {:else}
    <div class="list">
      {#each snapshot.agents as a (a.agent)}
        <div class="agent">
          <span class="name">{AGENT_NAMES[a.agent]}</span>
          <span class="note">{freshnessNote(a, now)}</span>
        </div>
        {#if a.rateLimitedUntil !== null}
          <p class="waiting" title="api.anthropic.com rate-limited the usage read; the numbers shown are the last read">
            {waitingNote(a, now)}
          </p>
        {:else if a.error}
          <p class="error" title={a.error}>{a.error}</p>
        {/if}
        {#each a.windows as w (w.label)}
          {@const percent = currentPercent(w, now)}
          <span class="label">{w.label}</span>
          <span class="track" title={barTitle(a, w)}>
            <span
              class="fill {usageLevel(percent)}"
              class:stale={a.error !== null}
              style:width="{fillFraction(percent) * 100}%"
            ></span>
          </span>
          <span class="percent {usageLevel(percent)}">{formatPercent(percent)}</span>
          <span class="resets" title={barTitle(a, w)}>{formatResetsIn(w, now)}</span>
        {/each}
      {/each}
    </div>
  {/if}
</div>

<style>
  .usage {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    font-size: calc(11px * var(--ui-font-scale));
    font-variant-numeric: tabular-nums;
  }
  .empty {
    margin: 8px 12px;
    color: var(--text-tertiary);
  }
  .link {
    appearance: none;
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--accent);
    cursor: pointer;
  }
  .link:hover {
    color: var(--accent-strong);
  }
  .list {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr) 30px minmax(40px, max-content);
    grid-auto-rows: min-content;
    column-gap: 8px;
    row-gap: 5px;
    align-items: center;
    padding: 0 12px 8px;
  }
  .list::-webkit-scrollbar {
    width: 8px;
  }
  .list::-webkit-scrollbar-thumb {
    background: var(--scrollbar-thumb);
    border-radius: 4px;
  }
  .agent {
    grid-column: 1 / -1;
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    margin-top: 4px;
  }
  .agent:not(:first-child) {
    margin-top: 8px;
  }
  .name {
    color: var(--text-secondary);
    white-space: nowrap;
  }
  .note {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: calc(10px * var(--ui-font-scale));
    color: var(--text-tertiary);
  }
  /* A rate limit is waited out quietly: the same muted line as a real failure, not an alarm. */
  .error,
  .waiting {
    grid-column: 1 / -1;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: calc(10px * var(--ui-font-scale));
    color: var(--text-tertiary);
  }
  .label {
    font-size: calc(10px * var(--ui-font-scale));
    color: var(--text-tertiary);
  }
  .track {
    display: flex;
    height: 6px;
    border-radius: 3px;
    overflow: hidden;
    background: var(--meter-track);
  }
  .fill {
    height: 100%;
    border-radius: 3px;
    background: var(--usage-fill);
    transition: width var(--duration-medium) var(--ease-standard);
  }
  .fill.high {
    background: var(--usage-high);
  }
  .fill.full {
    background: var(--usage-full);
  }
  .fill.stale {
    opacity: 0.45;
  }
  .percent {
    text-align: right;
    color: var(--text-secondary);
  }
  .percent.high {
    color: var(--usage-high);
  }
  .percent.full {
    color: var(--usage-full);
  }
  .resets {
    text-align: right;
    white-space: nowrap;
    color: var(--text-tertiary);
  }
</style>
