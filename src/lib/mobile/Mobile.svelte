<!-- The phone's page (/m): pairing, then the Tab list, then one Tab's Session on screen.
     See docs/architecture.md "Remote". -->
<script lang="ts">
  import PairScreen from "./PairScreen.svelte";
  import TabList from "./TabList.svelte";
  import TerminalScreen from "./TerminalScreen.svelte";
  import { currentTab, initMobile, mobile } from "./store.svelte";

  $effect(() => initMobile());

  const tab = $derived(currentTab());
</script>

<div class="mobile">
  {#if mobile.phase === "loading"}
    <p class="loading">…</p>
  {:else if mobile.phase === "pair"}
    <PairScreen />
  {:else if tab}
    <TerminalScreen {tab} />
  {:else}
    <TabList />
  {/if}
</div>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
    background: var(--term-bg);
    color: var(--text-primary);
    font-family:
      -apple-system,
      BlinkMacSystemFont,
      "SF Pro Text",
      sans-serif;
    -webkit-text-size-adjust: 100%;
    -webkit-tap-highlight-color: transparent;
    overscroll-behavior: none;
  }
  .mobile {
    height: 100%;
  }
  .loading {
    margin: 40vh 0 0;
    text-align: center;
    color: var(--text-tertiary);
  }
</style>
