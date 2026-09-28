<!-- Tabs / Manager: the window's mode, in the sidebar's drag region and Manager's top bar. The
     Manager segment carries how many agents wait on the user (hidden at 0). -->
<script lang="ts">
  import { layout, setMode, type WindowMode } from "../layout.svelte";
  import { hotkeyLabel } from "../hotkeys.svelte";
  import { waiting } from "./state.svelte";
  import Segmented from "./Segmented.svelte";

  const count = $derived(waiting().length);
  const key = $derived(hotkeyLabel("view.manager"));
  const items = $derived<{ id: WindowMode; label: string; count?: number; title?: string }[]>([
    { id: "tabs", label: "Tabs", title: key ? `Tabs (${key})` : "Tabs" },
    { id: "manager", label: "Manager", count, title: key ? `Manager (${key})` : "Manager" },
  ]);
</script>

<Segmented label="Window mode" {items} selected={layout.mode} onselect={setMode} />
