<!-- Agent session icon: a small robot face. Drawn inline, no icon library. -->
<script lang="ts">
  let { size = 14 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
  <path d="M8 1.5v2" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
  <circle cx="8" cy="1.5" r="0.9" fill="currentColor" />
  <rect x="2.25" y="4" width="11.5" height="9" rx="2.6" stroke="currentColor" stroke-width="1.2" />
  <circle cx="5.6" cy="8.4" r="1.05" fill="currentColor" />
  <circle cx="10.4" cy="8.4" r="1.05" fill="currentColor" />
  <path d="M5.6 11.1h4.8" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
  <path d="M0.9 7v3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
  <path d="M15.1 7v3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
</svg>
