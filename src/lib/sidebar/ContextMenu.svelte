<!-- Generic custom context menu (Tab: Rename, Move to Group ▸, New Group from Tab, Close;
     Group: Rename, New Tab in Group, Collapse, Delete). Supports one level of submenu. -->
<script lang="ts">
  import SelfContextMenu from "./ContextMenu.svelte";

  export interface MenuItem {
    label: string;
    action?: () => void;
    danger?: boolean;
    disabled?: boolean;
    separatorBefore?: boolean;
    submenu?: MenuItem[];
  }

  let {
    x = 0,
    y = 0,
    items,
    onClose,
    embedded = false,
  }: { x?: number; y?: number; items: MenuItem[]; onClose: () => void; embedded?: boolean } = $props();

  let menuEl: HTMLDivElement | undefined = $state();
  let openSubmenu = $state<number | null>(null);
  let clampedLeft = $state<number | null>(null);
  let clampedTop = $state<number | null>(null);

  function pick(item: MenuItem, index: number) {
    if (item.disabled) return;
    if (item.submenu) {
      openSubmenu = openSubmenu === index ? null : index;
      return;
    }
    item.action?.();
    onClose();
  }

  function onWindowMousedown(e: MouseEvent) {
    if (menuEl && !menuEl.contains(e.target as Node)) onClose();
  }
  function onWindowKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  }

  $effect(() => {
    if (embedded) return;
    window.addEventListener("mousedown", onWindowMousedown, true);
    window.addEventListener("keydown", onWindowKeydown, true);
    return () => {
      window.removeEventListener("mousedown", onWindowMousedown, true);
      window.removeEventListener("keydown", onWindowKeydown, true);
    };
  });

  $effect(() => {
    if (embedded || !menuEl) {
      clampedLeft = null;
      clampedTop = null;
      return;
    }
    const rect = menuEl.getBoundingClientRect();
    clampedLeft = rect.right > window.innerWidth - 8 ? Math.max(8, window.innerWidth - rect.width - 8) : null;
    clampedTop = rect.bottom > window.innerHeight - 8 ? Math.max(8, window.innerHeight - rect.height - 8) : null;
  });

  const left = $derived(clampedLeft ?? x);
  const top = $derived(clampedTop ?? y);
</script>

<div
  class="menu"
  class:embedded
  style={embedded ? "" : `left:${left}px; top:${top}px;`}
  bind:this={menuEl}
  role="menu"
  tabindex="-1"
>
  {#each items as item, i (item.label + i)}
    {#if item.separatorBefore}<div class="sep" role="separator"></div>{/if}
    <div class="item-wrap">
      <button
        type="button"
        class="item"
        class:danger={item.danger}
        disabled={item.disabled}
        role="menuitem"
        onclick={() => pick(item, i)}
        onmouseenter={() => {
          if (item.submenu) openSubmenu = i;
        }}
      >
        <span class="label">{item.label}</span>
        {#if item.submenu}<span class="arrow">▸</span>{/if}
      </button>
      {#if item.submenu && openSubmenu === i}
        <div class="submenu">
          <SelfContextMenu items={item.submenu} onClose={() => onClose()} embedded={true} />
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 200;
    min-width: 168px;
    background: var(--sidebar-bg-raised);
    border: 1px solid var(--sidebar-border);
    border-radius: var(--radius-md);
    box-shadow: 0 10px 28px #00000059;
    padding: 4px;
    display: flex;
    flex-direction: column;
    animation: pop var(--duration-fast) var(--ease-standard);
  }
  .menu.embedded {
    position: absolute;
    left: calc(100% + 4px);
    top: -5px;
  }
  .item-wrap {
    position: relative;
  }
  .item {
    appearance: none;
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font: inherit;
    font-size: 12px;
    text-align: left;
    padding: 6px 8px;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .item:hover:not(:disabled) {
    background: var(--sidebar-bg-active);
  }
  .item.danger {
    color: var(--danger);
  }
  .item:disabled,
  .item.danger:disabled {
    color: var(--text-tertiary);
    cursor: default;
  }
  .arrow {
    color: var(--text-tertiary);
    font-size: 10px;
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--sidebar-divider);
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateY(-2px);
    }
  }
</style>
