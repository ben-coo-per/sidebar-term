<!-- Connection state, shown while not online: reconnecting, or why the Host went away. -->
<script lang="ts">
  import { mobile } from "./store.svelte";
</script>

{#if mobile.status !== "online"}
  <div class="banner" class:connecting={mobile.status === "connecting"} role="status">
    {#if mobile.status === "connecting"}
      Connecting to the Host…
    {:else}
      {mobile.statusDetail ?? "The Host is not reachable. Is Tailscale on, on both?"} Retrying…
    {/if}
  </div>
{/if}

<style>
  .banner {
    padding: 8px 20px;
    font-size: 13px;
    background: var(--danger-dim);
    color: var(--text-primary);
  }
  .banner.connecting {
    background: var(--accent-dim);
  }
</style>
