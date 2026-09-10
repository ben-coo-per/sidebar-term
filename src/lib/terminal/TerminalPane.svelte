<!-- Shows the active Session's Terminal. OWNER: terminal agent. CONTRACT: props are fixed. -->
<script lang="ts">
  import type { SessionId } from "../types";

  let { sessionId }: { sessionId: SessionId | null } = $props();
</script>

<div class="terminal-pane" data-session={sessionId}></div>

<style>
  .terminal-pane {
    width: 100%;
    height: 100%;
  }
</style>
