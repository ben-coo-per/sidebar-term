<!-- The Tray: a row of small icon buttons and indicators at the top of the sidebar, right of the
     traffic lights. Each item is its own component, usually a TrayButton; list them here in order.
     See docs/architecture.md "Tray". -->
<script lang="ts">
  import CaffeinateButton from "./CaffeinateButton.svelte";
</script>

<div class="tray" role="toolbar" aria-label="Tray">
  <CaffeinateButton />
</div>

<style>
  .tray {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    overflow: hidden;
  }
</style>
