<!-- A Tab row, local or linked: Agent/plain icon, Agent status (or a snowflake while Memory Guard
     has it frozen), Title (inline rename; bold while unread; an agent's reads as its project, then
     a few muted words on what it is at), Badge, the Session's CPU and memory
     (Settings), close button. Everything shown comes from the Tab's Host's SessionInfo, so a
     linked Tab reads exactly as a local one, plus a chip with its Host's name by the Badge
     (greyed, like the row, while that Host is not connected). -->
<script lang="ts">
  import type { Tab } from "../layout.svelte";
  import { activateTab, hostGroups, layout, moveTab, newGroupFromTab, newTab, renameTab } from "../layout.svelte";
  import { sessionOf, setTabRead, tabAgentLabel, tabIsUnread, tabTitle } from "../sessions.svelte";
  import { AGENT_NAMES } from "../agentStatus";
  import { isLocal, LOCAL_HOST } from "../host/ids";
  import { hostActivity, hostState } from "../host/hosts.svelte";
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
  import { canHandOff, handoffHosts, moveTabToHost, newTabOnHost } from "../handoff/handoff.svelte";
  import { hostName } from "../host/hosts.svelte";

  let { tab }: { tab: Tab } = $props();

  const local = $derived(isLocal(tab.host));
  /** A linked Tab's Host is connected (a local Tab's always is). */
  const online = $derived(local || hostState(tab.host)?.status === "online");
  const hostLabel = $derived(local ? "" : hostName(tab.host));
  const session = $derived(sessionOf(tab));
  const agent = $derived(session?.info?.agent ?? null);
  const status = $derived(session?.status ?? null);
  const finished = $derived(session?.finished ?? false);
  const highlight = $derived(session?.highlight ?? false);
  const remote = $derived(session?.info?.remote ?? false);
  const git = $derived(session?.info?.git ?? null);
  const title = $derived(tabTitle(tab));
  const label = $derived(tabAgentLabel(tab));
  const isActive = $derived(layout.activeTabId === tab.id);
  // Memory Guard is this Mac's: a paired Host's Tabs are never frozen from here.
  const frozen = $derived(local ? frozenSession(tab.sessionId) : undefined);
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
  // This Mac's samples for a local Tab; the Host's `activity` messages for a paired Host's.
  const usage = $derived(
    !activitySettings.tabStats
      ? undefined
      : local
        ? activity.snapshot?.sessions.find((s) => s.sessionId === tab.sessionId)
        : hostActivity(tab.host, tab.sessionId),
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

  /** Freeze (not the Tab in view: going to a Tab thaws it) or Thaw. Local Tabs only: the Host protocol has no freeze. */
  function freezeItem(): MenuItem {
    const sessionId = tab.sessionId;
    if (sessionId === null || !local) return { label: "Freeze", disabled: true };
    if (frozen) return { label: "Thaw", action: () => void thawSession(sessionId) };
    return { label: "Freeze", action: () => void freezeSession(sessionId), disabled: isActive };
  }

  /**
   * "New Tab on <Host>" and "Move Tab to <Host>" (Handoff): one entry per Group of each online
   * paired Host (the Host's own Groups: where the Tab goes there, for its other clients), so the
   * Group there is the user's choice; here the new Tab goes right after this one. Local Tabs
   * only: a Session on a paired Host does not move.
   */
  function handoffItems(): MenuItem[] {
    if (!canHandOff(tab)) return [];
    const targets = handoffHosts().flatMap((h) =>
      hostGroups(h.id).map((g) => ({ host: h.id, group: g, label: `${hostName(h.id)} · ${g.name}` })),
    );
    const none = [{ label: "No Host connected", disabled: true }];
    return [
      {
        label: "New Tab on Host",
        submenu: targets.length ? targets.map((t) => ({ label: t.label, action: () => void newTabOnHost(tab, t.host, t.group.id) })) : none,
      },
      {
        label: "Move Tab to Host",
        submenu: targets.length ? targets.map((t) => ({ label: t.label, action: () => void moveTabToHost(tab, t.host, t.group.id) })) : none,
      },
    ];
  }

  /** A linked Tab's explicit picks for a plain new Tab right after it: on its Host (as ⌘T would), or here. */
  function newTabItems(): MenuItem[] {
    if (local) return [];
    return [
      { label: `New Tab on ${hostLabel}`, action: () => void newTab({ after: tab.id }), disabled: !online },
      { label: "New Local Tab", action: () => void newTab({ after: tab.id, host: LOCAL_HOST }) },
    ];
  }

  function menuItems(): MenuItem[] {
    // Any Group, local and linked Tabs alike: a move places the Tab and never moves its Session
    // (Handoff, below, does).
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
      ...newTabItems(),
      ...handoffItems(),
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
  class:offline={!online}
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
      <span class="title" role="button" tabindex="-1" ondblclick={startEdit}
        >{#if label}{label.project}{#if label.description}<span class="description">{" "}{label.description}</span>{/if}{:else}{title}{/if}</span
      >
    {/if}
    {#if git || remote || !local}
      <span class="badge-line">
        {#if !local}
          <span class="host-chip" title={online ? `On ${hostLabel}` : `On ${hostLabel}, which is not connected`}>{hostLabel}</span>
        {/if}
        {#if git || remote}
          <Badge {git} {remote} />
        {/if}
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
  /* An agent's few words on what it is at: after its project, quieter, never bold. */
  .description {
    font-weight: 400;
    color: var(--text-tertiary);
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
    align-items: center;
    gap: 5px;
    min-width: 0;
    overflow: hidden;
  }
  /* The Host a linked Tab runs on. Local Tabs carry none. */
  .host-chip {
    flex: none;
    max-width: 9em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 10px;
    line-height: 14px;
    padding: 0 5px;
    border-radius: 4px;
    border: 1px solid var(--sidebar-border);
    background: var(--sidebar-bg-raised);
    color: var(--text-secondary);
  }
  /* A linked Tab whose Host is not connected keeps its place and its last facts, greyed. */
  .row.offline .icon,
  .row.offline .title,
  .row.offline .badge-line,
  .row.offline .stats {
    opacity: 0.45;
  }
  .row.offline .host-chip {
    border-style: dashed;
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
