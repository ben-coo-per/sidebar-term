<!-- The 2 px bar of a Suite: a red segment for failures at the left, then the filled share in
     blue (half opacity for an estimate from the history, amber past the longest run), or a
     short segment sliding left to right when nothing is known. Under a Tab's Badge line and in
     the Activity view's Tests block. -->
<script lang="ts">
  import type { SuiteView } from "./model";

  let { view }: { view: SuiteView } = $props();

  const filled = $derived(view.fraction === null ? 0 : Math.max(0, view.fraction - view.failedFraction));
</script>

<span class="bar {view.tone}" class:indeterminate={view.fraction === null} aria-hidden="true">
  {#if view.fraction === null}
    <span class="slider"></span>
  {:else}
    {#if view.failedFraction > 0}
      <span class="segment failed" style:width="{view.failedFraction * 100}%"></span>
    {/if}
    <span class="segment fill" style:width="{filled * 100}%"></span>
  {/if}
</span>

<style>
  .bar {
    position: relative;
    display: flex;
    width: 100%;
    height: 2px;
    margin-top: 2px;
    border-radius: 1px;
    overflow: hidden;
    background: var(--meter-track);
  }
  .segment {
    flex: none;
    height: 100%;
    transition: width var(--duration-medium) var(--ease-standard);
  }
  .fill {
    background: var(--status-running);
  }
  .estimate .fill {
    opacity: 0.5;
  }
  .overrun .fill {
    background: var(--status-needs-input);
  }
  .failed {
    background: var(--danger);
  }
  .slider {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 30%;
    background: var(--status-running);
    animation: slide 1.6s var(--ease-standard) infinite;
  }
  @keyframes slide {
    from {
      left: -30%;
    }
    to {
      left: 100%;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .slider {
      animation: none;
      left: 0;
      opacity: 0.5;
    }
  }
</style>
