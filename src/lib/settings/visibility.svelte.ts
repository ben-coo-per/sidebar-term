// Whether the Settings page (./SettingsPage.svelte) is showing over the Terminal, and which of
// its categories (./categories.ts) it shows. Not persisted: within one app run the page reopens
// on the category last shown.

import { activeTab, tabSessionKey } from "../layout.svelte";
import { terminals } from "../terminal/manager";
import type { CategoryId } from "./categories";

export const settingsPage = $state<{ open: boolean; category: CategoryId | null }>({
  open: false,
  /** Null until one is chosen: the page then shows its first category. */
  category: null,
});

/**
 * Open the page, on `category` when one is named. Also used as an event handler and a menu
 * callback, so anything that is not a category id (an event, a payload) is ignored.
 */
export function openSettings(category?: CategoryId | Event): void {
  if (typeof category === "string") settingsPage.category = category;
  settingsPage.open = true;
}

export function selectCategory(category: CategoryId): void {
  settingsPage.category = category;
}

/** Close, handing keyboard focus back to the active Tab's Terminal. */
export function closeSettings(): void {
  if (!settingsPage.open) return;
  settingsPage.open = false;
  const key = tabSessionKey(activeTab());
  if (key !== null) terminals.focus(key);
}

export function toggleSettings(): void {
  if (settingsPage.open) closeSettings();
  else openSettings();
}
