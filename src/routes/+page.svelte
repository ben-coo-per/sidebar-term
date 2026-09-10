<!-- App shell: sidebar on the left, the active Tab's Terminal on the right.
     See docs/architecture.md "Window": titleBarStyle Overlay, hidden title, the sidebar carries
     the ~28px traffic-light inset and its own data-tauri-drag-region (src/lib/sidebar/Sidebar.svelte). -->
<script lang="ts">
  import "$lib/theme.css";
  import Sidebar from "$lib/sidebar/Sidebar.svelte";
  import TerminalPane from "$lib/terminal/TerminalPane.svelte";
  import { activeTab, initLayout, layout } from "$lib/layout.svelte";
  import { initShortcuts } from "$lib/shortcuts";

  $effect(() => {
    void initLayout();
    const stopShortcuts = initShortcuts();
    return stopShortcuts;
  });

  const active = $derived(activeTab());
</script>

<main class="app">
  {#if layout.sidebarVisible}
    <Sidebar />
  {/if}
  <section class="main">
    <TerminalPane sessionId={active?.sessionId ?? null} />
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
    flex: 1 1 auto;
    min-width: 0;
    height: 100%;
    background: var(--term-bg);
  }
</style>
