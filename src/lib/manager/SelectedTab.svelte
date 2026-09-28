<!-- The selected Tab, beside Needs you: a line on what it is (as its lane says it, or its Title
     once no agent runs), "Open Tab" and a way to close, then its Terminal, the same one Tabs mode
     shows, fitted to the space here. It takes the keys while it has the focus. -->
<script lang="ts">
  import { labelOf, type Lane } from "./state.svelte";
  import { formatDuration } from "./model";
  import { AGENT_NAMES } from "../agentStatus";
  import { tabSessionKey, type Tab } from "../layout.svelte";
  import { tabTitle } from "../sessions.svelte";
  import TerminalPane from "../terminal/TerminalPane.svelte";
  import RobotIcon from "../sidebar/icons/RobotIcon.svelte";
  import SpinnerIcon from "../sidebar/icons/SpinnerIcon.svelte";
  import CheckIcon from "../sidebar/icons/CheckIcon.svelte";
  import Place from "./Place.svelte";

  let {
    tab,
    lane,
    now,
    onopen,
    onclose,
  }: {
    tab: Tab;
    /** Its lane, while an agent runs in it. */
    lane: Lane | null;
    now: number;
    onopen: () => void;
    onclose: () => void;
  } = $props();

  const label = $derived(lane ?? labelOf(tab));
  const age = $derived(lane ? formatDuration(now - lane.since) : "");
  const status = $derived(
    !lane ? "" : lane.kind === "waiting" ? `waiting ${age}` : lane.kind === "working" ? `working ${age}` : `idle ${age} ago`,
  );
</script>

<div class="bar">
  {#if lane}
    <span class="icon {lane.kind}">
      {#if lane.kind === "working"}
        <SpinnerIcon size={14} />
      {:else if lane.kind === "waiting"}
        <RobotIcon size={14} />
      {:else}
        <CheckIcon size={14} />
      {/if}
    </span>
  {/if}
  <span class="title" title={tabTitle(tab)}>{label.project}</span>
  {#if label.description}<span class="description">{label.description}</span>{/if}
  {#if lane}
    <span class="status {lane.kind}">{AGENT_NAMES[lane.agent]} · {status}</span>
    {#if lane.host}<span class="chip host">{lane.host}</span>{/if}
    {#if !lane.hooked}<span class="chip screen">screen only</span>{/if}
    <Place git={lane.git} remote={lane.remote} />
  {/if}
  <span class="spacer"></span>
  <button type="button" class="open" tabindex="-1" onclick={onopen}>Open Tab<kbd class="hotkey">↵</kbd></button>
  <button type="button" class="close" tabindex="-1" aria-label="Close" title="Close (Esc)" onclick={onclose}>×</button>
</div>

<div class="terminal">
  <TerminalPane sessionKey={tabSessionKey(tab)} />
</div>

<style>
  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    height: 34px;
    padding: 0 8px 0 14px;
    border-bottom: 1px solid var(--sidebar-divider);
  }
  .icon {
    flex: none;
    display: flex;
    color: var(--text-tertiary);
  }
  .icon.working {
    color: var(--status-running);
  }
  .icon.waiting {
    color: var(--status-needs-input);
  }
  .title {
    flex: 0 1 auto;
    min-width: 3ch;
    font-size: calc(12.5px * var(--ui-font-scale));
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .description {
    flex: 0 3 auto;
    min-width: 0;
    font-size: calc(12.5px * var(--ui-font-scale));
    color: var(--text-tertiary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .status {
    flex: none;
    font-size: calc(11px * var(--ui-font-scale));
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    color: var(--text-tertiary);
  }
  .status.waiting {
    color: var(--status-needs-input);
  }
  .status.working {
    color: var(--text-secondary);
  }
  .chip {
    flex: none;
    font-size: calc(10px * var(--ui-font-scale));
    line-height: 14px;
    padding: 0 5px;
    border-radius: 4px;
    white-space: nowrap;
  }
  .chip.host {
    border: 1px solid var(--sidebar-border);
    background: var(--sidebar-bg-raised);
    color: var(--text-secondary);
  }
  .chip.screen {
    border: 1px dashed var(--scrollbar-thumb);
    color: var(--text-tertiary);
  }
  .spacer {
    flex: 1 1 auto;
  }
  .open {
    appearance: none;
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 6px;
    border: none;
    background: none;
    font: inherit;
    font-size: calc(11.5px * var(--ui-font-scale));
    color: var(--accent-strong);
    white-space: nowrap;
    cursor: default;
  }
  .hotkey {
    font-family: inherit;
    font-size: calc(10.5px * var(--ui-font-scale));
    line-height: 16px;
    padding: 0 5px;
    border: 1px solid var(--sidebar-border);
    border-radius: 4px;
    background: var(--sidebar-bg-raised);
    color: var(--text-secondary);
  }
  .close {
    appearance: none;
    flex: none;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    font: inherit;
    font-size: calc(16px * var(--ui-font-scale));
    line-height: 1;
    color: var(--text-tertiary);
    cursor: default;
  }
  .open:hover,
  .close:hover {
    background: var(--sidebar-bg-raised);
  }
  .close:hover {
    color: var(--text-primary);
  }
  .terminal {
    flex: 1 1 auto;
    min-height: 0;
    min-width: 0;
  }
</style>
