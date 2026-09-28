// This client's own sidebar state, the `sidebar` section of settings.json: the sidebar width,
// the Panel, the user's unread marks, and the window's mode (Tabs or Manager) with Manager's zoom
// and the sizes the user dragged its areas to. Pure: the parse with its defaults and clamps, no state.
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

/** The window shows every agent's lane (Manager), or the sidebar and one Terminal (Tabs). */
export type WindowMode = "tabs" | "manager";

/** How much time Manager's lanes span: 15 minutes, an hour, 4 hours, or since the oldest lane began. */
export type ManagerZoom = "15m" | "1h" | "4h" | "start";

export const MANAGER_ZOOMS: readonly ManagerZoom[] = ["15m", "1h", "4h", "start"];

export interface SidebarSettings {
  width: number;
  panel: PanelState;
  /** Tab ids the user marked unread. */
  unread: string[];
  mode: WindowMode;
  managerZoom: ManagerZoom;
  /** Height in px of Manager's lanes, as dragged; null: as tall as the lanes, up to a share of the window. */
  managerLanesHeight: number | null;
  /** Width in px of Manager's Needs you column, as dragged; null: a share of the window. */
  managerNeedsWidth: number | null;
}

export const DEFAULT_SIDEBAR_WIDTH = 240;
export const MIN_SIDEBAR_WIDTH = 180;
export const MAX_SIDEBAR_WIDTH = 420;
/** Room for every view's header and a few lines of the open one. */
export const MIN_PANEL_HEIGHT = 128;
export const MAX_PANEL_HEIGHT = 640;
export const DEFAULT_PANEL: PanelState = { view: PANEL_VIEWS[0].id, collapsed: false, height: 220 };
/** The mode a window opens in until the user picks one. */
export const DEFAULT_MODE: WindowMode = "manager";
export const DEFAULT_MANAGER_ZOOM: ManagerZoom = "1h";
/** Room for the lanes' header, two lanes and the legend. */
export const MIN_MANAGER_LANES_HEIGHT = 140;
export const MAX_MANAGER_LANES_HEIGHT = 1200;
/** Room for a card's answers side by side. */
export const MIN_MANAGER_NEEDS_WIDTH = 240;
export const MAX_MANAGER_NEEDS_WIDTH = 900;

export function clampWidth(px: number): number {
  return Math.max(MIN_SIDEBAR_WIDTH, Math.min(MAX_SIDEBAR_WIDTH, Math.round(px)));
}

export function clampPanelHeight(px: number): number {
  return Math.max(MIN_PANEL_HEIGHT, Math.min(MAX_PANEL_HEIGHT, Math.round(px)));
}

export function clampManagerLanesHeight(px: number): number {
  return Math.max(MIN_MANAGER_LANES_HEIGHT, Math.min(MAX_MANAGER_LANES_HEIGHT, Math.round(px)));
}

export function clampManagerNeedsWidth(px: number): number {
  return Math.max(MIN_MANAGER_NEEDS_WIDTH, Math.min(MAX_MANAGER_NEEDS_WIDTH, Math.round(px)));
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
    mode: r.mode === "tabs" || r.mode === "manager" ? r.mode : DEFAULT_MODE,
    managerZoom: MANAGER_ZOOMS.includes(r.managerZoom as ManagerZoom) ? (r.managerZoom as ManagerZoom) : DEFAULT_MANAGER_ZOOM,
    managerLanesHeight: typeof r.managerLanesHeight === "number" ? clampManagerLanesHeight(r.managerLanesHeight) : null,
    managerNeedsWidth: typeof r.managerNeedsWidth === "number" ? clampManagerNeedsWidth(r.managerNeedsWidth) : null,
  };
}
