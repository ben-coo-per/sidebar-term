<!-- Memory Guard icon: a gauge, its needle low while off and in the red while on. Drawn inline. -->
<script lang="ts">
  let { size = 14, on = false }: { size?: number; on?: boolean } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
  <path d="M2.2 11.5a6 6 0 1 1 11.6 0" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
  <path d="M11.2 5.2 12.6 4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" opacity="0.6" />
  {#if on}
    <path d="M8 10.5 11 6.6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
  {:else}
    <path d="M8 10.5 5 7.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
  {/if}
  <circle cx="8" cy="10.5" r="1.3" fill="currentColor" />
</svg>
