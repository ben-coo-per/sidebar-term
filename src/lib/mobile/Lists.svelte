<!-- The phone's lists, once paired: the page's Host as the heading, the Hosts that do not
     answer, the Hosts whose Tabs wait behind a pairing, then Manager or the Tabs, chosen at the
     bottom. Manager's button counts the agents waiting on the user. -->
<script lang="ts">
  import ManagerScreen from "./ManagerScreen.svelte";
  import StatusBanner from "./StatusBanner.svelte";
  import TabList from "./TabList.svelte";
  import { homeHost, lanes, mobile, pairWith, setView, unpaired } from "./store.svelte";

  const home = $derived(homeHost());
  const waiting = $derived(lanes().waiting.length);
  const behind = $derived(unpaired());
</script>

<main class="lists">
  <header>
    <h1>{home?.info?.name ?? home?.name ?? "sidebar-term"}</h1>
    {#if mobile.device}
      <span class="device">{mobile.device}</span>
    {/if}
  </header>
  <StatusBanner />
  {#each behind as host (host.host)}
    <button type="button" class="behind" onclick={() => pairWith(host.host)}>
      <span class="what">
        <b>{host.name}</b>
        {host.tabs === 1 ? "has 1 Tab" : `has ${host.tabs} Tabs`} of yours. Pair this phone with it to see {host.tabs === 1 ? "it" : "them"}.
      </span>
      <span class="go">Pair</span>
    </button>
  {/each}

  <div class="body">
    {#if mobile.view === "manager"}
      <ManagerScreen />
    {:else}
      <TabList />
    {/if}
  </div>

  <nav>
    <button type="button" class:on={mobile.view === "manager"} aria-pressed={mobile.view === "manager"} onclick={() => setView("manager")}>
      Manager{#if waiting > 0}<span class="count" aria-label="{waiting} waiting on you">{waiting}</span>{/if}
    </button>
    <button type="button" class:on={mobile.view === "tabs"} aria-pressed={mobile.view === "tabs"} onclick={() => setView("tabs")}>
      Tabs
    </button>
  </nav>
</main>

<style>
  .lists {
    box-sizing: border-box;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding-top: env(safe-area-inset-top);
  }
  header {
    flex: none;
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    padding: 18px 20px 8px;
  }
  h1 {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 22px;
    font-weight: 700;
  }
  .device {
    flex: none;
    font-size: 13px;
    color: var(--text-tertiary);
  }
  .behind {
    flex: none;
    appearance: none;
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 8px 16px 0;
    padding: 10px 12px;
    border: 1px solid var(--sidebar-border);
    border-radius: 10px;
    background: var(--sidebar-bg-raised);
    color: var(--text-secondary);
    font: inherit;
    font-size: 13px;
    line-height: 1.4;
    text-align: left;
  }
  .behind b {
    color: var(--text-primary);
  }
  .what {
    flex: 1 1 auto;
    min-width: 0;
  }
  .go {
    flex: none;
    padding: 6px 12px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--text-on-accent);
    font-weight: 600;
  }
  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    -webkit-overflow-scrolling: touch;
  }
  nav {
    flex: none;
    display: flex;
    padding: 6px 12px calc(6px + env(safe-area-inset-bottom));
    gap: 8px;
    border-top: 1px solid var(--sidebar-border);
    background: var(--sidebar-bg);
  }
  nav button {
    appearance: none;
    flex: 1 1 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-height: 44px;
    border: none;
    border-radius: 10px;
    background: transparent;
    color: var(--text-tertiary);
    font: inherit;
    font-size: 15px;
    font-weight: 600;
  }
  nav button.on {
    background: var(--sidebar-bg-active);
    color: var(--text-primary);
  }
  .count {
    min-width: 20px;
    padding: 0 6px;
    box-sizing: border-box;
    border-radius: 10px;
    background: var(--status-needs-input);
    color: var(--term-bg);
    font-size: 12px;
    line-height: 20px;
    font-variant-numeric: tabular-nums;
    text-align: center;
  }
</style>
