<!-- Shows the active Session's Terminal. OWNER: terminal agent. CONTRACT: props are fixed. -->
<script lang="ts">
  import { terminals } from "./manager";
  import { TERMINAL_BACKGROUND } from "./theme";
  import { carriesFiles, resolveDroppedPaths, shellEscape } from "./drop";
  import type { SessionId } from "../types";

  let { sessionId }: { sessionId: SessionId | null } = $props();
  let el: HTMLDivElement;
  let dropTarget = $state(false);

  $effect(() => {
    const id = sessionId;
    if (id === null) return;
    terminals.mount(id, el);
    // `terminals.fit` coalesces to one fit per animation frame and only resizes the pty when the
    // grid changed, so a sidebar drag costs one fit per frame.
    const ro = new ResizeObserver(() => terminals.fit(id));
    ro.observe(el);
    // Clicks on the padding or the slack right/below the grid would otherwise blur the Terminal.
    const onMouseDown = (ev: MouseEvent) => {
      if (ev.target !== el && ev.target !== el.firstElementChild) return;
      ev.preventDefault();
      terminals.focus(id);
    };
    const onClick = () => terminals.focus(id);
    // Dropped files paste as shell-escaped paths (see ./drop.ts).
    const onDragOver = (ev: DragEvent) => {
      if (!carriesFiles(ev)) return;
      ev.preventDefault();
      ev.dataTransfer!.dropEffect = "copy";
      dropTarget = true;
    };
    const onDragLeave = (ev: DragEvent) => {
      if (!el.contains(ev.relatedTarget as Node | null)) dropTarget = false;
    };
    const onDrop = (ev: DragEvent) => {
      if (!carriesFiles(ev)) return;
      ev.preventDefault();
      dropTarget = false;
      const files = Array.from(ev.dataTransfer!.files);
      void resolveDroppedPaths(files).then((paths) => {
        if (paths.length) terminals.paste(id, paths.map(shellEscape).join(" ") + " ");
      });
    };
    el.addEventListener("mousedown", onMouseDown);
    el.addEventListener("click", onClick);
    el.addEventListener("dragover", onDragOver);
    el.addEventListener("dragleave", onDragLeave);
    el.addEventListener("drop", onDrop);
    return () => {
      ro.disconnect();
      el.removeEventListener("mousedown", onMouseDown);
      el.removeEventListener("click", onClick);
      el.removeEventListener("dragover", onDragOver);
      el.removeEventListener("dragleave", onDragLeave);
      el.removeEventListener("drop", onDrop);
      dropTarget = false;
      terminals.unmount(id);
    };
  });
</script>

<div
  class="terminal-pane"
  class:drop-target={dropTarget}
  bind:this={el}
  style:--terminal-bg={TERMINAL_BACKGROUND}
></div>

<style>
  .terminal-pane {
    box-sizing: border-box;
    width: 100%;
    height: 100%;
    /* Inner padding; the Terminal's host fills the content box and fit() measures that. */
    padding: 6px 4px 4px 10px;
    overflow: hidden;
    background: var(--terminal-bg);
  }
  /* Drawn in the padding, around the Terminal. */
  .terminal-pane.drop-target {
    box-shadow: inset 0 0 0 2px var(--accent);
  }
</style>
