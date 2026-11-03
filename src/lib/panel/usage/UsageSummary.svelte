<!-- One-line Usage summary for the Usage header while the view is closed: each agent's fullest
     account-wide limit (5h or Week) and which one it is, e.g. "Claude 48% 5h  Codex 3% Week". -->
<script lang="ts">
  import { usage } from "./usage.svelte";
  import { AGENT_SHORT_NAMES, formatPercent, peakWindow, usageLevel } from "./model";

  const peaks = $derived(
    (usage.snapshot?.agents ?? []).flatMap((a) => {
      const peak = peakWindow(a, usage.now);
      return peak ? [{ agent: a.agent, ...peak }] : [];
    }),
  );
</script>

{#if peaks.length > 0}
  <span class="summary" title="Each agent's fullest usage limit: its 5-hour or weekly one">
    {#each peaks as p (p.agent)}
      <span class="k">{AGENT_SHORT_NAMES[p.agent]}</span><span class={usageLevel(p.percent)}
        >{formatPercent(p.percent)}</span
      ><span class="w">{p.label}</span>
    {/each}
  </span>
{/if}

<style>
  .summary {
    flex: none;
    font-size: calc(10.5px * var(--ui-font-scale));
    font-variant-numeric: tabular-nums;
    color: var(--text-secondary);
    white-space: nowrap;
  }
  .k {
    color: var(--text-tertiary);
    margin: 0 3px 0 6px;
  }
  .w {
    color: var(--text-tertiary);
    margin-left: 3px;
  }
  .high {
    color: var(--usage-high);
  }
  .full {
    color: var(--usage-full);
  }
</style>
