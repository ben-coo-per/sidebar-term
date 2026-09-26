<!-- The Settings page's Hosts section: the Hosts this Mac is paired with (each with its
     connection state and a Remove button), and pairing a new one by its URL and the code its
     Settings page shows (or `sidebar-termd --pair` prints). State: src/lib/host/hosts.svelte.ts. -->
<script lang="ts">
  import { addHost, hostName, hosts, reconnectHost, removeHost, type HostState } from "../host/hosts.svelte";
  import { codeFromPairingLink } from "../host/settings";

  let url = $state("");
  let code = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  const ready = $derived(url.trim() !== "" && code.replace(/[^A-Za-z0-9]/g, "").length === 8 && !busy);

  /** What the address field last put in the code field, so typing the link keeps updating it. */
  let autoCode = "";

  /** A pasted (or typed) pairing link (`…/m#pair=CODE`) fills the code too, unless the user typed one. */
  function onUrlInput() {
    const fromLink = codeFromPairingLink(url);
    if (fromLink && (code.trim() === "" || code === autoCode)) {
      code = fromLink;
      autoCode = fromLink;
    }
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!ready) return;
    busy = true;
    error = null;
    try {
      await addHost(url, code);
      url = "";
      code = "";
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      busy = false;
    }
  }

  function stateLine(h: HostState): string {
    if (!h.paired) return "The Host no longer accepts this Mac's token. Remove it and pair again.";
    switch (h.status) {
      case "online":
        return `Connected${h.info ? ` · sidebar-term ${h.info.version}` : ""}${h.device ? ` · known there as “${h.device}”` : ""}`;
      case "connecting":
        return "Connecting…";
      default:
        return `${h.detail ?? "Not reachable"}. Retrying.`;
    }
  }
</script>

<section>
  <div class="section-header">
    <div>
      <h2>Hosts</h2>
      <p class="hint">
        Other machines running sidebar-term (a Mac with Remote on, or sidebar-termd): their Tabs join the sidebar under
        the Host's name.
      </p>
    </div>
  </div>
  <ul class="rows">
    {#each hosts.list as h (h.id)}
      <li class="row">
        <span class="label">
          <span class="name">
            <span class="dot {h.status}" class:unpaired={!h.paired}></span>
            {hostName(h.id)}
          </span>
          <span class="detail" class:error={!h.paired}>{h.url} · {stateLine(h)}</span>
        </span>
        {#if h.paired && h.status === "offline"}
          <button type="button" class="text-btn small" onclick={() => reconnectHost(h.id)}>Retry</button>
        {/if}
        <button type="button" class="text-btn small danger" onclick={() => removeHost(h.id)}>Remove</button>
      </li>
    {/each}
    {#if hosts.ready && hosts.list.length === 0}
      <li class="row">
        <span class="label"><span class="detail">No Hosts paired yet.</span></span>
      </li>
    {/if}
    <li class="row">
      <form class="add" onsubmit={submit}>
        <span class="label">
          Add a Host
          <span class="detail">
            On the Host, turn on Remote and start a pairing (Settings there, or <code>sidebar-termd --pair</code>); enter
            its address and the code it shows.
          </span>
        </span>
        <div class="fields">
          <input
            class="field url"
            bind:value={url}
            oninput={onUrlInput}
            placeholder="https://dell.tail1234.ts.net"
            autocomplete="off"
            autocorrect="off"
            spellcheck="false"
            aria-label="Host address"
          />
          <input
            class="field code"
            bind:value={code}
            placeholder="ABCD EFGH"
            autocomplete="one-time-code"
            autocorrect="off"
            spellcheck="false"
            maxlength="9"
            aria-label="Pairing code"
          />
          <button type="submit" class="text-btn" disabled={!ready}>{busy ? "Pairing…" : "Pair"}</button>
        </div>
        {#if error}
          <span class="detail error">{error}</span>
        {/if}
      </form>
    </li>
  </ul>
</section>

<style>
  /* Matches SettingsPage.svelte's rows; scoped there, so repeated here. */
  .section-header {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 12px;
    padding-bottom: 10px;
    border-bottom: 1px solid var(--sidebar-divider);
  }
  h2 {
    margin: 0 0 3px;
    font-size: 14px;
    font-weight: 600;
  }
  .hint {
    margin: 0;
    font-size: 12px;
    color: var(--text-secondary);
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    min-height: 34px;
    padding: 5px 0;
    border-bottom: 1px solid var(--sidebar-divider);
  }
  .label {
    flex: 1 1 auto;
    min-width: 0;
    font-size: 13px;
  }
  .name {
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }
  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-tertiary);
  }
  .dot.online {
    background: var(--repo-color-1);
  }
  .dot.connecting {
    background: var(--status-needs-input);
  }
  .dot.unpaired {
    background: var(--danger);
  }
  .detail {
    display: block;
    margin-top: 1px;
    font-size: 11.5px;
    color: var(--text-tertiary);
    word-break: break-word;
  }
  .detail.error {
    color: var(--danger);
  }
  code {
    font-family: "SF Mono", ui-monospace, Menlo, monospace;
    font-size: 11px;
  }
  .add {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px 0;
  }
  .fields {
    display: flex;
    gap: 6px;
  }
  .field {
    font: inherit;
    font-size: 12px;
    padding: 4px 8px;
    border: 1px solid var(--sidebar-border);
    border-radius: var(--radius-sm);
    background: var(--sidebar-bg);
    color: var(--text-primary);
    outline: none;
    min-width: 0;
  }
  .field:focus {
    border-color: var(--accent);
  }
  .field.url {
    flex: 1 1 auto;
  }
  .field.code {
    flex: none;
    width: 108px;
    font-family: "SF Mono", ui-monospace, Menlo, monospace;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .text-btn {
    appearance: none;
    border: 1px solid var(--sidebar-border);
    background: var(--sidebar-bg-raised);
    color: var(--text-secondary);
    font: inherit;
    font-size: 12px;
    padding: 4px 10px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    white-space: nowrap;
  }
  .text-btn.small {
    font-size: 11px;
    padding: 2px 7px;
    border-color: transparent;
    background: transparent;
    color: var(--text-tertiary);
  }
  .text-btn:hover:not(:disabled) {
    background: var(--sidebar-bg-active);
    color: var(--text-primary);
  }
  .text-btn.danger:hover {
    color: var(--danger);
  }
  .text-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>
