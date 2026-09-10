// Closing a Tab whose Foreground process isn't the shell asks for confirmation first, via the
// in-app ConfirmDialog (never window.confirm). Shared by Cmd-W, the row's close button, and the
// context menu's Close item so there is exactly one place this rule lives.

import { sessionInfo } from "../ipc";
import { closeTab, layout } from "../layout.svelte";
import { tabTitle } from "../sessions.svelte";
import { requestConfirm } from "./confirm.svelte";

export async function requestCloseTab(tabId: string): Promise<void> {
  const tab = layout.tabs[tabId];
  if (!tab) return;
  if (tab.sessionId === null) {
    closeTab(tabId);
    return;
  }

  const info = await sessionInfo(tab.sessionId).catch(() => null);
  if (info && !info.shellIsForeground) {
    const ok = await requestConfirm({
      message: `Close "${tabTitle(tab)}"?`,
      detail: `${info.foreground ?? "A process"} is still running in this tab.`,
      confirmLabel: "Close Tab",
      danger: true,
    });
    if (!ok) return;
  }
  closeTab(tabId);
}
