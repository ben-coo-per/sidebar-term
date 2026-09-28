<!-- The sidebar as the phone lists it: the Groups of the page's Host with each Tab's Agent
     status and Badge, as the rows show them on the Mac, derived here from the Hosts' layouts and
     Session facts (./rows.ts). A Tab whose Session runs on another Host carries a chip with that
     Host's name, and is greyed while the Host is not connected. Tap a Tab to drive its Session. -->
<script lang="ts">
  import Badge from "../sidebar/Badge.svelte";
  import RobotIcon from "../sidebar/icons/RobotIcon.svelte";
  import TerminalIcon from "../sidebar/icons/TerminalIcon.svelte";
  import SpinnerIcon from "../sidebar/icons/SpinnerIcon.svelte";
  import { AGENT_NAMES } from "../agentStatus";
  import { homeHost, openTab, rows, unpair } from "./store.svelte";
  import type { TabRow } from "./rows";

  const groups = $derived(rows());
  const home = $derived(homeHost());

  function stateLabel(tab: TabRow): string {
    if (tab.agent) {
      const what = tab.status === "running" ? "working" : tab.status === "needs-input" ? "needs input" : "idle";
      return `${AGENT_NAMES[tab.agent]}: ${what}`;
    }
    return "Terminal session";
  }

  function confirmUnpair() {
    // A plain confirm is fine on the phone: nothing else is running in this page.
    if (window.confirm("Forget this Host, and the Hosts its Tabs run on? You will need to pair again.")) unpair();
  }
</script>

<div class="list">
  {#if !home?.layout}
    <p class="empty">
      {home?.status === "online" ? "Waiting for the Host's layout…" : "The Host is not reachable right now."}
    </p>
  {:else}
    {#each groups as group (group.id)}
      <section>
        <h2>{group.name} <span class="count">({group.tabs.length})</span></h2>
        {#each group.tabs as tab (tab.id)}
          <button
            type="button"
            class="tab"
            class:active={home.layout.activeTabId === tab.id}
            class:away={!tab.reachable}
            disabled={tab.sessionId === null || !tab.reachable}
            onclick={() => openTab(tab)}
          >
            <span class="icon {tab.agent ? (tab.status ?? 'done') : ''}" aria-label={stateLabel(tab)}>
              {#if tab.agent && tab.status === "running"}
                <SpinnerIcon size={18} />
              {:else if tab.agent}
                <RobotIcon size={18} />
              {:else}
                <TerminalIcon size={18} />
              {/if}
            </span>
            <span class="text">
              <span class="title">{tab.title}</span>
              {#if tab.hostName || tab.git || tab.remote}
                <span class="badge-line">
                  {#if tab.hostName}<span class="chip">{tab.hostName}</span>{/if}
                  <Badge git={tab.git} remote={tab.remote} />
                </span>
              {/if}
            </span>
            <span class="chevron" aria-hidden="true">›</span>
          </button>
        {/each}
        {#if group.tabs.length === 0}
          <p class="none">No Tabs</p>
        {/if}
      </section>
    {/each}
  {/if}

  <footer>
    <button type="button" class="link" onclick={confirmUnpair}>Forget this Host</button>
  </footer>
</div>

<style>
  .list {
    padding-bottom: 16px;
  }
  .empty,
  .none {
    margin: 0;
    padding: 12px 20px;
    font-size: 14px;
    color: var(--text-tertiary);
  }
  .empty {
    padding-top: 40px;
    text-align: center;
  }
  section {
    margin-top: 14px;
  }
  h2 {
    margin: 0;
    padding: 6px 20px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-tertiary);
  }
  .count {
    font-weight: 400;
  }
  .tab {
    appearance: none;
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 56px;
    padding: 8px 16px 8px 20px;
    border: none;
    border-bottom: 1px solid var(--sidebar-divider);
    background: transparent;
    color: var(--text-primary);
    font: inherit;
    text-align: left;
  }
  .tab:active {
    background: var(--sidebar-bg-raised);
  }
  .tab:disabled {
    opacity: 0.4;
  }
  .tab.away .chip {
    border-style: dashed;
  }
  .tab.active .title {
    color: var(--accent-strong);
  }
  .icon {
    flex: none;
    display: flex;
    color: var(--text-tertiary);
  }
  .icon.running {
    color: var(--status-running);
  }
  .icon.needs-input {
    color: var(--status-needs-input);
  }
  .text {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 16px;
  }
  .badge-line {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    overflow: hidden;
    font-size: 12px;
  }
  .chip {
    flex: none;
    padding: 0 6px;
    border: 1px solid var(--sidebar-border);
    border-radius: 4px;
    font-size: 11px;
    line-height: 16px;
    color: var(--text-secondary);
    white-space: nowrap;
  }
  .chevron {
    flex: none;
    font-size: 22px;
    color: var(--text-tertiary);
  }
  footer {
    padding: 32px 20px 0;
    text-align: center;
  }
  .link {
    appearance: none;
    border: none;
    background: transparent;
    font: inherit;
    font-size: 13px;
    color: var(--text-tertiary);
    text-decoration: underline;
  }
</style>
