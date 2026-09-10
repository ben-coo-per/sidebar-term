<!-- App shell. OWNER: sidebar agent (composition of Sidebar + TerminalPane). -->
<script lang="ts">
  import TerminalPane from "$lib/terminal/TerminalPane.svelte";
</script>

<main class="app">
  <aside class="sidebar" data-tauri-drag-region></aside>
  <section class="main"><TerminalPane sessionId={null} /></section>
</main>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
  }
  .app {
    display: grid;
    grid-template-columns: 240px 1fr;
    height: 100vh;
  }
</style>
