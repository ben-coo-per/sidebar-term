<!-- The window's top bar, the same in both modes: right of the traffic lights, the Tabs / Manager
     switch, Manager's zoom (in Manager), then the Usage summary and the Tray at the right. It is
     the window's Tauri drag region; its controls opt out (docs/architecture.md "Window"). -->
<script lang="ts">
  import { layout, setManagerZoom } from "../layout.svelte";
  import { MANAGER_ZOOMS, type ManagerZoom } from "../sidebar/settings";
  import Tray from "../tray/Tray.svelte";
  import UsageSummary from "../panel/usage/UsageSummary.svelte";
  import { watch as watchUsage } from "../panel/usage/usage.svelte";
  import { usageSettings } from "../panel/usage/settings.svelte";
  import ModeSwitch from "../manager/ModeSwitch.svelte";
  import Segmented from "../manager/Segmented.svelte";
  import { ZOOM_LABELS } from "../manager/model";

  const zoomItems = MANAGER_ZOOMS.map((id) => ({ id, label: ZOOM_LABELS[id] }));

  // The summary reads Usage whichever mode shows and whether or not the Panel is open.
  $effect(() => {
    if (usageSettings.ready) return watchUsage([...usageSettings.agents]);
  });
</script>

<header class="window-bar" data-tauri-drag-region>
  <ModeSwitch />
  {#if layout.mode === "manager"}
    <Segmented label="Zoom" items={zoomItems} selected={layout.managerZoom} onselect={(z: ManagerZoom) => setManagerZoom(z)} />
  {/if}
  <span class="spacer" data-tauri-drag-region></span>
  <span class="usage"><UsageSummary /></span>
  <Tray />
</header>

<style>
  .window-bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    height: var(--titlebar-inset);
    /* Nothing runs under the traffic lights. */
    padding: 0 10px 0 var(--traffic-lights-width);
    background: var(--sidebar-bg);
    border-bottom: 1px solid var(--sidebar-border);
    font-family:
      -apple-system,
      BlinkMacSystemFont,
      "SF Pro Text",
      sans-serif;
    color: var(--text-primary);
    user-select: none;
    -webkit-app-region: drag;
  }
  .spacer {
    flex: 1 1 auto;
    align-self: stretch;
  }
  /* Readable at a glance from across the bar. */
  .usage :global(.summary) {
    font-size: 12px;
  }
</style>
