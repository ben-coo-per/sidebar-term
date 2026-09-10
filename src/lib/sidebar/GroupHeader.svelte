<!-- A Group header: chevron, name (inline rename), Tab count, drag handle, context menu. -->
<script lang="ts">
  import type { Group } from "../layout.svelte";
  import { deleteGroup, layout, newTab, renameGroup, toggleGroupCollapsed } from "../layout.svelte";
  import ChevronIcon from "./icons/ChevronIcon.svelte";
  import { dnd, startGroupDrag, endDrag, overGroupHeader, dropOnGroupHeader } from "./dnd.svelte";
  import { openContextMenu } from "./menu.svelte";
  import type { MenuItem } from "./ContextMenu.svelte";
  import { requestConfirm } from "./confirm.svelte";

  let { group }: { group: Group } = $props();

  let editing = $state(false);
  let draft = $state("");
  let inputEl: HTMLInputElement | undefined = $state();

  function beginRename() {
    draft = group.name;
    editing = true;
  }

  function startEdit(e: MouseEvent) {
    e.stopPropagation();
    beginRename();
  }

  function commit() {
    if (editing) renameGroup(group.id, draft);
    editing = false;
  }

  function cancel() {
    editing = false;
  }

  function onEditKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      commit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    }
  }

  $effect(() => {
    if (editing) inputEl?.focus();
  });

  async function handleDelete() {
    if (layout.groups.length <= 1) return;
    if (group.tabIds.length > 0) {
      const ok = await requestConfirm({
        message: `Delete "${group.name}"?`,
        detail: `This closes ${group.tabIds.length} tab${group.tabIds.length === 1 ? "" : "s"} in this group.`,
        confirmLabel: "Delete Group",
        danger: true,
      });
      if (!ok) return;
    }
    deleteGroup(group.id);
  }

  function menuItems(): MenuItem[] {
    return [
      { label: "Rename", action: beginRename },
      { label: "New Tab in Group", action: () => void newTab({ groupId: group.id }) },
      { label: group.collapsed ? "Expand" : "Collapse", action: () => toggleGroupCollapsed(group.id) },
      {
        label: "Delete Group",
        action: handleDelete,
        danger: true,
        disabled: layout.groups.length <= 1,
        separatorBefore: true,
      },
    ];
  }
</script>

<div
  class="header"
  class:drop-target={dnd.overGroupId === group.id}
  role="button"
  tabindex="0"
  draggable={!editing}
  ondragstart={(e) => startGroupDrag(e, group.id)}
  ondragend={endDrag}
  ondragover={(e) => overGroupHeader(e, group.id)}
  ondrop={(e) => dropOnGroupHeader(e, group.id)}
  onclick={() => toggleGroupCollapsed(group.id)}
  onkeydown={(e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      toggleGroupCollapsed(group.id);
    }
  }}
  oncontextmenu={(e) => openContextMenu(e, menuItems())}
>
  <span class="chevron" class:collapsed={group.collapsed}><ChevronIcon size={11} /></span>

  {#if editing}
    <input
      class="name-input"
      bind:value={draft}
      bind:this={inputEl}
      onkeydown={onEditKeydown}
      onblur={commit}
      onclick={(e) => e.stopPropagation()}
    />
  {:else}
    <span class="name" role="button" tabindex="-1" ondblclick={startEdit}>{group.name}</span>
  {/if}

  <span class="count">{group.tabIds.length}</span>
</div>

<style>
  .header {
    display: flex;
    align-items: center;
    gap: 5px;
    height: var(--group-header-height);
    padding: 0 8px;
    margin: 6px 4px 1px;
    border-radius: var(--radius-sm);
    cursor: default;
    user-select: none;
    color: var(--text-secondary);
    outline: none;
  }
  .header:hover {
    background: var(--sidebar-bg-raised);
  }
  .header.drop-target {
    outline: 1.5px dashed var(--accent);
    outline-offset: -1px;
  }
  .chevron {
    flex: none;
    display: flex;
    color: var(--text-tertiary);
    transition: transform var(--duration-fast) var(--ease-standard);
    transform: rotate(90deg);
  }
  .chevron.collapsed {
    transform: rotate(0deg);
  }
  .name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    color: var(--text-tertiary);
  }
  .header:hover .name {
    color: var(--text-secondary);
  }
  .name-input {
    flex: 1 1 auto;
    min-width: 0;
    background: var(--sidebar-bg);
    border: 1px solid var(--accent);
    border-radius: 4px;
    color: var(--text-primary);
    font: inherit;
    font-size: 11px;
    text-transform: none;
    padding: 1px 4px;
    outline: none;
  }
  .count {
    flex: none;
    font-size: 10px;
    color: var(--text-tertiary);
    min-width: 12px;
    text-align: right;
  }
</style>
