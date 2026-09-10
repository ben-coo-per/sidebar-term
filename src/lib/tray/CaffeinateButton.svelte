<!-- The Tray's Caffeinate toggle: while on, this Mac's display and system stay awake. -->
<script lang="ts">
  import TrayButton from "./TrayButton.svelte";
  import CoffeeIcon from "../sidebar/icons/CoffeeIcon.svelte";
  import { caffeinate, toggleCaffeinate } from "./caffeinate.svelte";
</script>

<TrayButton
  label="Caffeinate"
  title={caffeinate.on ? "Caffeinated: this Mac stays awake. Click to let it sleep" : "Caffeinate: keep this Mac awake"}
  on={caffeinate.on}
  onclick={() => void toggleCaffeinate()}
>
  <CoffeeIcon steaming={caffeinate.on} />
</TrayButton>
