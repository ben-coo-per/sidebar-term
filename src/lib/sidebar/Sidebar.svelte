<!-- The sidebar: Group headers + Tab rows, resizable, with the Panel beneath them and the shared
     context menu and confirm dialog mounted once. Runs to the top of the window; its header is the
     Tauri drag region (see docs/architecture.md "Window"). -->
<script lang="ts">
  import {
    layout,
    newGroup,
    newTab,
    setSidebarWidth,
    MIN_SIDEBAR_WIDTH,
    MAX_SIDEBAR_WIDTH,
    PANEL_MIN_SIDEBAR_WIDTH,
  } from "../layout.svelte";
  import Panel from "../panel/Panel.svelte";
  import GroupHeader from "./GroupHeader.svelte";
  import TabRow from "./TabRow.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import PlusIcon from "./icons/PlusIcon.svelte";
  import { contextMenuBox, closeContextMenu } from "./menu.svelte";

  const totalTabs = $derived(Object.keys(layout.tabs).length);

  let resizing = $state(false);

  function startResize(e: PointerEvent) {
    e.preventDefault();
    resizing = true;
    const startX = e.clientX;
    const startWidth = layout.sidebarWidth;

    function onMove(ev: PointerEvent) {
      setSidebarWidth(startWidth + (ev.clientX - startX));
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

<aside class="sidebar" style:width="{layout.sidebarWidth}px">
  <div class="drag-region" data-tauri-drag-region></div>

  <div class="groups" role="tree" aria-label="Tabs">
    {#if !layout.ready}
      <!-- Startup: loading the persisted layout and respawning Sessions. -->
    {:else if totalTabs === 0}
      <div class="empty-state">
        <p class="empty-title">No Tabs open</p>
        <button type="button" class="new-tab-btn" onclick={() => void newTab()}>
          <PlusIcon size={11} />
          New Tab
        </button>
        <p class="empty-hint">⌘T</p>
      </div>
    {:else}
      {#each layout.groups as group (group.id)}
        <GroupHeader {group} />
        {#if !group.collapsed}
          {#each group.tabIds as tabId (tabId)}
            {#if layout.tabs[tabId]}
              <TabRow tab={layout.tabs[tabId]} />
            {/if}
          {/each}
        {/if}
      {/each}
    {/if}
  </div>

  <div class="footer">
    <button type="button" class="footer-btn" onclick={() => void newTab()} title="New Tab (⌘T)">
      <PlusIcon size={10} />
      Tab
    </button>
    <button type="button" class="footer-btn" onclick={() => newGroup()} title="New Group (⌘⇧N)">
      <PlusIcon size={10} />
      Group
    </button>
  </div>

  {#if layout.ready && layout.sidebarWidth >= PANEL_MIN_SIDEBAR_WIDTH}
    <Panel />
  {/if}

  <div
    class="resize-handle"
    class:active={resizing}
    role="separator"
    aria-orientation="vertical"
    aria-valuemin={MIN_SIDEBAR_WIDTH}
    aria-valuemax={MAX_SIDEBAR_WIDTH}
    aria-valuenow={layout.sidebarWidth}
    tabindex="-1"
    onpointerdown={startResize}
  ></div>
</aside>

{#if contextMenuBox.current}
  <ContextMenu x={contextMenuBox.current.x} y={contextMenuBox.current.y} items={contextMenuBox.current.items} onClose={closeContextMenu} />
{/if}

<ConfirmDialog />

<style>
  .sidebar {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--sidebar-bg);
    border-right: 1px solid var(--sidebar-border);
    font-family:
      -apple-system,
      BlinkMacSystemFont,
      "SF Pro Text",
      sans-serif;
    color: var(--text-primary);
    min-width: 0;
  }
  .drag-region {
    flex: none;
    height: var(--titlebar-inset);
    -webkit-app-region: drag;
  }
  .groups {
    flex: 1 1 auto;
    overflow-y: auto;
    overflow-x: hidden;
    padding-bottom: 6px;
  }
  .groups::-webkit-scrollbar {
    width: 8px;
  }
  .groups::-webkit-scrollbar-thumb {
    background: var(--scrollbar-thumb);
    border-radius: 4px;
  }
  .footer {
    flex: none;
    display: flex;
    gap: 4px;
    padding: 6px 8px;
    border-top: 1px solid var(--sidebar-divider);
  }
  .footer-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    appearance: none;
    background: transparent;
    border: none;
    color: var(--text-tertiary);
    font: inherit;
    font-size: 11px;
    padding: 4px 7px;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .footer-btn:hover {
    background: var(--sidebar-bg-raised);
    color: var(--text-secondary);
  }
  .resize-handle {
    position: absolute;
    top: 0;
    right: -3px;
    width: 6px;
    height: 100%;
    cursor: col-resize;
    z-index: 10;
  }
  .resize-handle:hover,
  .resize-handle.active {
    background: var(--accent-dim);
  }
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 48px 16px;
    text-align: center;
  }
  .empty-title {
    margin: 0;
    font-size: 12.5px;
    color: var(--text-secondary);
  }
  .new-tab-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    appearance: none;
    background: var(--accent);
    color: var(--text-on-accent);
    border: none;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    padding: 6px 12px;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .new-tab-btn:hover {
    background: var(--accent-strong);
  }
  .empty-hint {
    margin: 0;
    font-size: 11px;
    color: var(--text-tertiary);
  }
</style>
