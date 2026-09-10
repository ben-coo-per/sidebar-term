<!-- App shell: sidebar on the left, the active Tab's Terminal on the right.
     See docs/architecture.md "Window": titleBarStyle Overlay, hidden title, the sidebar carries
     the ~28px traffic-light inset and its own data-tauri-drag-region (src/lib/sidebar/Sidebar.svelte). -->
<script lang="ts">
  import "$lib/theme.css";
  import Sidebar from "$lib/sidebar/Sidebar.svelte";
  import TerminalPane from "$lib/terminal/TerminalPane.svelte";
  import { activeTab, initLayout, layout } from "$lib/layout.svelte";
  import { initShortcuts } from "$lib/shortcuts";
  import { initHotkeys } from "$lib/hotkeys.svelte";
  import { initUsageSettings } from "$lib/panel/usage/settings.svelte";
  import { initCaffeinate } from "$lib/tray/caffeinate.svelte";
  import { onMenuSettings } from "$lib/ipc";
  import { initDropGuard } from "$lib/terminal/drop";
  import SettingsPage from "$lib/settings/SettingsPage.svelte";
  import { closeSettings, openSettings, settingsPage } from "$lib/settings/visibility.svelte";
  import { untrack } from "svelte";

  $effect(() => {
    void initLayout();
    void initHotkeys();
    void initUsageSettings();
    const stopShortcuts = initShortcuts();
    const stopDropGuard = initDropGuard();
    const stopCaffeinate = initCaffeinate();
    const menuSettings = onMenuSettings(openSettings);
    return () => {
      stopShortcuts();
      stopDropGuard();
      stopCaffeinate();
      void menuSettings.then((stop) => stop());
    };
  });

  // Going to a Tab (click, Hotkey, new Tab) leaves the Settings page.
  $effect(() => {
    void layout.activeTabId;
    untrack(closeSettings);
  });

  const active = $derived(activeTab());
</script>

<main class="app">
  {#if layout.sidebarVisible}
    <Sidebar />
  {/if}
  <section class="main">
    <TerminalPane sessionId={active?.sessionId ?? null} />
    {#if settingsPage.open}
      <SettingsPage />
    {/if}
  </section>
</main>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
    background: var(--term-bg);
  }
  .app {
    display: flex;
    height: 100vh;
  }
  .main {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
    height: 100%;
    background: var(--term-bg);
  }
</style>
