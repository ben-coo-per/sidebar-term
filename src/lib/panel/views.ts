// The Panel's views, in tab-strip order. To add one (e.g. agent usage limits): add it here and
// render its component in Panel.svelte. See docs/architecture.md "Panel".

export const PANEL_VIEWS = [{ id: "activity", label: "Activity" }] as const;

export type PanelViewId = (typeof PANEL_VIEWS)[number]["id"];

export function isPanelViewId(value: unknown): value is PanelViewId {
  return PANEL_VIEWS.some((v) => v.id === value);
}
