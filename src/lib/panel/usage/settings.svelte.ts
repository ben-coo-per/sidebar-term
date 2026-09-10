// Which coding agents the Panel's Usage view shows: the `usage` section of the settings
// (../../settings/store.ts), chosen on the Settings page.

import { loadSection, saveSection } from "../../settings/store";
import type { AgentKind } from "../../types";
import { parseUsageSection, USAGE_AGENTS } from "./model";

export const usageSettings = $state<{
  /** Chosen agents, in display order. */
  agents: AgentKind[];
  /** False until loaded, so the Panel does not start watching the defaults first. */
  ready: boolean;
}>({ agents: [], ready: false });

export async function initUsageSettings(): Promise<void> {
  usageSettings.agents = parseUsageSection(await loadSection("usage"));
  usageSettings.ready = true;
}

export function setUsageAgent(agent: AgentKind, on: boolean): void {
  const chosen = new Set(usageSettings.agents);
  if (on) chosen.add(agent);
  else chosen.delete(agent);
  usageSettings.agents = USAGE_AGENTS.filter((a) => chosen.has(a));
  saveSection("usage", { agents: usageSettings.agents });
}
