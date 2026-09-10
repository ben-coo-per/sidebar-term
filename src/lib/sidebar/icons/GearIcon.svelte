<script lang="ts">
  let { size = 12 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
  <circle cx="8" cy="8" r="2.1" stroke="currentColor" stroke-width="1.3" />
  <path
    d="M8 1.6v1.6M8 12.8v1.6M1.6 8h1.6M12.8 8h1.6M3.5 3.5l1.1 1.1M11.4 11.4l1.1 1.1M3.5 12.5l1.1-1.1M11.4 4.6l1.1-1.1"
    stroke="currentColor"
    stroke-width="1.3"
    stroke-linecap="round"
  />
</svg>
