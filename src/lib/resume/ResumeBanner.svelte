<!-- The Resume banner: shown at the bottom of the Terminal area after a launch when the last run
     closed with Tabs still running something. One row per Tab (click to go to it, or resume just
     that one), a button per kind for all of them, and a dismiss. See docs/architecture.md "Resume". -->
<script lang="ts">
  import { activateTab, layout } from "../layout.svelte";
  import { tabTitle } from "../sessions.svelte";
  import RobotIcon from "../sidebar/icons/RobotIcon.svelte";
  import TerminalIcon from "../sidebar/icons/TerminalIcon.svelte";
  import CloseIcon from "../sidebar/icons/CloseIcon.svelte";
  import type { ResumeEntry } from "../types";
  import { dismissResume, resume, resumeEntries } from "./resume.svelte";

  const claude = $derived(resume.entries.filter((e) => e.kind === "claude"));
  const commands = $derived(resume.entries.filter((e) => e.kind === "command"));
  const count = $derived(resume.entries.length);

  let working = $state(false);
  async function start(entries: ResumeEntry[]) {
    if (working) return;
    working = true;
    try {
      await resumeEntries(entries);
    } finally {
      working = false;
    }
  }
</script>

{#if count > 0}
  <aside class="banner" aria-label="Resume">
    <div class="head">
      <p class="message">
        {count === 1 ? "A Tab was" : `${count} Tabs were`} still running when sidebar-term last closed.
      </p>
      <div class="actions">
        {#if claude.length > 0}
          <button type="button" class="btn primary" disabled={working} onclick={() => start(claude)}>
            Resume Claude Code{claude.length > 1 ? ` (${claude.length})` : ""}
          </button>
        {/if}
        {#if commands.length > 0}
          <button
            type="button"
            class="btn"
            class:primary={claude.length === 0}
            disabled={working}
            onclick={() => start(commands)}
          >
            Rerun {commands.length > 1 ? `commands (${commands.length})` : "command"}
          </button>
        {/if}
        <button type="button" class="dismiss" title="Dismiss" aria-label="Dismiss" onclick={dismissResume}>
          <CloseIcon />
        </button>
      </div>
    </div>
    <ul class="rows">
      {#each resume.entries as entry (entry.key)}
        {@const tab = layout.tabs[entry.key]}
        {#if tab}
          <li class="row">
            <button type="button" class="goto" title="Go to this Tab" onclick={() => activateTab(entry.key)}>
              <span class="icon">
                {#if entry.kind === "claude"}<RobotIcon />{:else}<TerminalIcon />{/if}
              </span>
              <span class="title">{tabTitle(tab)}</span>
              <code class="line" title={entry.cwd ? `${entry.line}\nin ${entry.cwd}` : entry.line}>{entry.line}</code>
            </button>
            {#if resume.busy.includes(entry.key)}
              <span class="busy" title="This Tab is running something. Stop it, then try again.">Busy</span>
            {/if}
            <button type="button" class="row-action" disabled={working} onclick={() => start([entry])}>
              {entry.kind === "claude" ? "Resume" : "Rerun"}
            </button>
          </li>
        {/if}
      {/each}
    </ul>
  </aside>
{/if}

<style>
  .banner {
    flex: 0 0 auto;
    border-top: 1px solid var(--sidebar-border);
    background: var(--sidebar-bg);
    padding: 8px 10px 6px 12px;
    font-size: 12px;
    color: var(--text-primary);
    animation: rise var(--duration-medium) var(--ease-standard);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .message {
    flex: 1 1 auto;
    min-width: 0;
    margin: 0;
    font-weight: 600;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 0 0 auto;
  }
  .btn {
    appearance: none;
    border: 1px solid var(--sidebar-border);
    background: var(--sidebar-bg-active);
    color: var(--text-primary);
    font: inherit;
    padding: 4px 10px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    white-space: nowrap;
  }
  .btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--text-on-accent);
  }
  .btn:hover:not(:disabled) {
    filter: brightness(1.15);
  }
  .btn:disabled,
  .row-action:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .dismiss {
    appearance: none;
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    margin-left: 2px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-tertiary);
    cursor: pointer;
  }
  .dismiss:hover {
    background: var(--sidebar-bg-raised);
    color: var(--text-primary);
  }
  .rows {
    list-style: none;
    margin: 6px 0 0;
    padding: 0;
    /* About five rows, then scroll: the Terminal keeps most of the height. */
    max-height: calc(5 * 26px);
    overflow-y: auto;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 26px;
  }
  .goto {
    appearance: none;
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 100%;
    padding: 0 6px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .goto:hover {
    background: var(--sidebar-bg-raised);
  }
  .icon {
    display: grid;
    place-items: center;
    flex: 0 0 auto;
    color: var(--text-secondary);
  }
  /* A fixed column, so the command lines line up. */
  .title {
    flex: 0 0 140px;
    max-width: 30%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .line {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: ui-monospace, "SF Mono", Menlo, monospace;
    font-size: 11.5px;
    color: var(--text-secondary);
  }
  .busy {
    flex: 0 0 auto;
    color: var(--status-needs-input);
    font-size: 11px;
  }
  .row-action {
    appearance: none;
    flex: 0 0 auto;
    padding: 2px 8px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--accent-strong);
    font: inherit;
    cursor: pointer;
  }
  .row-action:hover:not(:disabled) {
    background: var(--accent-dim);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>
