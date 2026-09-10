// The live Hotkey bindings: defaults from ./hotkeys.ts with the user's overrides applied,
// persisted (overrides only) via settings_load/settings_save (src/lib/ipc.ts). Edited on the
// Settings page (src/lib/settings/SettingsPage.svelte); dispatched by ./shortcuts.ts.

import { loadSettings, saveSettings } from "./ipc";
import {
  ACTIONS,
  actionFor,
  defaultBindings,
  diffFromDefaults,
  formatCombo,
  parseOverrides,
  resolveBindings,
  type ActionId,
  type Bindings,
  type Combo,
} from "./hotkeys";

const SETTINGS_VERSION = 1;

export const hotkeys = $state<{
  bindings: Bindings;
  /** True while the Settings page is capturing a new combo: ./shortcuts.ts stands aside. */
  recording: boolean;
}>({ bindings: defaultBindings(), recording: false });

export async function initHotkeys(): Promise<void> {
  const raw = await loadSettings().catch(() => null);
  const overrides = raw && typeof raw === "object" ? (raw as Record<string, unknown>).hotkeys : null;
  hotkeys.bindings = resolveBindings(parseOverrides(overrides));
}

function persist(): void {
  void saveSettings({ version: SETTINGS_VERSION, hotkeys: diffFromDefaults(hotkeys.bindings) });
}

/**
 * Bind `combo` to `id` (null unassigns). A combo can only drive one action, so whichever action
 * held it before is unassigned; that action is returned so the caller can say so.
 */
export function setBinding(id: ActionId, combo: Combo | null): ActionId | null {
  const displaced = combo ? actionFor(hotkeys.bindings, combo) : null;
  if (displaced && displaced !== id) hotkeys.bindings[displaced] = null;
  hotkeys.bindings[id] = combo;
  persist();
  return displaced === id ? null : displaced;
}

export function resetBinding(id: ActionId): ActionId | null {
  const def = ACTIONS.find((a) => a.id === id)?.default ?? null;
  return setBinding(id, def);
}

export function resetAllBindings(): void {
  hotkeys.bindings = defaultBindings();
  persist();
}

/** The bound combo for display, e.g. "⌘T"; "" when unassigned. */
export function hotkeyLabel(id: ActionId): string {
  return formatCombo(hotkeys.bindings[id]);
}
