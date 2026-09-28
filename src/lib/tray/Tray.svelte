<!-- The Tray: a row of small icon buttons and indicators at the right of the window bar
     (src/lib/window/WindowBar.svelte), in both modes. Each item is its own component, usually a TrayButton; list them here in order.
     See docs/architecture.md "Tray". -->
<script lang="ts">
  import CaffeinateButton from "./CaffeinateButton.svelte";
  import MemoryGuardButton from "./MemoryGuardButton.svelte";
</script>

<div class="tray" role="toolbar" aria-label="Tray">
  <CaffeinateButton />
  <MemoryGuardButton />
</div>

<style>
  .tray {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    overflow: hidden;
  }
</style>
