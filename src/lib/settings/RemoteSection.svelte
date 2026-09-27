<!-- The Settings page's Remote section: the on/off switch, where Tailscale stands, pairing a
     phone (QR code + typed code), and the phones paired. State: src/lib/remote/remote.svelte.ts. -->
<script lang="ts">
  import QRCode from "qrcode";
  import { beginPairing, cancelPairing, refreshRemote, remote, revokeDevice, toggleRemote } from "../remote/remote.svelte";

  const snap = $derived(remote.snapshot);
  const on = $derived(snap?.on ?? false);
  const ts = $derived(snap?.tailscale ?? null);

  /** One line under the switch: where a phone can reach this Mac, or what is in the way. */
  const statusLine = $derived.by(() => {
    if (!snap) return "";
    if (!ts?.installed) return "Tailscale is not installed on this Mac. Install it from tailscale.com and sign in.";
    if (!ts.running) return ts.error ?? "Tailscale is not running or not signed in.";
    if (!on) return `Off. When on, phones on your tailnet reach it at https://${ts.dnsName}/m.`;
    if (snap.url) return `Phones open ${snap.url}`;
    return ts.error ?? "Tailscale Serve has not published the server yet.";
  });

  let qr = $state("");
  let now = $state(Date.now());

  // The QR code for the pairing link, and a ticking clock for the time it has left.
  $effect(() => {
    const url = snap?.pairing?.url ?? null;
    if (!url) {
      qr = "";
      return;
    }
    let live = true;
    void QRCode.toString(url, { type: "svg", margin: 1, errorCorrectionLevel: "M", color: { dark: "#e7e8ea", light: "#0000" } })
      .then((svg) => {
        if (live) qr = svg;
      })
      .catch(() => (qr = ""));
    const tick = setInterval(() => (now = Date.now()), 1000);
    return () => {
      live = false;
      clearInterval(tick);
    };
  });

  const timeLeft = $derived.by(() => {
    const at = snap?.pairing?.expiresAt;
    if (!at) return "";
    const s = Math.max(0, Math.round((at - now) / 1000));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  });

  function when(ms: number | null): string {
    if (ms === null) return "never";
    const d = new Date(ms);
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric" }) + " " + d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  }

  $effect(() => {
    void refreshRemote();
  });
</script>

<section>
  <div class="section-header">
    <div>
      <h2>Remote</h2>
      <p class="hint">Drive these Sessions from your phone, over Tailscale only. Nothing listens while off.</p>
    </div>
  </div>
  <ul class="rows">
    <li class="row">
      <label class="check">
        <span class="label">
          Remote access
          <span class="detail" class:error={!!snap?.error}>{snap?.error ?? statusLine}</span>
          {#if remote.error}
            <span class="detail error">{remote.error}</span>
          {/if}
        </span>
        <input type="checkbox" checked={on} disabled={remote.busy || !snap} onchange={(e) => void toggleRemote(e.currentTarget.checked)} />
      </label>
    </li>

    {#if on}
      <li class="row pairing">
        {#if snap?.pairing}
          <div class="pair-card">
            <div class="qr">{@html qr}</div>
            <div class="pair-text">
              <span class="code">{snap.pairing.code}</span>
              <span class="detail">
                On the phone, scan this or open the link and type the code. Expires in {timeLeft}.
              </span>
              {#if snap.pairing.url && !snap.url}
                <span class="detail warn">Tailscale is not publishing the server: this link only works from a browser on this Mac.</span>
              {/if}
              <button type="button" class="text-btn small" onclick={() => void cancelPairing()}>Cancel</button>
            </div>
          </div>
        {:else}
          <span class="label">
            Pair a phone
            <span class="detail">
              {snap?.clients ? `${snap.clients} connected now.` : "Shows a code the phone presents once; it is then let in until removed here."}
            </span>
          </span>
          <button type="button" class="text-btn" onclick={() => void beginPairing()}>Pair a phone…</button>
        {/if}
      </li>
      {#each snap?.devices ?? [] as device (device.id)}
        <li class="row">
          <span class="label">
            {device.name}
            <span class="detail">
              Paired {when(device.createdAt)} · last seen {when(device.lastSeenAt)}{device.login ? ` · ${device.login}` : ""}
            </span>
          </span>
          <button type="button" class="text-btn small danger" onclick={() => void revokeDevice(device.id)}>Remove</button>
        </li>
      {/each}
    {/if}
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
    gap: 12px;
    min-height: 34px;
    padding: 3px 0;
    border-bottom: 1px solid var(--sidebar-divider);
  }
  .label {
    flex: 1 1 auto;
    font-size: 13px;
  }
  .check {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 0;
    cursor: pointer;
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
  .detail.warn {
    color: var(--status-needs-input);
  }
  .check input {
    flex: none;
    width: 14px;
    height: 14px;
    margin: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .pair-card {
    display: flex;
    align-items: flex-start;
    gap: 16px;
    padding: 8px 0;
  }
  .qr {
    flex: none;
    width: 148px;
    height: 148px;
    padding: 6px;
    border-radius: var(--radius-md);
    background: var(--sidebar-bg-raised);
  }
  .qr :global(svg) {
    display: block;
    width: 100%;
    height: 100%;
  }
  .pair-text {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    min-width: 0;
  }
  .code {
    font-family: "SF Mono", ui-monospace, Menlo, monospace;
    font-size: 22px;
    letter-spacing: 0.1em;
    user-select: text;
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
  .text-btn:hover {
    background: var(--sidebar-bg-active);
    color: var(--text-primary);
  }
  .text-btn.danger:hover {
    color: var(--danger);
  }
</style>
