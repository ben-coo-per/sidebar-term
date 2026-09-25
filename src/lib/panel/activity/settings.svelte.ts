// Whether each Tab shows its CPU and memory: the `activity` section of the settings
// (../../settings/store.ts), chosen on the Settings page. While on, Activity is sampled.

import { loadSection, saveSection } from "../../settings/store";
import { parseActivitySection } from "./model";

export const activitySettings = $state<{ tabStats: boolean }>({ tabStats: false });

export async function initActivitySettings(): Promise<void> {
  activitySettings.tabStats = parseActivitySection(await loadSection("activity")).tabStats;
}

export function setTabStats(on: boolean): void {
  activitySettings.tabStats = on;
  saveSection("activity", { tabStats: on });
}
