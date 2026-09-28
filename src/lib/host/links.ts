// Linked Tabs (ADR 0003): which of a paired Host's Tabs this Mac should have links for, and
// whether its links are out of line with them, so src/lib/layout.svelte.ts asks the core to
// reconcile them (`links_reconcile`, whose rules are src-tauri/core/src/layout/model.rs
// `reconcile_links`) only when something changed. Pure.

import type { LayoutSnapshot } from "../types";
import type { HostId } from "./ids";

/**
 * A Host's Tab ids in its own sidebar order (the order strays are linked in), less those this
 * Mac has asked it to close: until its snapshot drops them, they are neither strays nor kept.
 */
export function hostTabOrder(snap: LayoutSnapshot, closing: ReadonlySet<string> = new Set()): string[] {
  return snap.groups.flatMap((g) => g.tabIds).filter((id) => snap.tabs[id] && !closing.has(id));
}

/** Whether `host` has a Tab (of `there`) this Mac has no link for, or this Mac links a Tab not in `there`. */
export function linksOutOfLine(mac: LayoutSnapshot, host: HostId, there: string[]): boolean {
  const has = new Set(there);
  const linked = new Set<string>();
  for (const t of Object.values(mac.tabs)) {
    if (t.link?.hostId !== host) continue;
    if (!has.has(t.link.tabId)) return true;
    linked.add(t.link.tabId);
  }
  return there.some((id) => !linked.has(id));
}
