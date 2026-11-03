<!-- Connection state, shown for each Host the phone is paired with while it is not online:
     reconnecting, or why it went away; and while it is online but strained, why (its Sessions
     short of memory, naming the Tab using the most, or late answers). With `host`, that Host's
     only (the terminal screen). -->
<script lang="ts">
  import { LOCAL_HOST, type HostId } from "../host/ids";
  import { describeStrain, type Strain } from "../host/strain";
  import { tabRow } from "./rows";
  import { hostViews, mobile, type HostState } from "./store.svelte";

  let { host = null }: { host?: HostId | null } = $props();

  const away = $derived(
    Object.values(mobile.hosts).filter((h) => h.paired && h.status !== "online" && (host === null || h.id === host)),
  );

  const strained = $derived(
    Object.values(mobile.hosts)
      .filter((h) => h.paired && h.status === "online" && (host === null || h.id === host))
      .map((h) => ({ h, strain: strainOf(h) }))
      .filter((x): x is { h: HostState; strain: Strain } => x.strain !== null),
  );

  function strainOf(h: HostState): Strain | null {
    return describeStrain(h.pressure, h.slow, (sid) => {
      const view = hostViews()[h.id];
      const tab = h.layout && Object.values(h.layout.tabs).find((t) => t.sessionId === sid);
      return tab && view ? `“${tabRow(tab, view).title}”` : null;
    });
  }

  function nameOf(h: HostState): string {
    return h.info?.name ?? h.name ?? (h.id === LOCAL_HOST ? "the Host" : new URL(h.url).host);
  }

  /** What is still shown of a Host that does not answer. */
  function meanwhile(h: HostState): string {
    return host === null && h.id === LOCAL_HOST && h.layout ? " Showing what it last said." : "";
  }
</script>

{#each strained as { h, strain } (h.id)}
  <div class="banner strain" class:throttled={strain.level === "throttled"} role="status">
    {nameOf(h)}: {strain.text}
  </div>
{/each}
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
  .banner.strain {
    background: color-mix(in srgb, var(--status-needs-input) 18%, transparent);
  }
  .banner.strain.throttled {
    background: color-mix(in srgb, var(--status-needs-input) 32%, transparent);
  }
</style>
