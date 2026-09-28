// Closing a Tab whose Foreground process isn't the shell asks for confirmation first, via the
// in-app ConfirmDialog (never window.confirm). Shared by Cmd-W, the row's close button, and the
// context menu's Close item so there is exactly one place this rule lives. Local or linked: the
// local Host is re-probed on the spot; a linked Tab's Host's word is its last forwarded
// SessionInfo. A linked Tab closes on its Host, so not while that Host is not connected.

import { sessionInfo } from "../ipc";
import { closeTab, layout } from "../layout.svelte";
import { isLocal } from "../host/ids";
import { hostName, hostState } from "../host/hosts.svelte";
import { sessionOf, tabTitle } from "../sessions.svelte";
import { requestConfirm, requestNotice } from "./confirm.svelte";

export async function requestCloseTab(tabId: string): Promise<void> {
  const tab = layout.tabs[tabId];
  if (!tab) return;
  if (!isLocal(tab.host) && hostState(tab.host)?.status !== "online") {
    await requestNotice({
      message: `Could not close "${tabTitle(tab)}".`,
      detail: `It runs on ${hostName(tab.host)}, which is not connected. Close it once ${hostName(tab.host)} is back.`,
    });
    return;
  }
  if (tab.sessionId === null) {
    closeTab(tabId);
    return;
  }

  const info = isLocal(tab.host)
    ? await sessionInfo(tab.sessionId).catch(() => null)
    : (sessionOf(tab)?.info ?? null);
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
