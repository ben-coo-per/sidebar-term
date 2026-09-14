// Remote on the Mac: a mirror of Rust's state (src-tauri/src/remote/) for the Settings page,
// and the publisher that sends phones the sidebar whenever it changes. See docs/architecture.md
// "Remote".

import { onRemote, publishSidebar, remotePairBegin, remotePairCancel, remoteRevoke, remoteState, setRemote } from "../ipc";
import type { RemoteSnapshot } from "../types";
import { layout } from "../layout.svelte";
import { sessionState, tabTitle } from "../sessions.svelte";
import { buildSidebarSnapshot } from "./sidebar";

export const remote = $state<{
  /** Rust's latest word; null until first read. */
  snapshot: RemoteSnapshot | null;
  /** A command is in flight (turning on runs the Tailscale CLI). */
  busy: boolean;
  /** Why the last command failed, shown under the toggle. */
  error: string | null;
}>({ snapshot: null, busy: false, error: null });

/** Read the state and follow its changes; returns the function that stops following. */
export function initRemote(): () => void {
  const listening = onRemote((s) => {
    remote.snapshot = s;
  });
  void remoteState()
    .then((s) => {
      remote.snapshot = s;
    })
    .catch((e) => (remote.error = String(e)));
  return () => void listening.then((stop) => stop());
}

/** Re-read Tailscale's state (the Settings page does this when it opens). */
export async function refreshRemote(): Promise<void> {
  try {
    remote.snapshot = await remoteState();
  } catch (e) {
    remote.error = String(e);
  }
}

export async function toggleRemote(on: boolean): Promise<void> {
  remote.busy = true;
  remote.error = null;
  try {
    remote.snapshot = await setRemote(on);
  } catch (e) {
    remote.error = String(e);
    await refreshRemote();
  } finally {
    remote.busy = false;
  }
}

export async function beginPairing(): Promise<void> {
  try {
    await remotePairBegin();
  } catch (e) {
    remote.error = String(e);
  }
}

export async function cancelPairing(): Promise<void> {
  await remotePairCancel().catch(() => {});
}

export async function revokeDevice(id: string): Promise<void> {
  await remoteRevoke(id).catch((e) => (remote.error = String(e)));
}

// --- The sidebar, for phones ---------------------------------------------------------------------

const PUBLISH_DEBOUNCE_MS = 150;

/**
 * While Remote is on, send phones the sidebar whenever anything it shows changes: Groups, Tabs,
 * Titles, Agent status, Badges, the active Tab. Returns the function that stops publishing.
 */
export function initSidebarPublisher(): () => void {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let last = "";
  const stop = $effect.root(() => {
    $effect(() => {
      if (!remote.snapshot?.on) return;
      const snapshot = buildSidebarSnapshot({
        groups: layout.groups,
        tabs: layout.tabs,
        activeTabId: layout.activeTabId,
        titleOf: (id) => tabTitle(layout.tabs[id]),
        stateOf: sessionState,
      });
      const json = JSON.stringify(snapshot);
      if (json === last) return;
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        last = json;
        void publishSidebar(snapshot).catch((e) => console.error("remote: publishing the sidebar failed", e));
      }, PUBLISH_DEBOUNCE_MS);
    });
  });
  return () => {
    if (timer !== null) clearTimeout(timer);
    stop();
  };
}
