<script lang="ts">
  let { size = 12 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
  <path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
</svg>
