<!-- Pairing: present a Host's code, name this phone, and get a token. The page's own Host
     first (its code comes prefilled when the page was opened from the pairing link: the QR
     code, or the link the daemon prints); then, from the list, each Host its linked Tabs point
     at. The code must come from the Host being paired with, which the screen names. Nothing
     says before pairing whether that Host is the Mac app or the daemon, so the screen shows
     both ways to a code. -->
<script lang="ts">
  import { hostNameFromUrl } from "../host/settings";
  import { mobile, pair, pairWith } from "./store.svelte";

  function defaultName(): string {
    const ua = navigator.userAgent;
    return /iPad/.test(ua) ? "iPad" : /iPhone/.test(ua) ? "iPhone" : /Android/.test(ua) ? "Android phone" : "Phone";
  }

  /** Another Host than the page's own, chosen from the list. */
  const other = $derived(mobile.pairWith ? (mobile.hosts[mobile.pairWith] ?? null) : null);
  /** The Host the code must come from: the one chosen, else the one the page came from. */
  const hostName = $derived(other ? (other.name ?? hostNameFromUrl(other.url)) : hostNameFromUrl(location.origin));

  /** The code came with the pairing link: how to get one is said only once it is refused. */
  const prefilled = !mobile.pairWith && mobile.pairCode !== "";
  const showHow = $derived(!prefilled || mobile.pairError !== null);

  let code = $state(mobile.pairWith ? "" : mobile.pairCode);
  let name = $state(mobile.device ?? defaultName());

  const ready = $derived(code.replace(/[^A-Za-z0-9]/g, "").length === 8 && !mobile.pairing);

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (ready) void pair(code, name);
  }
</script>

<main class="pair">
  <h1>Pair with {hostName}</h1>
  {#if other}
    <p class="how">
      Some of your Tabs run on <b>{hostName}</b>. This phone reaches {hostName} directly, so it needs a pairing code
      from {hostName}, once.
    </p>
  {:else if !showHow}
    <p class="how">The code from the link is filled in. Name this phone and press <b>Pair</b>.</p>
  {:else}
    <p class="how">This page comes from <b>{hostName}</b>, so the pairing code must come from {hostName}.</p>
  {/if}
  {#if showHow}
    <section class="way">
      <h2>If {hostName} is a Mac</h2>
      <p>
        In sidebar-term on {hostName}, open Settings, then Remote. Turn on Remote access and press
        <b>Pair a phone…</b>.
        {#if other}
          Type the code it shows here. Do not scan the QR code: it opens {hostName}'s own page.
        {:else}
          Scan the QR code it shows, or type the code here.
        {/if}
      </p>
    </section>
    <section class="way">
      <h2>If {hostName} runs the daemon</h2>
      <p>In a terminal on {hostName}, run these two commands:</p>
      <pre class="command">systemctl --user kill --kill-whom=main -s USR1 sidebar-termd
journalctl --user -u sidebar-termd -n 3</pre>
      <p>
        The second prints the daemon's log. The line with "pairing code" has the code and a link. Type the code here.
        {#if other}
          Do not open the link: it opens {hostName}'s own page.
        {/if}
      </p>
    </section>
    <p class="note">A code is 8 characters. It lasts ten minutes and five wrong tries. After that, start a new pairing.</p>
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
    /* The page itself does not scroll (Mobile.svelte): the screen does, when it is taller. */
    height: 100%;
    overflow-y: auto;
    -webkit-overflow-scrolling: touch;
    padding: calc(24px + env(safe-area-inset-top)) 24px calc(24px + env(safe-area-inset-bottom));
    max-width: 420px;
    margin: 0 auto;
  }
  h1 {
    margin: 32px 0 12px;
    font-size: 24px;
    font-weight: 700;
    overflow-wrap: anywhere;
  }
  .how {
    margin: 0 0 20px;
    font-size: 15px;
    line-height: 1.45;
    color: var(--text-secondary);
  }
  .way {
    margin: 0 0 12px;
    padding: 12px 14px;
    border: 1px solid var(--sidebar-border);
    border-radius: 10px;
    font-size: 14px;
    line-height: 1.45;
    color: var(--text-secondary);
  }
  .way h2 {
    margin: 0 0 6px;
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    overflow-wrap: anywhere;
  }
  .way p {
    margin: 0;
  }
  .command {
    margin: 8px 0;
    padding: 10px 12px;
    overflow-x: auto;
    border-radius: 10px;
    background: var(--sidebar-bg);
    font-family: "SF Mono", ui-monospace, Menlo, monospace;
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-primary);
  }
  .note {
    margin: 0 0 24px;
    font-size: 13px;
    line-height: 1.45;
    color: var(--text-tertiary);
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
