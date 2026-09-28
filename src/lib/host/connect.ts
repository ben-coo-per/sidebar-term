// How the Mac app reaches a paired Host: the real connection (./client.ts) in the app, or the
// browser mock's fake Host (src/lib/mock.ts) for a `mock://` URL under plain `vite dev`, so the
// sidebar's linked Tabs can be developed without a daemon. As src/lib/ipc.ts routes IPC.

import { inTauri } from "../ipc";
import * as mock from "../mock";
import { pairWithHost, RemoteClient, type HostClient, type Paired } from "./client";

export function isMockHostUrl(url: string): boolean {
  return !inTauri && url.startsWith("mock://");
}

/** A connection to the Host at `url`, not yet connected. */
export function openHostClient(url: string, token: string): HostClient {
  return isMockHostUrl(url) ? mock.hostClient(url, token) : new RemoteClient(url, token);
}

/** Present a pairing code to the Host at `url`; resolves with the token, rejects with why not. */
export function pairHost(url: string, code: string, name: string): Promise<Paired> {
  return isMockHostUrl(url) ? mock.pairHost(url, code, name) : pairWithHost(url, code, name);
}
