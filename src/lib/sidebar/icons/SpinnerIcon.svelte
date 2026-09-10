<!-- Agent working: a faint ring with a spinning arc. Drawn inline, no icon library. -->
<script lang="ts">
  let { size = 14 }: { size?: number } = $props();
</script>

<svg class="spinner" width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
  <circle cx="8" cy="8" r="6" stroke="currentColor" stroke-width="1.8" opacity="0.25" />
  <path d="M8 2a6 6 0 0 1 6 6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
</svg>

<style>
  .spinner {
    animation: spin 0.9s linear infinite;
  }
  /* Slower, not stopped: the motion is the signal. */
  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation-duration: 2.4s;
    }
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
