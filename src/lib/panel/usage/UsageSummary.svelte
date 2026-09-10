<!-- One-line Usage summary for the Usage header while the view is closed: each agent's fullest
     limit, e.g. "Claude 48%  Codex 3%". -->
<script lang="ts">
  import { usage } from "./usage.svelte";
  import { AGENT_SHORT_NAMES, formatPercent, peakPercent, usageLevel } from "./model";

  const peaks = $derived(
    (usage.snapshot?.agents ?? [])
      .map((a) => ({ agent: a.agent, percent: peakPercent(a, usage.now) }))
      .filter((p): p is { agent: typeof p.agent; percent: number } => p.percent !== null),
  );
</script>

{#if peaks.length > 0}
  <span class="summary" title="Each agent's fullest usage limit">
    {#each peaks as p (p.agent)}
      <span class="k">{AGENT_SHORT_NAMES[p.agent]}</span><span class={usageLevel(p.percent)}
        >{formatPercent(p.percent)}</span
      >
    {/each}
  </span>
{/if}

<style>
  .summary {
    flex: none;
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
    color: var(--text-secondary);
    white-space: nowrap;
  }
  .k {
    color: var(--text-tertiary);
    margin: 0 3px 0 6px;
  }
  .high {
    color: var(--usage-high);
  }
  .full {
    color: var(--usage-full);
  }
</style>
