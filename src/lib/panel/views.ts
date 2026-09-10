// The Panel's views, in accordion order, top to bottom. To add one: add it here and render its
// view and its closed-header summary in Panel.svelte. See docs/architecture.md "Panel".

export const PANEL_VIEWS = [
  { id: "activity", label: "Activity" },
  { id: "usage", label: "Usage" },
] as const;

export type PanelViewId = (typeof PANEL_VIEWS)[number]["id"];

export function isPanelViewId(value: unknown): value is PanelViewId {
  return PANEL_VIEWS.some((v) => v.id === value);
}
