<!-- The Settings page's Hotkeys section: click a binding, press the new combo; Escape cancels.
     While one is being recorded every key press is taken here, before the app's Hotkeys and the
     page see it. See src/lib/hotkeys.ts for the rules. State: src/lib/hotkeys.svelte.ts. -->
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
  import CloseIcon from "../sidebar/icons/CloseIcon.svelte";

  const ACTION_CATEGORIES: ActionCategory[] = ["Tabs", "Groups", "Hosts", "App"];
  const byCategory = ACTION_CATEGORIES.map((category) => ({
    category,
    actions: ACTIONS.filter((a) => a.category === category),
  }));

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

  // WebKit doesn't focus buttons on click, so a click elsewhere won't blur: cancel here.
  function onWindowPointerdown(e: PointerEvent) {
    if (recordingId && !(e.target as Element).closest(".binding.recording")) stopRecording();
  }

  function bindingText(a: ActionDef): string {
    if (recordingId === a.id) return heldModifiers ? `${heldModifiers}…` : "Press keys…";
    return formatCombo(hotkeys.bindings[a.id]) || "Unassigned";
  }

  // Leaving the section mid-recording (another category, or the page closing) must hand the
  // keyboard back to the app's Hotkeys.
  $effect(() => stopRecording);
</script>

<svelte:window
  onkeydowncapture={onWindowKeydown}
  onkeyupcapture={onWindowKeyup}
  onpointerdowncapture={onWindowPointerdown}
/>

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

<style>
  .binding {
    appearance: none;
    min-width: 92px;
    height: var(--control-height);
    padding: 0 10px;
    border: 1px solid var(--sidebar-border);
    border-radius: var(--radius-sm);
    background: var(--sidebar-bg-raised);
    color: var(--text-primary);
    font: inherit;
    font-size: calc(12px * var(--ui-font-scale));
    letter-spacing: 0.04em;
    text-align: center;
    cursor: pointer;
  }
  .binding:hover {
    background: var(--sidebar-bg-active);
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
    font-size: calc(11.5px * var(--ui-font-scale));
    color: var(--text-secondary);
    padding-bottom: 2px;
  }
  .note.error {
    color: var(--danger);
  }
</style>
