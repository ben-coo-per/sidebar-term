// The Suites each Host knows about: every live (or just finished) test suite under any of its
// Sessions. The local Host's come on the `suite` event; a paired Host's in its `hello` and
// `suites` messages (src/lib/host/hosts.svelte.ts). Session ids are per Host, so they are kept
// per Host. The Tab row shows its Session's newest; the Activity view lists this Mac's. Pure
// rules in ./model.ts.

import { onSuite } from "../ipc";
import { LOCAL_HOST, type HostId } from "../host/ids";
import type { Tab } from "../layout.svelte";
import type { SuiteSnapshot } from "../types";
import { newestSuite } from "./model";

export const suites = $state<{ byHost: Record<HostId, SuiteSnapshot[]> }>({ byHost: {} });

/** A Host's Suites, whole, as it last sent them. */
export function setHostSuites(host: HostId, list: SuiteSnapshot[]): void {
  suites.byHost[host] = list;
}

export function dropHostSuites(host: HostId): void {
  delete suites.byHost[host];
}

/** This Mac's Suites (the Activity view's Tests block). */
export function localSuites(): SuiteSnapshot[] {
  return suites.byHost[LOCAL_HOST] ?? [];
}

/** Follow the local Host's Suites; returns the function that stops following. */
export function initSuites(): () => void {
  const listening = onSuite((list) => setHostSuites(LOCAL_HOST, list));
  return () => void listening.then((stop) => stop());
}

/** The Suite a Tab shows: its Session's newest, on the Tab's Host, or null. */
export function suiteForTab(tab: Tab): SuiteSnapshot | null {
  if (tab.sessionId === null) return null;
  return newestSuite((suites.byHost[tab.host] ?? []).filter((s) => s.sessionId === tab.sessionId));
}
