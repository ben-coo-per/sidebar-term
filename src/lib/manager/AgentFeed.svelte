<!-- Manager's feed: what every agent did, newest first, one line each: the time, a dot for the
     kind of thing (asked amber, worked blue, failed red, the rest muted), the Tab's Title and the
     event. A hooked agent reports its tools; a screen-only one only its changes of status. -->
<script lang="ts">
  import { feedRows } from "./state.svelte";
  import { clockLabel, EVENT_TONES } from "./model";

  const rows = $derived(feedRows());
</script>

<div class="feed">
  {#each rows as row, i (i)}
    <div class="row">
      <span class="time">{clockLabel(row.event.at)}</span>
      <span class="dot {EVENT_TONES[row.event.kind]}"></span>
      <span class="text"><span class="who">{row.title}</span><span class="sep">{" · "}</span>{row.event.text}</span>
    </div>
  {:else}
    <p class="empty">Nothing yet. Agents' tool calls show here as they happen.</p>
  {/each}
</div>

<style>
  .row {
    display: grid;
    grid-template-columns: 40px 12px minmax(0, 1fr);
    align-items: baseline;
    padding: 5px 0;
    border-bottom: 1px solid var(--sidebar-bg-raised);
  }
  .time {
    font-family: "SF Mono", ui-monospace, Menlo, monospace;
    font-size: 11px;
    color: var(--text-tertiary);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    transform: translateY(-1px);
    background: var(--status-finished);
  }
  .dot.waiting {
    background: var(--status-needs-input);
  }
  .dot.working {
    background: var(--status-running);
  }
  .dot.failed {
    background: var(--diff-remove);
  }
  .text {
    font-size: 12.5px;
    line-height: 1.4;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .who {
    color: var(--text-primary);
  }
  .sep {
    color: var(--text-tertiary);
  }
  .empty {
    margin: 2px 0;
    font-size: 12.5px;
    color: var(--text-tertiary);
  }
</style>
