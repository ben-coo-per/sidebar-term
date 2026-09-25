<!-- One icon button in the Tray. Pass `on` for a toggle (muted off, lit on); omit it for a plain
     action. The icon is the children. -->
<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    label,
    title = label,
    on,
    onclick,
    children,
  }: {
    /** Accessible name. */
    label: string;
    /** Tooltip; defaults to `label`. */
    title?: string;
    on?: boolean;
    onclick: () => void;
    children: Snippet;
  } = $props();
</script>

<button type="button" class="tray-btn" class:on aria-label={label} aria-pressed={on} {title} {onclick}>
  {@render children()}
</button>

<style>
  .tray-btn {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 22px;
    height: 20px;
    padding: 0 4px;
    appearance: none;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);
    cursor: pointer;
    outline: none;
    -webkit-app-region: no-drag;
    transition:
      color var(--duration-fast) var(--ease-standard),
      background var(--duration-fast) var(--ease-standard);
  }
  .tray-btn:hover {
    background: var(--sidebar-bg-raised);
    color: var(--text-secondary);
  }
  .tray-btn:focus-visible {
    box-shadow: inset 0 0 0 1.5px var(--focus-ring);
  }
  .tray-btn.on {
    color: var(--tray-on);
  }
  .tray-btn.on:hover {
    background: var(--tray-on-dim);
  }
</style>
