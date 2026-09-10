<!-- The Settings page: shown over the Terminal (⌘, or the app menu's "Settings…"). Holds which
     agents the Panel's Usage view shows, and the Hotkeys: click a binding, press the new combo;
     Escape cancels. See src/lib/hotkeys.ts for the rules. -->
<script lang="ts">
  import {
    ACTIONS,
    comboError,
    comboFromEvent,
    combosEqual,
    formatCombo,
    isMac,
    isModifierOnly,
    type ActionCategory,
    type ActionDef,
    type ActionId,
  } from "../hotkeys";
  import { hotkeys, resetAllBindings, resetBinding, setBinding } from "../hotkeys.svelte";
  import { closeSettings } from "./visibility.svelte";
  import { USAGE_AGENTS } from "../panel/usage/model";
  import { setUsageAgent, usageSettings } from "../panel/usage/settings.svelte";
  import { AGENT_NAMES } from "../agentStatus";
  import type { AgentKind } from "../types";
  import CloseIcon from "../sidebar/icons/CloseIcon.svelte";

  const CATEGORIES: ActionCategory[] = ["Tabs", "Groups", "App"];
  const byCategory = CATEGORIES.map((category) => ({
    category,
    actions: ACTIONS.filter((a) => a.category === category),
  }));
  /** Where each agent's usage comes from, said next to its checkbox. */
  const USAGE_SOURCES: Record<AgentKind, string> = {
    claude: "Asks api.anthropic.com every minute, signed in as Claude Code (from your Keychain).",
    codex: "Read from Codex's session logs, which it updates on every turn.",
    gemini: "",
  };

  const labelOf = (id: ActionId) => ACTIONS.find((a) => a.id === id)?.label ?? id;

  /** The action whose binding is being recorded, if any. */
  let recordingId = $state<ActionId | null>(null);
  /** Modifiers held so far while recording, e.g. "⌘⇧". */
  let heldModifiers = $state("");
  /** One line under a row: why a combo was refused, or which action lost it. */
  let note = $state<{ id: ActionId; text: string; error: boolean } | null>(null);

  const anyCustom = $derived(ACTIONS.some((a) => !combosEqual(hotkeys.bindings[a.id], a.default)));

  function startRecording(id: ActionId) {
    recordingId = id;
    heldModifiers = "";
    note = null;
    hotkeys.recording = true;
  }

  function stopRecording() {
    recordingId = null;
    heldModifiers = "";
    hotkeys.recording = false;
  }

  function modifiersOf(e: KeyboardEvent): string {
    const c = comboFromEvent(e, isMac());
    return (c.ctrl ? "⌃" : "") + (c.alt ? "⌥" : "") + (c.shift ? "⇧" : "") + (c.meta ? "⌘" : "");
  }

  function assign(id: ActionId, set: () => ActionId | null) {
    const displaced = set();
    note = displaced
      ? { id, text: `Taken from “${labelOf(displaced)}”, which is now unassigned.`, error: false }
      : null;
  }

  function onWindowKeydown(e: KeyboardEvent) {
    if (!recordingId) return;
    e.preventDefault();
    e.stopPropagation();
    if (isModifierOnly(e)) {
      heldModifiers = modifiersOf(e);
      return;
    }
    if (e.key === "Escape" && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
      stopRecording();
      return;
    }
    const id = recordingId;
    const combo = comboFromEvent(e, isMac());
    const error = comboError(combo);
    if (error) {
      note = { id, text: error, error: true };
      return; // keep recording so the next try needs no extra click
    }
    stopRecording();
    assign(id, () => setBinding(id, combo));
  }

  function onWindowKeyup(e: KeyboardEvent) {
    if (recordingId) heldModifiers = modifiersOf(e);
  }

  function onPageKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !recordingId) {
      e.preventDefault();
      closeSettings();
    }
  }

  function bindingText(a: ActionDef): string {
    if (recordingId === a.id) return heldModifiers ? `${heldModifiers}…` : "Press keys…";
    return formatCombo(hotkeys.bindings[a.id]) || "Unassigned";
  }

  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  // Leaving the page mid-recording must hand the keyboard back to the app's Hotkeys.
  $effect(() => stopRecording);
</script>

<svelte:window onkeydowncapture={onWindowKeydown} onkeyupcapture={onWindowKeyup} />

<div
  class="page"
  role="dialog"
  aria-label="Settings"
  tabindex="-1"
  onkeydown={onPageKeydown}
  onpointerdown={(e) => {
    // WebKit doesn't focus buttons on click, so a click elsewhere won't blur: cancel here.
    if (recordingId && !(e.target as Element).closest(".binding.recording")) stopRecording();
  }}
  use:focusOnMount
>
  <div class="drag-region" data-tauri-drag-region></div>

  <div class="content">
    <header class="page-header">
      <h1>Settings</h1>
      <button type="button" class="icon-btn" onclick={closeSettings} title="Close (Esc)" aria-label="Close Settings">
        <CloseIcon size={12} />
      </button>
    </header>

    <section>
      <div class="section-header">
        <div>
          <h2>Usage</h2>
          <p class="hint">Agents whose usage limits the Panel's Usage view shows.</p>
        </div>
      </div>
      <ul class="rows">
        {#each USAGE_AGENTS as agent (agent)}
          <li class="row">
            <label class="check">
              <span class="label">
                {AGENT_NAMES[agent]}
                <span class="detail">{USAGE_SOURCES[agent]}</span>
              </span>
              <input
                type="checkbox"
                checked={usageSettings.agents.includes(agent)}
                onchange={(e) => setUsageAgent(agent, e.currentTarget.checked)}
              />
            </label>
          </li>
        {/each}
      </ul>
    </section>

    <section>
      <div class="section-header">
        <div>
          <h2>Hotkeys</h2>
          <p class="hint">Click a hotkey, then press the new combination. Esc cancels.</p>
        </div>
        <button type="button" class="text-btn" onclick={resetAllBindings} disabled={!anyCustom}>Restore Defaults</button>
      </div>

      {#each byCategory as { category, actions } (category)}
        <h3>{category}</h3>
        <ul class="rows">
          {#each actions as a (a.id)}
            {@const bound = hotkeys.bindings[a.id]}
            {@const custom = !combosEqual(bound, a.default)}
            <li class="row">
              <span class="label">{a.label}</span>
              <span class="controls">
                {#if custom}
                  <button
                    type="button"
                    class="text-btn small"
                    onclick={() => assign(a.id, () => resetBinding(a.id))}
                    title="Back to {formatCombo(a.default) || 'unassigned'}"
                  >
                    Reset
                  </button>
                {/if}
                {#if bound && recordingId !== a.id}
                  <button
                    type="button"
                    class="icon-btn small"
                    onclick={() => {
                      note = null;
                      setBinding(a.id, null);
                    }}
                    title="Unassign"
                    aria-label="Unassign {a.label}"
                  >
                    <CloseIcon size={9} />
                  </button>
                {/if}
                <button
                  type="button"
                  class="binding"
                  class:recording={recordingId === a.id}
                  class:unassigned={!bound && recordingId !== a.id}
                  class:custom
                  onclick={() => (recordingId === a.id ? stopRecording() : startRecording(a.id))}
                  aria-label="{a.label}: {bindingText(a)}"
                >
                  {bindingText(a)}
                </button>
              </span>
              {#if note?.id === a.id}
                <span class="note" class:error={note.error}>{note.text}</span>
              {/if}
            </li>
          {/each}
        </ul>
      {/each}
    </section>
  </div>
</div>

<style>
  .page {
    position: absolute;
    inset: 0;
    z-index: 50;
    display: flex;
    flex-direction: column;
    background: var(--term-bg);
    color: var(--text-primary);
    font-family:
      -apple-system,
      BlinkMacSystemFont,
      "SF Pro Text",
      sans-serif;
    outline: none;
    animation: fade var(--duration-fast) var(--ease-standard);
  }
  .drag-region {
    flex: none;
    height: var(--titlebar-inset);
    -webkit-app-region: drag;
  }
  .content {
    flex: 1 1 auto;
    overflow-y: auto;
    padding: 4px 32px 40px;
  }
  .content > * {
    max-width: 560px;
    margin-left: auto;
    margin-right: auto;
  }
  .content::-webkit-scrollbar {
    width: 8px;
  }
  .content::-webkit-scrollbar-thumb {
    background: var(--scrollbar-thumb);
    border-radius: 4px;
  }
  .page-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 18px;
  }
  h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
  }
  section + section {
    margin-top: 32px;
  }
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
  h3 {
    margin: 18px 0 4px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    color: var(--text-tertiary);
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
  }
  .check input {
    flex: none;
    width: 14px;
    height: 14px;
    margin: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .binding {
    appearance: none;
    min-width: 92px;
    font: inherit;
    font-size: 12px;
    letter-spacing: 0.04em;
    text-align: center;
    padding: 3px 10px;
    border: 1px solid var(--sidebar-border);
    border-radius: var(--radius-sm);
    background: var(--sidebar-bg-raised);
    color: var(--text-primary);
    cursor: pointer;
    outline: none;
  }
  .binding:hover {
    background: var(--sidebar-bg-active);
  }
  .binding:focus-visible {
    box-shadow: 0 0 0 2px var(--focus-ring);
  }
  .binding.custom {
    border-color: var(--accent-dim);
  }
  .binding.unassigned {
    color: var(--text-tertiary);
    letter-spacing: normal;
  }
  .binding.recording {
    border-color: var(--accent);
    background: var(--accent-dim);
    color: var(--accent-strong);
    letter-spacing: normal;
  }
  .note {
    flex-basis: 100%;
    text-align: right;
    font-size: 11.5px;
    color: var(--text-secondary);
    padding: 2px 0 3px;
  }
  .note.error {
    color: var(--danger);
  }
  .icon-btn {
    appearance: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-tertiary);
    cursor: pointer;
  }
  .icon-btn.small {
    width: 20px;
    height: 20px;
  }
  .icon-btn:hover {
    background: var(--sidebar-bg-raised);
    color: var(--text-primary);
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
  .text-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
