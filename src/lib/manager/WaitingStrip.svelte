<!-- In Tabs mode, at the top of the sidebar's Groups while any agent waits on the user: how many,
     and the Hotkey to Manager, where they can be answered. A click opens Manager. -->
<script lang="ts">
  import { setMode } from "../layout.svelte";
  import { hotkeyLabel } from "../hotkeys.svelte";
  import RobotIcon from "../sidebar/icons/RobotIcon.svelte";
  import { waiting } from "./state.svelte";

  const count = $derived(waiting().length);
  const key = $derived(hotkeyLabel("view.manager"));
</script>

{#if count > 0}
  <button type="button" class="strip" onclick={() => setMode("manager")} title="Open Manager">
    <span class="robot"><RobotIcon size={14} /></span>
    <span class="text">{count} {count === 1 ? "agent needs" : "agents need"} you</span>
    {#if key}<kbd class="hotkey">{key}</kbd>{/if}
  </button>
{/if}

<style>
  .strip {
    appearance: none;
    display: flex;
    align-items: center;
    gap: 7px;
    width: calc(100% - 16px);
    margin: 6px 8px 0;
    padding: 6px 8px;
    border: 1px solid var(--status-needs-input-border);
    border-radius: var(--radius-sm);
    background: var(--status-needs-input-dim);
    color: var(--text-primary);
    font: inherit;
    text-align: left;
    cursor: default;
  }
  .strip:hover {
    filter: brightness(1.15);
  }
  .robot {
    flex: none;
    display: flex;
    color: var(--status-needs-input);
  }
  .text {
    flex: 1 1 auto;
    min-width: 0;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hotkey {
    flex: none;
    font-family: inherit;
    font-size: 10.5px;
    line-height: 16px;
    padding: 0 5px;
    border: 1px solid var(--sidebar-border);
    border-radius: 4px;
    background: var(--sidebar-bg-raised);
    color: var(--text-secondary);
    letter-spacing: 0.04em;
  }
</style>
