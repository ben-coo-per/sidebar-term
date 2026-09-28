<!-- A card for one agent waiting on the user. A hooked agent's question comes with its detail
     (the diff of an edit, the command to run) and its answers as buttons, the first the default.
     An agent without one (screen-only, or a question its hooks could not carry) is answered in
     its Terminal, which the card opens. -->
<script lang="ts">
  import { AGENT_NAMES } from "../agentStatus";
  import { formatDuration } from "../manager/model";
  import RobotIcon from "../sidebar/icons/RobotIcon.svelte";
  import type { Lane } from "./lanes";

  let {
    lane,
    now,
    onanswer,
    onopen,
  }: {
    lane: Lane;
    now: number;
    onanswer: (index: number) => void;
    onopen: () => void;
  } = $props();

  const pending = $derived(lane.pending);
  const reachable = $derived(lane.row.reachable);
</script>

<div class="card">
  <div class="head">
    <span class="robot"><RobotIcon size={16} /></span>
    <span class="name">
      <span class="project">{lane.project}</span>
      {#if lane.description}<span class="description">{lane.description}</span>{/if}
    </span>
    <span class="since">{formatDuration(now - lane.since)}</span>
  </div>
  <div class="meta">
    <span>{AGENT_NAMES[lane.agent]}</span>
    {#if lane.row.hostName}<span class="chip">{lane.row.hostName}</span>{/if}
    {#if !lane.hooked}<span class="chip dashed">screen only</span>{/if}
  </div>

  {#if pending}
    <p class="question">{pending.text}</p>
    {#if pending.detail.length}
      <div class="mono">
        {#each pending.detail as line, i (i)}<div class="line {line.tone}">{line.text || " "}</div>{/each}
      </div>
    {/if}
    <div class="answers">
      {#each pending.options as option, i (i)}
        <button type="button" class="btn" class:primary={i === 0} disabled={!reachable} onclick={() => onanswer(i)}>
          {option}
        </button>
      {/each}
    </div>
    <button type="button" class="link" disabled={!reachable} onclick={onopen}>Answer in its Terminal instead</button>
  {:else}
    <p class="question">It is waiting in its Terminal.</p>
    <div class="answers">
      <button type="button" class="btn primary" disabled={!reachable} onclick={onopen}>Open its Terminal</button>
    </div>
  {/if}
</div>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0 16px 10px;
    padding: 12px 14px;
    border: 1px solid var(--status-needs-input-border);
    border-radius: 12px;
    background: var(--sidebar-bg-raised);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .robot {
    flex: none;
    display: flex;
    color: var(--status-needs-input);
  }
  .name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 16px;
  }
  .project {
    font-weight: 600;
  }
  .description {
    margin-left: 6px;
    color: var(--text-tertiary);
  }
  .since {
    flex: none;
    font-size: 13px;
    color: var(--status-needs-input);
    font-variant-numeric: tabular-nums;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-tertiary);
  }
  .chip {
    padding: 0 6px;
    border: 1px solid var(--sidebar-border);
    border-radius: 4px;
    font-size: 11px;
    line-height: 16px;
    color: var(--text-secondary);
    white-space: nowrap;
  }
  .chip.dashed {
    border-style: dashed;
    color: var(--text-tertiary);
  }
  .question {
    margin: 2px 0 0;
    font-size: 15px;
    line-height: 1.4;
    overflow-wrap: anywhere;
  }
  .mono {
    max-height: 40vh;
    overflow: auto;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--term-bg);
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 17px;
    -webkit-overflow-scrolling: touch;
  }
  .line {
    width: max-content;
    min-width: 100%;
    color: var(--text-secondary);
    white-space: pre;
  }
  .line.remove {
    color: var(--diff-remove);
  }
  .line.add {
    color: var(--diff-add);
  }
  .answers {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 2px;
  }
  .btn {
    appearance: none;
    min-height: 44px;
    padding: 8px 14px;
    border: 1px solid var(--sidebar-border);
    border-radius: 10px;
    background: var(--sidebar-bg-active);
    color: var(--text-primary);
    font: inherit;
    font-size: 15px;
    line-height: 1.3;
    text-align: left;
    overflow-wrap: anywhere;
  }
  .btn.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--text-on-accent);
    font-weight: 600;
  }
  .btn:active {
    filter: brightness(1.15);
  }
  .link {
    appearance: none;
    align-self: flex-start;
    min-height: 32px;
    padding: 0;
    border: none;
    background: none;
    color: var(--accent-strong);
    font: inherit;
    font-size: 13px;
  }
  button:disabled {
    opacity: 0.4;
  }
</style>
