<!-- Remote marker: shown instead of the repo/branch Badge for a remote (ssh/mosh/docker/...) Session. -->
<script lang="ts">
  let { size = 11 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
  <circle cx="8" cy="8" r="5.7" stroke="currentColor" stroke-width="1.2" />
  <path d="M2.3 8h11.4M8 2.3c1.6 1.6 2.4 3.6 2.4 5.7s-.8 4.1-2.4 5.7c-1.6-1.6-2.4-3.6-2.4-5.7S6.4 3.9 8 2.3Z" stroke="currentColor" stroke-width="1.1" />
</svg>
