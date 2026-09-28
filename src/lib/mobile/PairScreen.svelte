<!-- Pairing: present a Host's code, name this phone, and get a token. The page's own Host
     first (its code comes prefilled when the page was opened from the QR code); then, from the
     list, each Host its linked Tabs point at. -->
<script lang="ts">
  import { mobile, pair, pairWith } from "./store.svelte";

  function defaultName(): string {
    const ua = navigator.userAgent;
    return /iPad/.test(ua) ? "iPad" : /iPhone/.test(ua) ? "iPhone" : /Android/.test(ua) ? "Android phone" : "Phone";
  }

  /** Another Host than the page's own, chosen from the list. */
  const other = $derived(mobile.pairWith ? (mobile.hosts[mobile.pairWith] ?? null) : null);
  const otherName = $derived(other ? (other.name ?? new URL(other.url).host) : "");

  let code = $state(mobile.pairWith ? "" : mobile.pairCode);
  let name = $state(mobile.device ?? defaultName());

  const ready = $derived(code.replace(/[^A-Za-z0-9]/g, "").length === 8 && !mobile.pairing);

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (ready) void pair(code, name);
  }
</script>

<main class="pair">
  {#if other}
    <h1>Pair with {otherName}</h1>
    <p class="how">
      Some of your Tabs run on <b>{otherName}</b>. This phone reaches it directly, so it needs a pairing code from it,
      once. Where {otherName} is a Mac: Settings, Remote, <b>Pair a phone</b>. Where it runs the daemon:
    </p>
    <pre class="command">systemctl --user kill -s USR1 sidebar-termd
journalctl --user -u sidebar-termd -n 3</pre>
  {:else}
    <h1>Pair with your Mac</h1>
    <p class="how">
      In sidebar-term on the Mac, open Settings, turn on Remote and press <b>Pair a phone</b>. Scan the code it shows,
      or type it here.
    </p>
  {/if}
  <form onsubmit={submit}>
    <label>
      <span>Code</span>
      <input
        class="code"
        bind:value={code}
        autocapitalize="characters"
        autocomplete="one-time-code"
        autocorrect="off"
        spellcheck="false"
        placeholder="ABCD EFGH"
        inputmode="text"
      />
    </label>
    <label>
      <span>This phone's name</span>
      <input bind:value={name} autocomplete="off" maxlength="60" />
    </label>
    <button type="submit" disabled={!ready}>{mobile.pairing ? "Pairing…" : "Pair"}</button>
    {#if other}
      <button type="button" class="later" onclick={() => pairWith(null)}>Not now</button>
    {/if}
    {#if mobile.pairError}
      <p class="error">{mobile.pairError}</p>
    {/if}
  </form>
</main>

<style>
  .pair {
    box-sizing: border-box;
    min-height: 100%;
    padding: calc(24px + env(safe-area-inset-top)) 24px calc(24px + env(safe-area-inset-bottom));
    max-width: 420px;
    margin: 0 auto;
  }
  h1 {
    margin: 32px 0 12px;
    font-size: 24px;
    font-weight: 700;
  }
  .how {
    margin: 0 0 28px;
    font-size: 15px;
    line-height: 1.45;
    color: var(--text-secondary);
  }
  .command {
    margin: -16px 0 28px;
    padding: 10px 12px;
    overflow-x: auto;
    border-radius: 10px;
    background: var(--sidebar-bg);
    font-family: "SF Mono", ui-monospace, Menlo, monospace;
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-secondary);
  }
  form {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
    color: var(--text-secondary);
  }
  input {
    box-sizing: border-box;
    width: 100%;
    font: inherit;
    font-size: 17px;
    padding: 12px 14px;
    border: 1px solid var(--sidebar-border);
    border-radius: 10px;
    background: var(--sidebar-bg);
    color: var(--text-primary);
    outline: none;
  }
  input:focus {
    border-color: var(--accent);
  }
  .code {
    font-family: "SF Mono", ui-monospace, Menlo, monospace;
    font-size: 22px;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }
  button {
    margin-top: 8px;
    font: inherit;
    font-size: 17px;
    font-weight: 600;
    padding: 14px;
    border: none;
    border-radius: 10px;
    background: var(--accent);
    color: var(--text-on-accent);
  }
  .later {
    margin-top: 0;
    background: transparent;
    font-weight: 400;
    color: var(--text-secondary);
  }
  button:disabled {
    opacity: 0.4;
  }
  .error {
    margin: 0;
    font-size: 14px;
    color: var(--danger);
  }
</style>
