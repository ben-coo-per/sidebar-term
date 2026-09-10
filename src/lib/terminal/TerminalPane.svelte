<!-- Shows the active Session's Terminal. OWNER: terminal agent. CONTRACT: props are fixed. -->
<script lang="ts">
  import { terminals } from "./manager";
  import type { SessionId } from "../types";

  let { sessionId }: { sessionId: SessionId | null } = $props();
  let el: HTMLDivElement;

  $effect(() => {
    const id = sessionId;
    if (id === null) return;
    terminals.mount(id, el);
    const ro = new ResizeObserver(() => terminals.fit(id));
    ro.observe(el);
    return () => {
      ro.disconnect();
      terminals.unmount(id);
    };
  });
</script>

<div class="terminal-pane" bind:this={el}></div>

<style>
  .terminal-pane {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
</style>
