<!-- App shell: the whole window as Manager (src/lib/manager/Manager.svelte, with the selected
     Tab's Terminal), or the sidebar on the left and the active Tab's Terminal on the right (Tabs
     mode), per the window's mode: Manager until the user picks. Neither shows until the saved
     mode is read, so the window does not open in one and jump to the other.
     See docs/architecture.md "Window": titleBarStyle Overlay, hidden title, the sidebar carries
     the ~28px traffic-light inset and its own data-tauri-drag-region (src/lib/sidebar/Sidebar.svelte). -->
<script lang="ts">
  import "$lib/theme.css";
  import Sidebar from "$lib/sidebar/Sidebar.svelte";
  import TerminalPane from "$lib/terminal/TerminalPane.svelte";
  import { activeTab, initLayout, layout, localSessionId, managerTab, tabSessionKey } from "$lib/layout.svelte";
  import { initHosts } from "$lib/host/hosts.svelte";
  import { initShortcuts } from "$lib/shortcuts";
  import { initHotkeys } from "$lib/hotkeys.svelte";
  import { initAppearance } from "$lib/appearance/appearance.svelte";
  import { initUsageSettings } from "$lib/panel/usage/settings.svelte";
  import { initCaffeinate } from "$lib/tray/caffeinate.svelte";
  import { initMemoryGuard, setVisibleSession } from "$lib/guard/memoryGuard.svelte";
  import { activitySettings, initActivitySettings } from "$lib/panel/activity/settings.svelte";
  import { watch as watchActivity } from "$lib/panel/activity/activity.svelte";
  import { onMenuSettings } from "$lib/ipc";
  import { initDropGuard } from "$lib/terminal/drop";
  import SettingsPage from "$lib/settings/SettingsPage.svelte";
  import Manager from "$lib/manager/Manager.svelte";
  import WindowBar from "$lib/window/WindowBar.svelte";
  import { initAgentFeed } from "$lib/manager/feed.svelte";
  import ResumeBanner from "$lib/resume/ResumeBanner.svelte";
  import { initResume } from "$lib/resume/resume.svelte";
  import { initRemote } from "$lib/remote/remote.svelte";
  import { closeSettings, openSettings, settingsPage } from "$lib/settings/visibility.svelte";
  import { untrack } from "svelte";

  $effect(() => {
    // Resume needs the Tabs: it drops entries whose Tab is gone. Paired Hosts join once the
    // local Host's layout is in: their Tabs are linked into it.
    let stopHosts: (() => void) | null = null;
    let stopped = false;
    void initLayout().then(() => {
      void initResume();
      if (!stopped) stopHosts = initHosts();
    });
    void initAppearance();
    void initHotkeys();
    void initUsageSettings();
    void initActivitySettings();
    const stopShortcuts = initShortcuts();
    const stopDropGuard = initDropGuard();
    const stopCaffeinate = initCaffeinate();
    const stopRemote = initRemote();
    const stopMemoryGuard = initMemoryGuard();
    const stopAgentFeed = initAgentFeed();
    const menuSettings = onMenuSettings(openSettings);
    return () => {
      stopped = true;
      stopHosts?.();
      stopShortcuts();
      stopDropGuard();
      stopCaffeinate();
      stopRemote();
      stopMemoryGuard();
      stopAgentFeed();
      void menuSettings.then((stop) => stop());
    };
  });

  // Going to a Tab (click, Hotkey, new Tab) leaves the Settings page.
  $effect(() => {
    void layout.activeTabId;
    untrack(closeSettings);
  });

  const active = $derived(activeTab());

  const managerMode = $derived(layout.ready && layout.mode === "manager");

  // Memory Guard never freezes the Tab in view, and going to a frozen Tab thaws it. It is this
  // Mac's: a linked Tab in view leaves no local Session in view; Manager shows the selected Tab's.
  $effect(() => setVisibleSession(localSessionId(managerMode ? managerTab() : active)));

  // Tabs show their CPU and memory from Activity samples, taken only while something shows them.
  $effect(() => {
    if (activitySettings.tabStats) return watchActivity();
  });
</script>

<div class="window">
  <WindowBar />
  <main class="app">
    {#if !layout.ready}
      <section class="main"></section>
    {:else if managerMode}
      <section class="main">
        <Manager />
        {#if settingsPage.open}
          <SettingsPage />
        {/if}
      </section>
    {:else}
      {#if layout.sidebarVisible}
        <Sidebar />
      {/if}
      <section class="main">
        <div class="terminal">
          <TerminalPane sessionKey={tabSessionKey(active)} />
        </div>
        <ResumeBanner />
        {#if settingsPage.open}
          <SettingsPage />
        {/if}
      </section>
    {/if}
  </main>
</div>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
    background: var(--term-bg);
  }
  .window {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .app {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
  }
  .main {
    position: relative;
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-width: 0;
    height: 100%;
    background: var(--term-bg);
  }
  /* The Resume banner, when shown, takes its height from the Terminal, which refits. */
  .terminal {
    flex: 1 1 auto;
    min-height: 0;
  }
</style>
