<!-- Caffeinate icon: a coffee cup, full and steaming while on. Drawn inline, no icon library. -->
<script lang="ts">
  let { size = 14, steaming = false }: { size?: number; steaming?: boolean } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
  {#if steaming}
    <path d="M5.5 1.2c-.7.8.7 1.7 0 2.6M8.5 1.2c-.7.8.7 1.7 0 2.6" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" />
  {/if}
  <path
    d="M2.5 6h9v3.5a3.5 3.5 0 0 1-3.5 3.5H6a3.5 3.5 0 0 1-3.5-3.5z"
    stroke="currentColor"
    stroke-width="1.2"
    stroke-linejoin="round"
    fill={steaming ? "currentColor" : "none"}
    fill-opacity="0.3"
  />
  <path d="M11.5 7.25h.9a1.6 1.6 0 0 1 0 3.2h-1.1" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
</svg>
