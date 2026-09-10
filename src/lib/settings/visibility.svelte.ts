// Whether the Settings page (./SettingsPage.svelte) is showing over the Terminal.

import { activeTab } from "../layout.svelte";
import { terminals } from "../terminal/manager";

export const settingsPage = $state<{ open: boolean }>({ open: false });

export function openSettings(): void {
  settingsPage.open = true;
}

/** Close, handing keyboard focus back to the active Tab's Terminal. */
export function closeSettings(): void {
  if (!settingsPage.open) return;
  settingsPage.open = false;
  const sessionId = activeTab()?.sessionId ?? null;
  if (sessionId !== null) terminals.focus(sessionId);
}

export function toggleSettings(): void {
  if (settingsPage.open) closeSettings();
  else openSettings();
}
