<!-- Right-pointing chevron. Rotate the wrapper (e.g. via a `.collapsed` class) to flip state. -->
<script lang="ts">
  let { size = 12 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
  <path d="M6 3.5 10.5 8 6 12.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
</svg>
