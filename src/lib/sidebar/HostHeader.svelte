<!-- A Host section's header, above a paired Host's Groups: its name and a connection dot (green
     online, amber connecting, grey offline). The local Host has none. Click reconnects an
     offline Host; the context menu adds a Group or Tab on the Host, or reconnects. -->
<script lang="ts">
  import { newGroup, newTab } from "../layout.svelte";
  import { hostName, hostState, reconnectHost } from "../host/hosts.svelte";
  import type { HostId } from "../host/ids";
  import { openContextMenu } from "./menu.svelte";
  import type { MenuItem } from "./ContextMenu.svelte";
  import { openSettings } from "../settings/visibility.svelte";

  let { host }: { host: HostId } = $props();

  const state = $derived(hostState(host));
  const status = $derived(state?.status ?? "offline");
  const name = $derived(hostName(host));
  const statusText = $derived(
    !state
      ? ""
      : !state.paired
        ? "Not paired any more: remove it in Settings and pair again."
        : status === "online"
          ? "Connected"
          : status === "connecting"
            ? "Connecting…"
            : `${state.detail ?? "Not reachable"}. Retrying; click to try now.`,
  );
  const tooltip = $derived(state ? `${name} (${state.url})\n${statusText}` : name);

  function onClick() {
    if (status === "offline" && state?.paired) reconnectHost(host);
  }

  function menuItems(): MenuItem[] {
    return [
      { label: "New Tab", action: () => void newTab({ host }), disabled: status !== "online" },
      { label: "New Group", action: () => void newGroup(undefined, host), disabled: status !== "online" },
      { label: "Reconnect", action: () => reconnectHost(host), disabled: status === "online" || !state?.paired },
      { label: "Hosts in Settings…", action: openSettings, separatorBefore: true },
    ];
  }
</script>

<div
  class="host"
  role="button"
  tabindex="0"
  title={tooltip}
  onclick={onClick}
  onkeydown={(e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onClick();
    }
  }}
  oncontextmenu={(e) => openContextMenu(e, menuItems())}
>
  <span class="dot {status}" class:unpaired={state && !state.paired} aria-label={statusText}></span>
  <span class="name">{name}</span>
  {#if status !== "online"}
    <span class="state">{state && !state.paired ? "unpaired" : status}</span>
  {/if}
</div>

<style>
  .host {
    display: flex;
    align-items: center;
    gap: 7px;
    height: var(--group-header-height);
    padding: 0 8px 0 9px;
    margin: 10px 4px 0;
    border-top: 1px solid var(--sidebar-divider);
    padding-top: 8px;
    box-sizing: content-box;
    cursor: default;
    user-select: none;
    color: var(--text-primary);
    outline: none;
  }
  .host:hover {
    color: var(--text-primary);
  }
  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-tertiary);
    box-shadow: 0 0 0 2px var(--sidebar-bg);
  }
  .dot.online {
    background: var(--repo-color-1);
  }
  .dot.connecting {
    background: var(--status-needs-input);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .dot.unpaired {
    background: var(--danger);
  }
  .name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11.5px;
    font-weight: 700;
    letter-spacing: 0.01em;
  }
  .state {
    flex: none;
    font-size: 10.5px;
    color: var(--text-tertiary);
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
</style>
