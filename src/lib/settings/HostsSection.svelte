<!-- The Settings page's Hosts section: the Hosts this Mac is paired with (each with its
     connection state, its repo-to-path map for Handoff and a Remove button), and pairing a new
     one by its URL and the code the Host shows (its Settings page, or the daemon's journal).
     State: src/lib/host/hosts.svelte.ts. -->
<script lang="ts">
  import {
    addHost,
    hostName,
    hosts,
    reconnectHost,
    removeHost,
    setHostCheckoutRoot,
    setHostRepoPath,
    type HostState,
  } from "../host/hosts.svelte";
  import { codeFromPairingLink } from "../host/settings";

  let url = $state("");
  let code = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  /** A new override being typed per Host: repo name and path. */
  let newRepo = $state<Record<string, { repo: string; path: string }>>({});
  /** Hosts whose "add an override" fields are open. */
  let addingOverride = $state<Record<string, boolean>>({});

  function draft(id: string): { repo: string; path: string } {
    return newRepo[id] ?? { repo: "", path: "" };
  }

  function setDraft(id: string, field: "repo" | "path", value: string) {
    newRepo[id] = { ...draft(id), [field]: value };
  }

  function draftReady(id: string): boolean {
    const d = draft(id);
    return d.repo.trim() !== "" && d.path.trim() !== "";
  }

  function addOverride(id: string) {
    if (!draftReady(id)) return;
    const d = draft(id);
    setHostRepoPath(id, d.repo, d.path);
    newRepo[id] = { repo: "", path: "" };
    addingOverride[id] = false;
  }

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
        <!-- Handoff's repo-to-path map: where this Host keeps its checkouts. -->
        <div class="map">
          <label class="map-row">
            <span class="map-label">Checkout root</span>
            <input
              class="field path"
              value={h.checkoutRoot ?? ""}
              onchange={(e) => setHostCheckoutRoot(h.id, e.currentTarget.value)}
              placeholder="Not set: New Tab on Host opens in its home"
              autocomplete="off"
              autocorrect="off"
              spellcheck="false"
              aria-label="Checkout root on {hostName(h.id)}"
            />
          </label>
          <span class="detail map-hint">
            Where this Host keeps its clones, e.g. <code>~/repos</code>: a Tab in repo <code>x</code> lands in
            <code>&lt;root&gt;/x</code> there.
          </span>
          {#each Object.entries(h.repoPaths) as [repo, path] (repo)}
            <div class="map-row">
              <span class="map-label mono">{repo}</span>
              <input
                class="field path"
                value={path}
                onchange={(e) => setHostRepoPath(h.id, repo, e.currentTarget.value)}
                autocomplete="off"
                autocorrect="off"
                spellcheck="false"
                aria-label="Checkout of {repo} on {hostName(h.id)}"
              />
              <button type="button" class="text-btn small danger" onclick={() => setHostRepoPath(h.id, repo, "")}>Remove</button>
            </div>
          {/each}
          {#if addingOverride[h.id]}
            <form
              class="map-row"
              onsubmit={(e) => {
                e.preventDefault();
                addOverride(h.id);
              }}
            >
              <input
                class="field repo"
                value={draft(h.id).repo}
                oninput={(e) => setDraft(h.id, "repo", e.currentTarget.value)}
                placeholder="repo"
                autocomplete="off"
                autocorrect="off"
                spellcheck="false"
                aria-label="Repo to override on {hostName(h.id)}"
              />
              <input
                class="field path"
                value={draft(h.id).path}
                oninput={(e) => setDraft(h.id, "path", e.currentTarget.value)}
                placeholder="its checkout on this Host"
                autocomplete="off"
                autocorrect="off"
                spellcheck="false"
                aria-label="Its checkout on {hostName(h.id)}"
              />
              <button type="submit" class="text-btn small" disabled={!draftReady(h.id)}>Add</button>
              <button type="button" class="text-btn small" onclick={() => (addingOverride[h.id] = false)}>Cancel</button>
            </form>
          {:else}
            <div class="map-row">
              <button type="button" class="text-btn small add-override" onclick={() => (addingOverride[h.id] = true)}>
                A repo lives somewhere else…
              </button>
            </div>
          {/if}
        </div>
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
            Start a pairing on the Host, then enter its address and the code it shows. On a Mac: Settings › Remote.
            On <code>sidebar-termd</code>: <code>systemctl --user kill --kill-whom=main -s USR1 sidebar-termd</code>, then
            read the code in <code>journalctl --user -u sidebar-termd</code>.
          </span>
        </span>
        <div class="fields">
          <input
            class="field url"
            bind:value={url}
            oninput={onUrlInput}
            placeholder="https://<host>.<tailnet>.ts.net"
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
  .field::placeholder {
    color: var(--text-tertiary);
    opacity: 0.7;
    letter-spacing: normal;
    text-transform: none;
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
  .map {
    flex: 1 0 100%;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 4px 0 4px 14px;
  }
  .map-hint {
    margin: -2px 0 2px 116px;
  }
  .add-override {
    margin-left: 110px;
  }
  .map-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .map-label {
    flex: 0 0 110px;
    font-size: 12px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .map-label.mono,
  .field.path,
  .field.repo {
    font-family: "SF Mono", ui-monospace, Menlo, monospace;
    font-size: 11.5px;
  }
  .field.path {
    flex: 1 1 auto;
  }
  .field.repo {
    flex: 0 0 110px;
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
