<!-- Manager on the phone: every Agent session across the Hosts the phone reaches. First the
     agents waiting on the user, the oldest question first, each a card answered in place when
     its agent is hooked; then the agents at work; then those back at their prompt, with what
     their last turn changed. Tap one to open its Terminal. See CONTEXT.md "Manager". -->
<script lang="ts">
  import AskCard from "./AskCard.svelte";
  import LaneItem from "./LaneItem.svelte";
  import { answer, homeHost, lanes, mobile, openTab } from "./store.svelte";

  const all = $derived(lanes());
  const home = $derived(homeHost());
  const none = $derived(all.waiting.length + all.working.length + all.idle.length + mobile.sent.length === 0);
</script>

<div class="manager">
  {#if !home?.layout}
    <p class="empty">
      {home?.status === "online" ? "Waiting for the Host's layout…" : "The Host is not reachable right now."}
    </p>
  {:else if none}
    <p class="empty">No agent is running in any Tab.</p>
  {:else}
    <section>
      <h2>Needs you <span class="count">({all.waiting.length})</span></h2>
      {#each mobile.sent as sent (`${sent.rowId}:${sent.pendingId}`)}
        <p class="sent"><b>{sent.title}</b> Sent “{sent.choice}”</p>
      {/each}
      {#each all.waiting as lane (lane.row.id)}
        <AskCard {lane} now={mobile.now} onanswer={(i) => void answer(lane, i)} onopen={() => openTab(lane.row)} />
      {:else}
        {#if mobile.sent.length === 0}<p class="nothing">Nothing is waiting on you.</p>{/if}
      {/each}
    </section>

    {#if all.working.length}
      <section>
        <h2>Working <span class="count">({all.working.length})</span></h2>
        {#each all.working as lane (lane.row.id)}
          <LaneItem {lane} now={mobile.now} onopen={() => openTab(lane.row)} />
        {/each}
      </section>
    {/if}

    {#if all.idle.length}
      <section>
        <h2>Idle <span class="count">({all.idle.length})</span></h2>
        {#each all.idle as lane (lane.row.id)}
          <LaneItem {lane} now={mobile.now} onopen={() => openTab(lane.row)} />
        {/each}
      </section>
    {/if}
  {/if}
</div>

<style>
  .manager {
    padding-bottom: 16px;
  }
  .empty,
  .nothing {
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
    padding: 6px 20px 8px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-tertiary);
  }
  .count {
    font-weight: 400;
  }
  .sent {
    margin: 0 16px 10px;
    padding: 10px 14px;
    border: 1px solid var(--sidebar-border);
    border-radius: 12px;
    font-size: 14px;
    color: var(--text-secondary);
    overflow-wrap: anywhere;
  }
  .sent b {
    margin-right: 6px;
    color: var(--text-primary);
    font-weight: 600;
  }
</style>
