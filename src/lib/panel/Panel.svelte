<!-- The Panel: a collapsible, resizable area at the bottom of the sidebar showing one view at a
     time, picked from a tab strip in its header. Views are listed in ./views.ts. The sidebar
     hides the Panel when it is too narrow. See docs/architecture.md "Panel". -->
<script lang="ts">
  import { layout, setPanelHeight, setPanelView, togglePanelCollapsed } from "../layout.svelte";
  import { PANEL_VIEWS, type PanelViewId } from "./views";
  import ChevronIcon from "../sidebar/icons/ChevronIcon.svelte";
  import ActivityView from "./activity/ActivityView.svelte";
  import ActivitySummary from "./activity/ActivitySummary.svelte";
  import { watch as watchActivity } from "./activity/activity.svelte";

  /** The Panel never takes more than this share of the sidebar, so the Groups stay usable. */
  const MAX_SHARE = 0.7;

  const panel = $derived(layout.panel);

  // Sample only while the Activity view is the one shown (collapsed too: the header summarises).
  $effect(() => {
    if (layout.panel.view === "activity") return watchActivity();
  });

  let el: HTMLElement | undefined = $state();
  let resizing = $state(false);

  function onViewClick(e: MouseEvent, id: PanelViewId) {
    e.stopPropagation();
    if (id === panel.view) togglePanelCollapsed();
    else setPanelView(id);
  }

  function onHeaderKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      togglePanelCollapsed();
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
  class:collapsed={panel.collapsed}
  style:height={panel.collapsed ? null : `${panel.height}px`}
  style:max-height="{MAX_SHARE * 100}%"
  bind:this={el}
  aria-label="Panel"
>
  {#if !panel.collapsed}
    <div
      class="resize-handle"
      class:active={resizing}
      role="separator"
      aria-orientation="horizontal"
      tabindex="-1"
      onpointerdown={startResize}
    ></div>
  {/if}

  <div
    class="header"
    role="button"
    tabindex="0"
    aria-expanded={!panel.collapsed}
    title={panel.collapsed ? "Expand" : "Collapse"}
    onclick={togglePanelCollapsed}
    onkeydown={onHeaderKeydown}
  >
    <span class="chevron" class:collapsed={panel.collapsed}><ChevronIcon size={11} /></span>
    <div class="views" role="tablist">
      {#each PANEL_VIEWS as view (view.id)}
        <button
          type="button"
          role="tab"
          class="view-tab"
          class:selected={view.id === panel.view}
          aria-selected={view.id === panel.view}
          tabindex="-1"
          onclick={(e) => onViewClick(e, view.id)}
        >
          {view.label}
        </button>
      {/each}
    </div>
    {#if panel.collapsed && panel.view === "activity"}
      <ActivitySummary />
    {/if}
  </div>

  {#if !panel.collapsed}
    <div class="body" role="tabpanel">
      {#if panel.view === "activity"}
        <ActivityView />
      {/if}
    </div>
  {/if}
</section>

<style>
  .panel {
    position: relative;
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
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
    margin: 3px 4px;
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
  .views {
    flex: 1 1 auto;
    display: flex;
    gap: 2px;
    min-width: 0;
    overflow: hidden;
  }
  .view-tab {
    appearance: none;
    background: transparent;
    border: none;
    padding: 2px 5px;
    margin-left: -5px;
    border-radius: 4px;
    font: inherit;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    white-space: nowrap;
    color: var(--text-tertiary);
    cursor: default;
  }
  .view-tab + .view-tab {
    margin-left: 0;
  }
  .view-tab.selected,
  .header:hover .view-tab.selected {
    color: var(--text-secondary);
  }
  .view-tab:not(.selected):hover {
    color: var(--text-secondary);
    background: var(--sidebar-bg-active);
  }
  .body {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
</style>
