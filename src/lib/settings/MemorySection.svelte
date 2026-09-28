<!-- The Settings page's Memory section: the Tabs' CPU and memory, and Memory Guard with its limit.
     State: src/lib/panel/activity/settings.svelte.ts and src/lib/guard/memoryGuard.svelte.ts. -->
<script lang="ts">
  import { memoryGuard, setGuardLimit, toggleMemoryGuard } from "../guard/memoryGuard.svelte";
  import { GUARD_LIMITS, THAW_GAP } from "../guard/model";
  import { activitySettings, setTabStats } from "../panel/activity/settings.svelte";
</script>

<section>
  <div class="section-header">
    <div>
      <h2>Memory</h2>
      <p class="hint">What each Tab costs, and keeping heavy Tabs from slowing the Mac down.</p>
    </div>
  </div>
  <ul class="rows">
    <li class="row">
      <label class="check">
        <span class="label">
          CPU and memory on Tabs
          <span class="detail">Each Tab shows what its processes use. Reads every process every 2 seconds.</span>
        </span>
        <input type="checkbox" checked={activitySettings.tabStats} onchange={(e) => setTabStats(e.currentTarget.checked)} />
      </label>
    </li>
    <li class="row">
      <label class="check">
        <span class="label">
          Memory Guard
          <span class="detail">
            When memory use passes the limit, freezes the Tab using the most memory (never the one in view), and
            thaws it once memory is {THAW_GAP} points under the limit. A frozen Tab stops using CPU and stops
            growing but keeps the memory it holds. Going to it thaws it. Also in the Tray.
          </span>
        </span>
        <input type="checkbox" checked={memoryGuard.on} onchange={() => void toggleMemoryGuard()} />
      </label>
    </li>
    <li class="row">
      <label class="check">
        <span class="label">
          Memory Guard limit
          <span class="detail">Memory Used, as in the Activity view, as a share of this Mac's memory.</span>
        </span>
        <select
          class="select"
          value={memoryGuard.limitPercent}
          onchange={(e) => void setGuardLimit(Number(e.currentTarget.value))}
        >
          {#each GUARD_LIMITS as limit (limit)}
            <option value={limit}>{limit}%</option>
          {/each}
        </select>
      </label>
    </li>
  </ul>
</section>
