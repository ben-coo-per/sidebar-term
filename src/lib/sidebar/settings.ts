// This client's own sidebar state, the `sidebar` section of settings.json: the sidebar width,
// the Panel and the user's unread marks. Pure: the parse with its defaults and clamps, no state.
// The Host's layout (Groups, Tabs, the active Tab) is src/lib/layout.svelte.ts's mirror; this is
// what stays presentation (ADR 0002). A version-1 layout.json's `sidebarWidth`, `panel` and
// unread Tabs land here once, moved by the Host (src-tauri/core/src/layout/file.rs).

import { isPanelViewId, PANEL_VIEWS, type PanelViewId } from "../panel/views";

/** The Panel at the bottom of the sidebar: an accordion of views, at most one open. */
export interface PanelState {
  /** The open view, or the last one open while every view is closed. */
  view: PanelViewId;
  /** Every view closed to its header. */
  collapsed: boolean;
  /** Height in px while a view is open, headers included. */
  height: number;
}

export interface SidebarSettings {
  width: number;
  panel: PanelState;
  /** Tab ids the user marked unread. */
  unread: string[];
}

export const DEFAULT_SIDEBAR_WIDTH = 240;
export const MIN_SIDEBAR_WIDTH = 180;
export const MAX_SIDEBAR_WIDTH = 420;
/** Room for every view's header and a few lines of the open one. */
export const MIN_PANEL_HEIGHT = 128;
export const MAX_PANEL_HEIGHT = 640;
export const DEFAULT_PANEL: PanelState = { view: PANEL_VIEWS[0].id, collapsed: false, height: 220 };

export function clampWidth(px: number): number {
  return Math.max(MIN_SIDEBAR_WIDTH, Math.min(MAX_SIDEBAR_WIDTH, Math.round(px)));
}

export function clampPanelHeight(px: number): number {
  return Math.max(MIN_PANEL_HEIGHT, Math.min(MAX_PANEL_HEIGHT, Math.round(px)));
}

/** Missing or bad fields fall back to the defaults (a layout saved before the Panel has none). */
export function parsePanel(raw: unknown): PanelState {
  const p = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  return {
    view: isPanelViewId(p.view) ? p.view : DEFAULT_PANEL.view,
    collapsed: typeof p.collapsed === "boolean" ? p.collapsed : DEFAULT_PANEL.collapsed,
    height: typeof p.height === "number" ? clampPanelHeight(p.height) : DEFAULT_PANEL.height,
  };
}

/** Defensive parse of the section: unknown JSON, possibly stale or hand-edited. */
export function parseSidebarSection(raw: unknown): SidebarSettings {
  const r = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  return {
    width: typeof r.width === "number" ? clampWidth(r.width) : DEFAULT_SIDEBAR_WIDTH,
    panel: parsePanel(r.panel),
    unread: Array.isArray(r.unread) ? r.unread.filter((x): x is string => typeof x === "string") : [],
  };
}
