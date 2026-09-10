<!-- One-line Activity summary for the collapsed Panel's header: machine CPU and Memory Used. -->
<script lang="ts">
  import { activity } from "./activity.svelte";
  import { formatBytes, machineCpu } from "./model";

  const snapshot = $derived(activity.snapshot);
</script>

{#if snapshot}
  <span class="summary" title="CPU load and memory used">
    <span class="k">CPU</span>{Math.round(machineCpu(snapshot))}%
    <span class="k">Mem</span>{formatBytes(snapshot.memUsed)}
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
</style>
