<!-- An answer given from a card, in the card's place: "resuming" until its agent leaves Needs
     input, then it fades. -->
<script lang="ts">
  import type { Sent } from "./state.svelte";
  import CheckIcon from "../sidebar/icons/CheckIcon.svelte";
  import SpinnerIcon from "../sidebar/icons/SpinnerIcon.svelte";

  let { sent }: { sent: Sent } = $props();
</script>

<div class="sent" class:fading={sent.fading}>
  <span class="sent-icon"><CheckIcon size={14} /></span>
  <span class="sent-text">Sent <span class="choice">“{sent.choice}”</span> to {sent.title}</span>
  {#if !sent.resumed}<span class="resuming"><SpinnerIcon size={14} />resuming</span>{/if}
</div>

<style>
  .sent {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 12px;
    border: 1px dashed var(--scrollbar-thumb);
    border-radius: var(--radius-md);
    font-size: 12px;
    color: var(--text-secondary);
    transition: opacity var(--duration-medium) var(--ease-standard);
  }
  .sent.fading {
    opacity: 0;
  }
  .sent-icon {
    display: flex;
  }
  .sent-text {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .choice {
    color: var(--text-primary);
  }
  .resuming {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--status-running);
  }
</style>
