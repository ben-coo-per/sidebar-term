// The persisted app settings blob (`settings.json`, via settings_load/settings_save). Each part of
// the app owns one section of it (`hotkeys`: src/lib/hotkeys.svelte.ts; `usage`:
// src/lib/panel/usage/settings.svelte.ts) and saves only that section: the others are kept.

import { loadSettings, saveSettings } from "../ipc";

const SETTINGS_VERSION = 1;

let blob: Record<string, unknown> = {};
let loading: Promise<Record<string, unknown>> | null = null;

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

/** The saved blob, read once per app run; `{}` on first run or when unreadable. */
function load(): Promise<Record<string, unknown>> {
  loading ??= loadSettings()
    .catch(() => null)
    .then((raw) => {
      blob = isRecord(raw) ? { ...raw } : {};
      return blob;
    });
  return loading;
}

/** One section of the saved settings, or undefined when there is none. */
export async function loadSection(key: string): Promise<unknown> {
  return (await load())[key];
}

/** Save one section, keeping every other section as loaded. */
export function saveSection(key: string, value: unknown): void {
  void load().then(() => {
    blob = { ...blob, version: SETTINGS_VERSION, [key]: value };
    void saveSettings(blob);
  });
}
