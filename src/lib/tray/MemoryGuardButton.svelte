<!-- The Tray's Memory Guard toggle: while on, the heaviest Tab is frozen when memory gets tight.
     A count shows how many Tabs it has frozen (not those frozen by hand, which turning it off keeps). -->
<script lang="ts">
  import TrayButton from "./TrayButton.svelte";
  import GaugeIcon from "../sidebar/icons/GaugeIcon.svelte";
  import { memoryGuard, toggleMemoryGuard } from "../guard/memoryGuard.svelte";
  import { guardButtonTitle } from "../guard/model";

  const frozen = $derived(memoryGuard.frozen.filter((f) => !f.manual).length);
</script>

<TrayButton
  label="Memory Guard"
  title={guardButtonTitle(memoryGuard.on, memoryGuard.limitPercent, frozen)}
  on={memoryGuard.on}
  onclick={() => void toggleMemoryGuard()}
>
  <GaugeIcon on={memoryGuard.on} />
  {#if frozen > 0}
    <span class="count" aria-label="{frozen} frozen">{frozen}</span>
  {/if}
</TrayButton>

<style>
  .count {
    margin-left: 1px;
    font-size: 10px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--status-frozen);
  }
</style>
