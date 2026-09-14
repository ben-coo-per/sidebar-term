<!-- A frozen Tab (Memory Guard): a snowflake. Drawn inline, no icon library. -->
<script lang="ts">
  let { size = 14 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
  <g stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round">
    <path d="M8.00 1.50 8.00 14.50 M2.37 4.75 13.63 11.25 M2.37 11.25 13.63 4.75" />
    <path d="M9.70 3.80 8.00 2.50 6.30 3.80 M5.21 4.43 3.24 5.25 3.51 7.37 M3.51 8.63 3.24 10.75 5.21 11.57 M6.30 12.20 8.00 13.50 9.70 12.20 M10.79 11.57 12.76 10.75 12.49 8.63 M12.49 7.37 12.76 5.25 10.79 4.43" />
  </g>
</svg>
