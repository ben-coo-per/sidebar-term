// Remote on the Mac: a mirror of the core's state (src-tauri/core/src/remote/) for the Settings
// page. The sidebar phones list is the Host's own (the core's layout joined with Session facts),
// so nothing is published from here. See docs/architecture.md "Host protocol".

import { onRemote, remotePairBegin, remotePairCancel, remoteRevoke, remoteState, setRemote } from "../ipc";
import type { RemoteSnapshot } from "../types";

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
