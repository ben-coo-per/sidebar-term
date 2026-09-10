<!-- App-wide in-app confirm dialog. Mounted once in +page.svelte. Never window.confirm/alert. -->
<script lang="ts">
  import { confirmDialog, answerConfirm } from "./confirm.svelte";

  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      answerConfirm(false);
    } else if (e.key === "Enter") {
      e.preventDefault();
      answerConfirm(true);
    }
  }
</script>

{#if confirmDialog.current}
  {@const req = confirmDialog.current}
  <div class="scrim" role="presentation" onmousedown={() => answerConfirm(false)}>
    <div
      class="dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="confirm-message"
      tabindex="-1"
      onmousedown={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
      use:focusOnMount
    >
      <p id="confirm-message" class="message">{req.message}</p>
      {#if req.detail}
        <p class="detail">{req.detail}</p>
      {/if}
      <div class="actions">
        <button type="button" class="btn" onclick={() => answerConfirm(false)}>Cancel</button>
        <button
          type="button"
          class="btn primary"
          class:danger={req.danger}
          onclick={() => answerConfirm(true)}
        >
          {req.confirmLabel ?? "Confirm"}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: #00000066;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
    animation: fade var(--duration-fast) var(--ease-standard);
  }
  .dialog {
    width: 320px;
    background: var(--sidebar-bg-raised);
    border: 1px solid var(--sidebar-border);
    border-radius: var(--radius-md);
    padding: 16px;
    box-shadow: 0 12px 32px #00000059;
    outline: none;
    animation: pop var(--duration-medium) var(--ease-standard);
  }
  .message {
    margin: 0 0 4px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }
  .detail {
    margin: 0 0 14px;
    font-size: 12px;
    color: var(--text-secondary);
    line-height: 1.4;
  }
  .message:last-of-type {
    margin-bottom: 14px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .btn {
    appearance: none;
    border: 1px solid var(--sidebar-border);
    background: var(--sidebar-bg-active);
    color: var(--text-primary);
    font: inherit;
    font-size: 12px;
    padding: 6px 12px;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .btn:hover {
    filter: brightness(1.15);
  }
  .btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--text-on-accent);
  }
  .btn.primary.danger {
    background: var(--danger);
    border-color: var(--danger);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: scale(0.96);
    }
  }
</style>
