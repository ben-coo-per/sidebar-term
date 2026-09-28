<!-- Connection state, shown for each Host the phone is paired with while it is not online:
     reconnecting, or why it went away. With `host`, that Host's only (the terminal screen). -->
<script lang="ts">
  import { LOCAL_HOST, type HostId } from "../host/ids";
  import { mobile, type HostState } from "./store.svelte";

  let { host = null }: { host?: HostId | null } = $props();

  const away = $derived(
    Object.values(mobile.hosts).filter((h) => h.paired && h.status !== "online" && (host === null || h.id === host)),
  );

  function nameOf(h: HostState): string {
    return h.info?.name ?? h.name ?? (h.id === LOCAL_HOST ? "the Host" : new URL(h.url).host);
  }

  /** What is still shown of a Host that does not answer. */
  function meanwhile(h: HostState): string {
    return host === null && h.id === LOCAL_HOST && h.layout ? " Showing what it last said." : "";
  }
</script>

{#each away as h (h.id)}
  <div class="banner" class:connecting={h.status === "connecting"} role="status">
    {#if h.status === "connecting"}
      Connecting to {nameOf(h)}…
    {:else}
      {h.detail ?? `${nameOf(h)} is not reachable. Is Tailscale on, on both?`} Retrying…{meanwhile(h)}
    {/if}
  </div>
{/each}

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
