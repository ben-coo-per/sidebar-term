<!-- The Panel: a collapsible, resizable area at the bottom of the sidebar. An accordion: each view
     (./views.ts) has its own header, and opening one closes the others. A closed view's header
     carries a one-line summary. The sidebar hides the Panel when it is too narrow.
     See docs/architecture.md "Panel". -->
<script lang="ts">
  import { layout, setPanelHeight, setPanelView, togglePanelCollapsed } from "../layout.svelte";
  import { PANEL_VIEWS, type PanelViewId } from "./views";
  import ChevronIcon from "../sidebar/icons/ChevronIcon.svelte";
  import ActivityView from "./activity/ActivityView.svelte";
  import ActivitySummary from "./activity/ActivitySummary.svelte";
  import { watch as watchActivity } from "./activity/activity.svelte";
  import UsageView from "./usage/UsageView.svelte";
  import UsageSummary from "./usage/UsageSummary.svelte";
  import { watch as watchUsage } from "./usage/usage.svelte";
  import { usageSettings } from "./usage/settings.svelte";

  /** The Panel never takes more than this share of the sidebar, so the Groups stay usable. */
  const MAX_SHARE = 0.7;

  const panel = $derived(layout.panel);
  /** The open view; null while every view is closed to its header. */
  const open = $derived<PanelViewId | null>(panel.collapsed ? null : panel.view);

  // Every header shows either its view or its summary, so both read while the Panel is shown.
  $effect(() => watchActivity());
  $effect(() => {
    if (usageSettings.ready) return watchUsage([...usageSettings.agents]);
  });

  let el: HTMLElement | undefined = $state();
  let resizing = $state(false);

  /** Open `id`, or close it if it is the open one. */
  function toggle(id: PanelViewId) {
    if (id === panel.view) togglePanelCollapsed();
    else setPanelView(id);
  }

  function onHeaderKeydown(e: KeyboardEvent, id: PanelViewId) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      toggle(id);
    }
  }

  function startResize(e: PointerEvent) {
    e.preventDefault();
    resizing = true;
    const startY = e.clientY;
    const startHeight = el?.getBoundingClientRect().height ?? layout.panel.height;
    const maxHeight = (el?.parentElement?.clientHeight ?? Infinity) * MAX_SHARE;

    function onMove(ev: PointerEvent) {
      setPanelHeight(Math.min(maxHeight, startHeight - (ev.clientY - startY)));
    }
    function onUp() {
      resizing = false;
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    }
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }
</script>

<section
  class="panel"
  class:collapsed={open === null}
  style:height={open === null ? null : `${panel.height}px`}
  style:max-height="{MAX_SHARE * 100}%"
  bind:this={el}
  aria-label="Panel"
>
  {#if open !== null}
    <div
      class="resize-handle"
      class:active={resizing}
      role="separator"
      aria-orientation="horizontal"
      tabindex="-1"
      onpointerdown={startResize}
    ></div>
  {/if}

  {#each PANEL_VIEWS as view (view.id)}
    {@const isOpen = open === view.id}
    <div
      class="header"
      role="button"
      tabindex="0"
      aria-expanded={isOpen}
      aria-controls="panel-{view.id}"
      title={isOpen ? "Collapse" : "Expand"}
      onclick={() => toggle(view.id)}
      onkeydown={(e) => onHeaderKeydown(e, view.id)}
    >
      <span class="chevron" class:collapsed={!isOpen}><ChevronIcon size={11} /></span>
      <span class="title" class:open={isOpen}>{view.label}</span>
      {#if !isOpen}
        {#if view.id === "activity"}
          <ActivitySummary />
        {:else if view.id === "usage"}
          <UsageSummary />
        {/if}
      {/if}
    </div>

    {#if isOpen}
      <div class="body" id="panel-{view.id}" role="region" aria-label={view.label}>
        {#if view.id === "activity"}
          <ActivityView />
        {:else if view.id === "usage"}
          <UsageView />
        {/if}
      </div>
    {/if}
  {/each}
</section>

<style>
  .panel {
    position: relative;
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 2px 0;
    border-top: 1px solid var(--sidebar-divider);
  }
  .panel.collapsed {
    height: auto;
  }
  .resize-handle {
    position: absolute;
    top: -3px;
    left: 0;
    right: 0;
    height: 6px;
    cursor: row-resize;
    z-index: 5;
  }
  .resize-handle:hover,
  .resize-handle.active {
    background: var(--accent-dim);
  }
  .header {
    flex: none;
    display: flex;
    align-items: center;
    gap: 5px;
    height: var(--group-header-height);
    padding: 0 8px;
    margin: 1px 4px;
    border-radius: var(--radius-sm);
    user-select: none;
    color: var(--text-secondary);
    outline: none;
  }
  .header:hover {
    background: var(--sidebar-bg-raised);
  }
  .header:focus-visible {
    box-shadow: inset 0 0 0 1.5px var(--focus-ring);
  }
  .chevron {
    flex: none;
    display: flex;
    color: var(--text-tertiary);
    transition: transform var(--duration-fast) var(--ease-standard);
    transform: rotate(90deg);
  }
  .chevron.collapsed {
    transform: rotate(0deg);
  }
  .title {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    white-space: nowrap;
    color: var(--text-tertiary);
  }
  .title.open,
  .header:hover .title {
    color: var(--text-secondary);
  }
  .body {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
</style>
