<!-- The sidebar as the phone lists it: Groups of Tabs with the Agent status and Badge each row
     shows on the Mac. Tap a Tab to drive its Session. -->
<script lang="ts">
  import Badge from "../sidebar/Badge.svelte";
  import RobotIcon from "../sidebar/icons/RobotIcon.svelte";
  import TerminalIcon from "../sidebar/icons/TerminalIcon.svelte";
  import CheckIcon from "../sidebar/icons/CheckIcon.svelte";
  import SpinnerIcon from "../sidebar/icons/SpinnerIcon.svelte";
  import { AGENT_NAMES } from "../agentStatus";
  import StatusBanner from "./StatusBanner.svelte";
  import { mobile, openTab, unpair } from "./store.svelte";
  import type { SidebarTab } from "./protocol";

  function stateLabel(tab: SidebarTab): string {
    if (tab.agent) {
      const what = tab.status === "running" ? "working" : tab.status === "needs-input" ? "needs input" : "idle";
      return `${AGENT_NAMES[tab.agent]}: ${what}`;
    }
    return tab.finished ? "Agent finished" : "Terminal session";
  }

  function confirmUnpair() {
    // A plain confirm is fine on the phone: nothing else is running in this page.
    if (window.confirm("Forget this Mac? You will need to pair again.")) unpair();
  }
</script>

<main class="list">
  <header>
    <h1>sidebar-term</h1>
    {#if mobile.device}
      <span class="device">{mobile.device}</span>
    {/if}
  </header>
  <StatusBanner />

  {#if !mobile.sidebar}
    <p class="empty">
      {mobile.status === "online" ? "Waiting for the Mac's sidebar…" : "The Mac is not reachable right now."}
    </p>
  {:else}
    {#each mobile.sidebar.groups as group (group.id)}
      <section>
        <h2>{group.name} <span class="count">({group.tabs.length})</span></h2>
        {#each group.tabs as tab (tab.id)}
          <button
            type="button"
            class="tab"
            class:active={mobile.sidebar.activeTabId === tab.id}
            disabled={tab.sessionId === null}
            onclick={() => openTab(tab)}
          >
            <span class="icon {tab.agent ? (tab.status ?? 'done') : tab.finished ? 'finished' : ''}" aria-label={stateLabel(tab)}>
              {#if tab.agent && tab.status === "running"}
                <SpinnerIcon size={18} />
              {:else if tab.agent}
                <RobotIcon size={18} />
              {:else if tab.finished}
                <CheckIcon size={18} />
              {:else}
                <TerminalIcon size={18} />
              {/if}
            </span>
            <span class="text">
              <span class="title">{tab.title}</span>
              {#if tab.git || tab.remote}
                <span class="badge-line"><Badge git={tab.git} remote={tab.remote} /></span>
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
    <button type="button" class="link" onclick={confirmUnpair}>Forget this Mac</button>
  </footer>
</main>

<style>
  .list {
    box-sizing: border-box;
    height: 100%;
    overflow-y: auto;
    padding: env(safe-area-inset-top) 0 calc(16px + env(safe-area-inset-bottom));
    -webkit-overflow-scrolling: touch;
  }
  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    padding: 18px 20px 8px;
  }
  h1 {
    margin: 0;
    font-size: 22px;
    font-weight: 700;
  }
  .device {
    font-size: 13px;
    color: var(--text-tertiary);
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
  .icon.finished {
    color: var(--status-finished);
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
    min-width: 0;
    overflow: hidden;
    font-size: 12px;
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
