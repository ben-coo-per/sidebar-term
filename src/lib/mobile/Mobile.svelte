<!-- The phone's page (/m): pairing, then the lists (Manager and the Tabs), then one Tab's
     Session on screen. See docs/architecture.md "Host protocol". -->
<script lang="ts">
  import Lists from "./Lists.svelte";
  import PairScreen from "./PairScreen.svelte";
  import TerminalScreen from "./TerminalScreen.svelte";
  import { untrack } from "svelte";
  import { currentTab, initMobile, mobile } from "./store.svelte";

  // Once: starting reads and writes the store, which must not start it again.
  $effect(() => untrack(initMobile));

  const tab = $derived(currentTab());
</script>

<div class="mobile">
  {#if mobile.phase === "loading"}
    <p class="loading">…</p>
  {:else if mobile.phase === "pair" || mobile.pairWith}
    {#key mobile.pairWith}
      <PairScreen />
    {/key}
  {:else if tab}
    <TerminalScreen {tab} />
  {:else}
    <Lists />
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
