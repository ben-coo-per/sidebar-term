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
        Other machines running sidebar-term (a Mac with Remote on, or sidebar-termd): their Tabs join your Groups, tagged
        with the Host's name. Tabs made there from elsewhere land in a Group named after it. Removing a Host takes its
        Tabs out of the sidebar; they keep running there.
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
        <span class="controls">
          {#if h.paired && h.status === "offline"}
            <button type="button" class="text-btn small" onclick={() => reconnectHost(h.id)}>Retry</button>
          {/if}
          <button type="button" class="text-btn small danger" onclick={() => removeHost(h.id)}>Remove</button>
        </span>
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
  .add {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
    padding: 4px 0;
  }
  /* In a column: the shared basis would be its height. */
  .add .label {
    flex: none;
  }
  .fields {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .field.url {
    flex: 1 1 180px;
  }
  .field.code {
    flex: none;
    width: calc(108px * var(--ui-font-scale));
    font-family: var(--font-mono);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .map {
    flex: 1 0 100%;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    padding: 8px 0 4px 14px;
  }
  .map-hint {
    margin: -2px 0 2px 116px;
  }
  .add-override {
    margin-left: 110px;
  }
  .map-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .map-label {
    flex: 0 0 110px;
    font-size: calc(12px * var(--ui-font-scale));
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .map-label.mono,
  .field.path,
  .field.repo {
    font-family: var(--font-mono);
    font-size: calc(11.5px * var(--ui-font-scale));
  }
  .field.path {
    flex: 1 1 140px;
  }
  .field.repo {
    flex: 0 0 110px;
  }
  /* A pane too narrow for the label column: what hangs under the fields starts at the left. */
  @container settings-pane (max-width: 380px) {
    .map-hint,
    .add-override {
      margin-left: 0;
    }
  }
</style>
