// Which Host a Tab or Session belongs to, as this client tells them apart. The Mac app is its
// own local Host (reached in process); every paired Host is a section of the sidebar (see
// docs/architecture.md "Hosts"). Tab and Group ids are 128-bit random on every Host
// (src-tauri/core/src/layout/model.rs `new_id`), so the client keys its mirror by them as they
// are; Session ids are small per-Host integers, so a Session is keyed by Host and id. Pure.

import type { SessionId } from "../types";

/** `"local"` for this Mac's own Host; a paired Host's id (settings `hosts[].id`) otherwise. */
export type HostId = string;

export const LOCAL_HOST: HostId = "local";

export function isLocal(host: HostId): boolean {
  return host === LOCAL_HOST;
}

/** One Session across every Host: `<host>/<session id>`. What the Terminal manager and the Session facts are keyed by. */
export type SessionKey = string;

export function sessionKey(host: HostId, id: SessionId): SessionKey {
  return `${host}/${id}`;
}

export function parseSessionKey(key: SessionKey): { host: HostId; id: SessionId } | null {
  const slash = key.lastIndexOf("/");
  if (slash <= 0) return null;
  const id = Number(key.slice(slash + 1));
  if (!Number.isInteger(id) || id < 0) return null;
  return { host: key.slice(0, slash), id };
}
