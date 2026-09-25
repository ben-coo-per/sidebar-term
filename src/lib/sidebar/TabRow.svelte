<!-- A Tab row: Agent/plain icon, Agent status (or a snowflake while Memory Guard has it frozen),
     Title (inline rename; bold while unread), Badge, the Session's CPU and memory (Settings), close button. -->
<script lang="ts">
  import type { Tab } from "../layout.svelte";
  import { activateTab, layout, moveTab, newGroupFromTab, renameTab } from "../layout.svelte";
  import { sessionState, setTabRead, tabIsUnread, tabTitle } from "../sessions.svelte";
  import { AGENT_NAMES } from "../agentStatus";
  import RobotIcon from "./icons/RobotIcon.svelte";
  import TerminalIcon from "./icons/TerminalIcon.svelte";
  import CheckIcon from "./icons/CheckIcon.svelte";
  import SpinnerIcon from "./icons/SpinnerIcon.svelte";
  import CloseIcon from "./icons/CloseIcon.svelte";
  import SnowflakeIcon from "./icons/SnowflakeIcon.svelte";
  import { activity } from "../panel/activity/activity.svelte";
  import { activitySettings } from "../panel/activity/settings.svelte";
  import { formatBytes, formatTabStats } from "../panel/activity/model";
  import { freezeSession, frozenSession, thawSession } from "../guard/memoryGuard.svelte";
  import { frozenTitle } from "../guard/model";
  import Badge from "./Badge.svelte";
  import { dnd, startTabDrag, endDrag, overTabRow, dropOnTabRow } from "./dnd.svelte";
  import { openContextMenu } from "./menu.svelte";
  import type { MenuItem } from "./ContextMenu.svelte";
  import { requestCloseTab } from "./closeTabFlow";

  let { tab }: { tab: Tab } = $props();

  const session = $derived(sessionState(tab.sessionId));
  const agent = $derived(session?.info?.agent ?? null);
  const status = $derived(session?.status ?? null);
  const finished = $derived(session?.finished ?? false);
  const highlight = $derived(session?.highlight ?? false);
  const remote = $derived(session?.info?.remote ?? false);
  const git = $derived(session?.info?.git ?? null);
  const title = $derived(tabTitle(tab));
  const isActive = $derived(layout.activeTabId === tab.id);
  const frozen = $derived(frozenSession(tab.sessionId));
  const stateLabel = $derived(
    frozen
      ? "frozen"
      : status === "running"
        ? "working"
        : status === "needs-input"
          ? "needs input"
          : agent
            ? "idle"
            : undefined,
  );
  const usage = $derived(
    activitySettings.tabStats ? activity.snapshot?.sessions.find((s) => s.sessionId === tab.sessionId) : undefined,
  );
  const stats = $derived(
    frozen ? `frozen · ${formatBytes(usage?.mem ?? frozen.mem)}` : usage ? formatTabStats(usage.cpu, usage.mem) : null,
  );
  const rowTitle = $derived(
    frozen
      ? frozenTitle(frozen, formatBytes(usage?.mem ?? frozen.mem))
      : agent
        ? `${AGENT_NAMES[agent]}: ${stateLabel}`
        : finished
          ? "Agent finished"
          : "Terminal session",
  );

  let editing = $state(false);
  let draft = $state("");
  let inputEl: HTMLInputElement | undefined = $state();

  function beginRename() {
    draft = tab.customTitle ?? "";
    editing = true;
  }

  function startEdit(e: MouseEvent) {
    e.stopPropagation();
    beginRename();
  }

  function commit() {
    if (editing) renameTab(tab.id, draft);
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

  function onRowClick() {
    if (!editing) activateTab(tab.id);
  }

  function onRowKeydown(e: KeyboardEvent) {
    if (editing) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      activateTab(tab.id);
    }
  }

  /** Freeze (not the Tab in view: going to a Tab thaws it) or Thaw. */
  function freezeItem(): MenuItem {
    const sessionId = tab.sessionId;
    if (sessionId === null) return { label: "Freeze", disabled: true };
    if (frozen) return { label: "Thaw", action: () => void thawSession(sessionId) };
    return { label: "Freeze", action: () => void freezeSession(sessionId), disabled: isActive };
  }

  function menuItems(): MenuItem[] {
    const otherGroups = layout.groups.filter((g) => g.id !== tab.groupId);
    return [
      { label: "Rename", action: beginRename },
      tabIsUnread(tab)
        ? { label: "Mark as Read", action: () => setTabRead(tab, true) }
        : { label: "Mark as Unread", action: () => setTabRead(tab, false) },
      {
        label: "Move to Group",
        submenu: otherGroups.length
          ? otherGroups.map((g) => ({ label: g.name, action: () => moveTab(tab.id, g.id) }))
          : [{ label: "No other Groups", disabled: true }],
      },
      { label: "New Group from Tab", action: () => void newGroupFromTab(tab.id) },
      freezeItem(),
      { label: "Close", action: () => void requestCloseTab(tab.id), danger: true, separatorBefore: true },
    ];
  }

  const dropIndicatorClass = $derived(
    dnd.overTabId === tab.id ? (dnd.overPosition === "before" ? "drop-before" : "drop-after") : "",
  );
</script>

<div
  class="row {dropIndicatorClass}"
  class:active={isActive}
  class:dragging={dnd.draggingTabId === tab.id}
  class:highlight={highlight || finished}
  class:unread={tab.unread}
  role="button"
  tabindex="0"
  draggable="true"
  title={rowTitle}
  ondragstart={(e) => startTabDrag(e, tab.id)}
  ondragend={endDrag}
  ondragover={(e) => overTabRow(e, tab.id)}
  ondrop={(e) => dropOnTabRow(e, tab.groupId, tab.id)}
  onclick={onRowClick}
  onkeydown={onRowKeydown}
  oncontextmenu={(e) => openContextMenu(e, menuItems())}
>
  <!-- The icon slot carries Agent status: spinning while working, a still robot once stopped. -->
  <span class="icon {frozen ? 'frozen' : agent ? (status ?? 'done') : finished ? 'finished' : ''}" aria-label={stateLabel}>
    {#if frozen}
      <SnowflakeIcon size={14} />
    {:else if agent && status === "running"}
      <SpinnerIcon size={14} />
    {:else if agent}
      <RobotIcon size={14} />
    {:else if finished}
      <CheckIcon size={14} />
    {:else}
      <TerminalIcon size={14} />
    {/if}
  </span>

  <span class="text">
    {#if editing}
      <input
        class="title-input"
        bind:value={draft}
        bind:this={inputEl}
        onkeydown={onEditKeydown}
        onblur={commit}
        onclick={(e) => e.stopPropagation()}
      />
    {:else}
      <span class="title" role="button" tabindex="-1" ondblclick={startEdit}>{title}</span>
    {/if}
    {#if git || remote}
      <span class="badge-line">
        <Badge {git} {remote} />
      </span>
    {/if}
  </span>

  {#if stats}
    <span class="stats" class:frozen>{stats}</span>
  {/if}

  <button
    type="button"
    class="close"
    tabindex="-1"
    onclick={(e) => {
      e.stopPropagation();
      void requestCloseTab(tab.id);
    }}
    title="Close Tab"
  >
    <CloseIcon size={11} />
  </button>
</div>

<style>
  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: var(--row-height);
    padding: 4px 8px 4px 20px;
    border-radius: var(--radius-sm);
    margin: 0 4px;
    cursor: default;
    user-select: none;
    color: var(--text-secondary);
    outline: none;
  }
  .row:hover {
    background: var(--sidebar-bg-raised);
  }
  .row.active {
    background: var(--sidebar-bg-active);
    color: var(--text-primary);
  }
  .row.dragging {
    opacity: 0.4;
  }
  /* A user's unread mark shows on the Tab in view too, so marking it is visible straight away. */
  .row.highlight:not(.active) .title,
  .row.unread .title {
    color: var(--text-primary);
    font-weight: 600;
  }
  .row.drop-before::before,
  .row.drop-after::after {
    content: "";
    position: absolute;
    left: 6px;
    right: 6px;
    height: 2px;
    background: var(--accent);
    border-radius: 1px;
  }
  .row.drop-before::before {
    top: -1px;
  }
  .row.drop-after::after {
    bottom: -1px;
  }
  .icon {
    flex: none;
    display: flex;
    color: var(--text-tertiary);
  }
  .row.active .icon {
    color: var(--text-secondary);
  }
  /* Working is the only moving state; a stopped agent is as quiet as a plain session.
     `.row` prefix: these must beat `.row.active .icon` above. */
  .row .icon.running {
    color: var(--status-running);
  }
  .row .icon.needs-input {
    color: var(--status-needs-input);
  }
  .row .icon.finished {
    color: var(--status-finished);
  }
  .row .icon.frozen {
    color: var(--status-frozen);
  }
  .title {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
  }
  .title-input {
    flex: 1 1 auto;
    min-width: 0;
    background: var(--sidebar-bg);
    border: 1px solid var(--accent);
    border-radius: 4px;
    color: var(--text-primary);
    font: inherit;
    font-size: 12.5px;
    padding: 1px 4px;
    outline: none;
  }
  .text {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .badge-line {
    display: flex;
    min-width: 0;
    overflow: hidden;
  }
  .close {
    flex: none;
    appearance: none;
    border: none;
    background: transparent;
    color: var(--text-tertiary);
    display: none;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: 4px;
    cursor: pointer;
  }
  .row:hover .close {
    display: flex;
  }
  .stats {
    flex: none;
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
    color: var(--text-tertiary);
    white-space: nowrap;
  }
  .stats.frozen {
    color: var(--status-frozen);
  }
  /* The close button takes the stats' place on hover. */
  .row:hover .stats {
    display: none;
  }
  .close:hover {
    background: var(--sidebar-bg-active);
    color: var(--text-primary);
  }
</style>
