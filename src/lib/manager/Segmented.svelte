<!-- A segmented control for the window bar: Tabs / Manager, and Manager's zoom. One
     segment is selected; a segment may end with a count (the Manager segment's waiting agents).
     It sits in a Tauri drag region, so it opts out of dragging. -->
<script lang="ts" generics="T extends string">
  let {
    items,
    selected,
    onselect,
    label,
  }: {
    items: { id: T; label: string; count?: number; title?: string }[];
    selected: T;
    onselect: (id: T) => void;
    /** Accessible name of the group. */
    label: string;
  } = $props();
</script>

<div class="segmented" role="radiogroup" aria-label={label}>
  {#each items as item (item.id)}
    <button
      type="button"
      class="segment"
      class:selected={item.id === selected}
      role="radio"
      aria-checked={item.id === selected}
      title={item.title}
      onclick={() => onselect(item.id)}
    >
      {item.label}{#if item.count}<span class="count">{item.count}</span>{/if}
    </button>
  {/each}
</div>

<style>
  .segmented {
    flex: none;
    display: flex;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--sidebar-border);
    border-radius: var(--radius-md);
    background: var(--term-bg);
    -webkit-app-region: no-drag;
  }
  .segment {
    appearance: none;
    display: flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 12px;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-tertiary);
    font: inherit;
    font-size: 12.5px;
    white-space: nowrap;
    cursor: default;
  }
  .segment:hover:not(.selected) {
    color: var(--text-secondary);
  }
  .segment.selected {
    background: var(--sidebar-bg-active);
    color: var(--text-primary);
  }
  .count {
    font-size: 11px;
    font-weight: 600;
    color: var(--status-needs-input);
    font-variant-numeric: tabular-nums;
  }
</style>
