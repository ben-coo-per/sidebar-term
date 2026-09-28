<!-- A card for one agent waiting on the user. A hooked agent's question comes with its detail
     (the diff of an edit, the command to run) and its answers as buttons, 1-9, the first the
     default. An agent without one (screen-only, or a question its hooks could not carry) shows
     the last lines of its screen and one way on: open its Tab to answer there. The oldest card
     has the focus ring: its keys answer. -->
<script lang="ts">
  import type { Lane } from "./state.svelte";
  import { formatDuration, lastLines } from "./model";
  import { AGENT_NAMES } from "../agentStatus";
  import { terminals } from "../terminal/manager";
  import { tabSessionKey } from "../layout.svelte";
  import RobotIcon from "../sidebar/icons/RobotIcon.svelte";

  /** Lines of screen a card without a question shows. */
  const SCREEN_LINES = 3;
  /** How often those lines are read again. */
  const SCREEN_MS = 1000;

  let {
    lane,
    now,
    focused,
    onanswer,
    onopen,
  }: {
    lane: Lane;
    now: number;
    focused: boolean;
    onanswer: (index: number) => void;
    onopen: () => void;
  } = $props();

  const pending = $derived(lane.pending);
  let screen = $state<string[]>([]);

  $effect(() => {
    if (pending) return;
    const key = tabSessionKey(lane.tab);
    if (!key) return;
    const read = () => (screen = lastLines(terminals.screenLines(key), SCREEN_LINES));
    read();
    const timer = setInterval(read, SCREEN_MS);
    return () => clearInterval(timer);
  });
</script>

<div class="card" class:focused>
  <div class="head">
    <span class="robot"><RobotIcon size={14} /></span>
    <span class="title" title={lane.title}>{lane.project}</span>
    {#if lane.description}<span class="description">{lane.description}</span>{/if}
    <span class="agent">{AGENT_NAMES[lane.agent]}</span>
    {#if !lane.hooked}<span class="chip">screen only</span>{/if}
    <span class="spacer"></span>
    <span class="since">{formatDuration(now - lane.since)}</span>
  </div>

  {#if pending}
    <span class="question">{pending.text}</span>
    {#if pending.detail.length}
      <div class="mono">
        {#each pending.detail as line, i (i)}<div class="line {line.tone}">{line.text || " "}</div>{/each}
      </div>
    {/if}
    <div class="actions">
      {#each pending.options as option, i (i)}
        <button type="button" class="btn" class:primary={i === 0} onclick={() => onanswer(i)}>
          <span class="n">{i + 1}</span>{option}
        </button>
      {/each}
      <span class="spacer"></span>
      <button type="button" class="link" onclick={onopen}>Open Tab</button>
    </div>
  {:else}
    <div class="mono">
      {#each screen as line, i (i)}<div class="line">{line}</div>{:else}<div class="line"> </div>{/each}
    </div>
    <div class="actions">
      <button type="button" class="btn primary" onclick={onopen}>Open Tab to answer<span class="n">↵</span></button>
      <span class="note">{lane.hooked ? "Answer in its Tab" : "No hooks: answer in its Tab"}</span>
    </div>
  {/if}
</div>

<style>
  .card {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 11px 14px;
    background: var(--sidebar-bg-raised);
    border: 1px solid var(--status-needs-input-border);
    border-radius: var(--radius-md);
  }
  .card.focused {
    border-color: var(--sidebar-border);
    box-shadow: 0 0 0 1.5px var(--focus-ring);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
  }
  .robot {
    flex: none;
    display: flex;
    color: var(--status-needs-input);
  }
  .title {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 600;
  }
  .description {
    flex: 0 2 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    color: var(--text-tertiary);
  }
  .agent {
    flex: none;
    font-size: 11px;
    color: var(--text-tertiary);
    white-space: nowrap;
  }
  .chip {
    flex: none;
    font-size: 10px;
    line-height: 14px;
    padding: 0 5px;
    border-radius: 4px;
    border: 1px dashed var(--scrollbar-thumb);
    color: var(--text-tertiary);
    white-space: nowrap;
  }
  .spacer {
    flex: 1 1 auto;
  }
  .since {
    flex: none;
    font-size: 10.5px;
    color: var(--status-needs-input);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .question {
    font-size: 13px;
    line-height: 1.4;
    color: var(--text-primary);
    text-wrap: pretty;
  }
  .mono {
    background: var(--term-bg);
    border-radius: var(--radius-sm);
    padding: 6px 8px;
    font-family: "SF Mono", ui-monospace, Menlo, monospace;
    font-size: 11.5px;
    line-height: 16px;
  }
  .line {
    color: var(--text-secondary);
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .line.remove {
    color: var(--diff-remove);
  }
  .line.add {
    color: var(--diff-add);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 5px;
  }
  .btn {
    appearance: none;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--sidebar-border);
    border-radius: var(--radius-sm);
    background: var(--sidebar-bg-active);
    color: var(--text-primary);
    font: inherit;
    font-size: 11.5px;
    white-space: nowrap;
    cursor: default;
  }
  .btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--text-on-accent);
  }
  .btn:hover {
    filter: brightness(1.15);
  }
  .n {
    opacity: 0.7;
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
  }
  .link {
    appearance: none;
    border: none;
    background: none;
    padding: 0;
    height: 24px;
    font: inherit;
    font-size: 11.5px;
    color: var(--accent-strong);
    white-space: nowrap;
    cursor: default;
  }
  .link:hover {
    text-decoration: underline;
  }
  .note {
    font-size: 11px;
    color: var(--text-tertiary);
    white-space: nowrap;
  }
</style>
