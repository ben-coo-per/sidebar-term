<!-- App shell: sidebar on the left, the active Tab's Terminal on the right.
     See docs/architecture.md "Window": titleBarStyle Overlay, hidden title, the sidebar carries
     the ~28px traffic-light inset and its own data-tauri-drag-region (src/lib/sidebar/Sidebar.svelte). -->
<script lang="ts">
  import "$lib/theme.css";
  import Sidebar from "$lib/sidebar/Sidebar.svelte";
  import TerminalPane from "$lib/terminal/TerminalPane.svelte";
  import { activeTab, initLayout, layout, localSessionId, tabSessionKey } from "$lib/layout.svelte";
  import { initHosts } from "$lib/host/hosts.svelte";
  import { initShortcuts } from "$lib/shortcuts";
  import { initHotkeys } from "$lib/hotkeys.svelte";
  import { initUsageSettings } from "$lib/panel/usage/settings.svelte";
  import { initCaffeinate } from "$lib/tray/caffeinate.svelte";
  import { initMemoryGuard, setVisibleSession } from "$lib/guard/memoryGuard.svelte";
  import { activitySettings, initActivitySettings } from "$lib/panel/activity/settings.svelte";
  import { watch as watchActivity } from "$lib/panel/activity/activity.svelte";
  import { onMenuSettings } from "$lib/ipc";
  import { initDropGuard } from "$lib/terminal/drop";
  import SettingsPage from "$lib/settings/SettingsPage.svelte";
  import ResumeBanner from "$lib/resume/ResumeBanner.svelte";
  import { initResume } from "$lib/resume/resume.svelte";
  import { initRemote } from "$lib/remote/remote.svelte";
  import { closeSettings, openSettings, settingsPage } from "$lib/settings/visibility.svelte";
  import { untrack } from "svelte";

  $effect(() => {
    // Resume needs the Tabs: it drops entries whose Tab is gone. Paired Hosts join once the
    // local Host's layout is in, so their sections come after its Groups.
    let stopHosts: (() => void) | null = null;
    let stopped = false;
    void initLayout().then(() => {
      void initResume();
      if (!stopped) stopHosts = initHosts();
    });
    void initHotkeys();
    void initUsageSettings();
    void initActivitySettings();
    const stopShortcuts = initShortcuts();
    const stopDropGuard = initDropGuard();
    const stopCaffeinate = initCaffeinate();
    const stopRemote = initRemote();
    const stopMemoryGuard = initMemoryGuard();
    const menuSettings = onMenuSettings(openSettings);
    return () => {
      stopped = true;
      stopHosts?.();
      stopShortcuts();
      stopDropGuard();
      stopCaffeinate();
      stopRemote();
      stopMemoryGuard();
      void menuSettings.then((stop) => stop());
    };
  });

  // Going to a Tab (click, Hotkey, new Tab) leaves the Settings page.
  $effect(() => {
    void layout.activeTabId;
    untrack(closeSettings);
  });

  const active = $derived(activeTab());

  // Memory Guard never freezes the Tab in view, and going to a frozen Tab thaws it. It is this
  // Mac's: a paired Host's Tab in view leaves no local Session in view.
  $effect(() => setVisibleSession(localSessionId(active)));

  // Tabs show their CPU and memory from Activity samples, taken only while something shows them.
  $effect(() => {
    if (activitySettings.tabStats) return watchActivity();
  });
</script>

<main class="app">
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
