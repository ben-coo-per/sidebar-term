// The "Suite progress" setting: whether new Sessions get the progress variables that load the
// app's reporters (the `suite` section of the settings, ../settings/store.ts). Read before the
// first Session spawns, since the variables are set at spawn; Suites are detected either way.

import { loadSection, saveSection } from "../settings/store";
import { parseSuiteSection } from "./model";

export const suiteSettings = $state<{ progress: boolean; ready: boolean }>({ progress: true, ready: false });

export async function initSuiteSettings(): Promise<void> {
  suiteSettings.progress = parseSuiteSection(await loadSection("suite")).progress;
  suiteSettings.ready = true;
}

/** Applies to Sessions spawned from now on. */
export function setSuiteProgress(on: boolean): void {
  suiteSettings.progress = on;
  saveSection("suite", { progress: on });
}
