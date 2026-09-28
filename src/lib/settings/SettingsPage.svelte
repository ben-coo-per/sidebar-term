<!-- The Settings page: shown over the Terminal (⌘, or the app menu's "Settings…"). Its categories
     (./categories.ts) are listed down the left, and the selected one's section shows in the pane
     beside them; a narrow page lists them across the top instead. The page holds no setting
     itself. What the sections share in looks is ./settings.css. -->
<script lang="ts">
  import "./settings.css";
  import { hotkeys } from "../hotkeys.svelte";
  import CloseIcon from "../sidebar/icons/CloseIcon.svelte";
  import { CATEGORIES, categoryOf } from "./categories";
  import { closeSettings, selectCategory, settingsPage } from "./visibility.svelte";

  /** A page narrower than this (px) lists its categories across the top. */
  const COMPACT_BELOW = 640;

  let page = $state<HTMLElement>();
  /** The page's width, 0 until measured. */
  let width = $state(0);

  const compact = $derived(width > 0 && width < COMPACT_BELOW);
  const current = $derived(categoryOf(settingsPage.category));
  const Section = $derived(current.component);

  function focusCategory(id: string) {
    page?.querySelector<HTMLElement>(`[data-category="${id}"]`)?.focus();
  }

  function onPageKeydown(e: KeyboardEvent) {
    if (hotkeys.recording) return;
    if (e.key === "Escape") {
      e.preventDefault();
      closeSettings();
      return;
    }
    if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
    // The arrow keys move between categories from the navigation only: in the pane they scroll,
    // and move the caret in a field.
    if (!(e.target as Element).closest(".nav")) return;
    const at = CATEGORIES.findIndex((c) => c.id === current.id);
    let to: number;
    switch (e.key) {
      case "ArrowUp":
      case "ArrowLeft":
        to = Math.max(at - 1, 0);
        break;
      case "ArrowDown":
      case "ArrowRight":
        to = Math.min(at + 1, CATEGORIES.length - 1);
        break;
      case "Home":
        to = 0;
        break;
      case "End":
        to = CATEGORIES.length - 1;
        break;
      default:
        return;
    }
    e.preventDefault();
    const { id } = CATEGORIES[to];
    selectCategory(id);
    focusCategory(id);
  }

  // Keyboard focus belongs in the page while it shows, so that Esc closes it and the arrow keys
  // move between categories: on opening, and when the section that held the focus went away.
  $effect(() => {
    const { id } = current;
    if (page && !page.contains(document.activeElement)) focusCategory(id);
  });
</script>

<div
  class="settings-page"
  class:compact
  role="dialog"
  aria-label="Settings"
  tabindex="-1"
  bind:this={page}
  bind:clientWidth={width}
  onkeydown={onPageKeydown}
>
  <div class="nav-column">
    <h1>Settings</h1>
    <div class="nav" role="tablist" aria-label="Settings categories" aria-orientation={compact ? "horizontal" : "vertical"}>
      {#each CATEGORIES as c (c.id)}
        {@const selected = c.id === current.id}
        <button
          type="button"
          class="nav-item"
          class:selected
          role="tab"
          id="settings-category-{c.id}"
          aria-selected={selected}
          aria-controls="settings-pane"
          tabindex={selected ? 0 : -1}
          data-category={c.id}
          onclick={(e) => {
            selectCategory(c.id);
            // WebKit doesn't focus buttons on click.
            e.currentTarget.focus();
          }}
        >
          {c.label}
        </button>
      {/each}
    </div>
  </div>

  <!-- Keyed: each category starts scrolled to its top. -->
  {#key current.id}
    <div class="pane" id="settings-pane" role="tabpanel" aria-labelledby="settings-category-{current.id}" tabindex="-1">
      <div class="pane-column">
        <Section />
      </div>
    </div>
  {/key}

  <button type="button" class="icon-btn close" onclick={closeSettings} title="Close (Esc)" aria-label="Close Settings">
    <CloseIcon size={12} />
  </button>
</div>

<style>
  .settings-page {
    position: absolute;
    inset: 0;
    z-index: 50;
    display: flex;
    background: var(--term-bg);
    color: var(--text-primary);
    font-family: var(--font-ui);
    outline: none;
    animation: fade var(--duration-fast) var(--ease-standard);
  }

  /* The navigation: a column in the sidebar's looks. */
  .nav-column {
    flex: none;
    width: 176px;
    overflow-x: hidden;
    overflow-y: auto;
    background: var(--sidebar-bg);
    border-right: 1px solid var(--sidebar-border);
  }
  h1 {
    margin: 0;
    padding: 16px 16px 10px;
    font-size: calc(16px * var(--ui-font-scale));
    font-weight: 600;
  }
  .nav {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 0 6px 12px;
  }
  .nav-item {
    appearance: none;
    flex: none;
    min-height: var(--row-height);
    padding: 4px 10px;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    font-size: calc(12.5px * var(--ui-font-scale));
    text-align: left;
    cursor: default;
    user-select: none;
    outline: none;
  }
  .nav-item:hover {
    background: var(--sidebar-bg-raised);
  }
  .nav-item.selected {
    background: var(--sidebar-bg-active);
    color: var(--text-primary);
  }
  .nav-item:focus-visible {
    box-shadow: inset 0 0 0 1.5px var(--focus-ring);
  }

  /* The pane: the selected category's section, in a column of a width that reads well. Its side
     padding keeps the column clear of the close button at any width. */
  .pane {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    overflow-x: hidden;
    overflow-y: auto;
    padding: 16px 40px 40px;
    outline: none;
  }
  /* A container (settings-pane), for a section to ask how wide it is. */
  .pane-column {
    max-width: 600px;
    margin: 0 auto;
    container: settings-pane / inline-size;
  }
  .pane::-webkit-scrollbar,
  .nav-column::-webkit-scrollbar {
    width: 8px;
  }
  .pane::-webkit-scrollbar-thumb,
  .nav-column::-webkit-scrollbar-thumb {
    background: var(--scrollbar-thumb);
    border-radius: 4px;
  }
  .close {
    position: absolute;
    top: 10px;
    right: 10px;
  }

  /* Narrow: the navigation is a row across the top, wrapping when it must; the close button
     sits beside the title, over the navigation. */
  .compact {
    flex-direction: column;
  }
  .compact .nav-column {
    width: auto;
    overflow: visible;
    border-right: none;
    border-bottom: 1px solid var(--sidebar-border);
  }
  .compact h1 {
    padding: 14px 48px 8px 16px;
  }
  .compact .nav {
    flex-direction: row;
    flex-wrap: wrap;
    gap: 2px;
    padding: 0 10px 8px;
  }
  .compact .pane {
    padding: 16px 16px 32px;
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
