// The Settings page's categories, in the order its navigation lists them. Each is one entry here
// plus one component beside this file (./XxxSection.svelte), styled by ./settings.css.

import type { Component } from "svelte";
import AppearanceSection from "./AppearanceSection.svelte";
import UsageSection from "./UsageSection.svelte";
import RemoteSection from "./RemoteSection.svelte";
import HostsSection from "./HostsSection.svelte";
import MemorySection from "./MemorySection.svelte";
import HotkeysSection from "./HotkeysSection.svelte";

export interface Category {
  /** Names the category to openSettings (./visibility.svelte.ts). Never shown. */
  id: string;
  /** Its name in the navigation. The component's own heading repeats it. */
  label: string;
  /** What the page shows while the category is selected. Takes no props. */
  component: Component;
}

export const CATEGORIES = [
  { id: "appearance", label: "Appearance", component: AppearanceSection },
  { id: "usage", label: "Usage", component: UsageSection },
  { id: "remote", label: "Remote", component: RemoteSection },
  { id: "hosts", label: "Hosts", component: HostsSection },
  { id: "memory", label: "Memory", component: MemorySection },
  { id: "hotkeys", label: "Hotkeys", component: HotkeysSection },
] as const satisfies readonly Category[];

export type CategoryId = (typeof CATEGORIES)[number]["id"];

/** The category with this id; the first one when there is none (nothing chosen yet, or an id since removed). */
export function categoryOf(id: string | null): (typeof CATEGORIES)[number] {
  return CATEGORIES.find((c) => c.id === id) ?? CATEGORIES[0];
}
