<!-- Keys a phone keyboard lacks, above it: Esc, Tab, Shift-Tab, a one-shot Ctrl, arrows, ^C.
     Sends key sequences through `send`; `ctrl` reflects the armed Ctrl. -->
<script lang="ts">
  let {
    send,
    ctrl,
    onCtrl,
    applicationCursor,
  }: {
    send: (sequence: string) => void;
    ctrl: boolean;
    onCtrl: () => void;
    /** DECCKM: arrows send `\x1bOA` instead of `\x1b[A`. */
    applicationCursor: () => boolean;
  } = $props();

  const arrow = (letter: string) => () => send((applicationCursor() ? "\x1bO" : "\x1b[") + letter);

  const keys: { label: string; title: string; action: () => void; wide?: boolean }[] = [
    { label: "esc", title: "Escape", action: () => send("\x1b") },
    { label: "tab", title: "Tab", action: () => send("\t") },
    { label: "⇧tab", title: "Shift-Tab", action: () => send("\x1b[Z") },
    { label: "ctrl", title: "Control (applies to the next key)", action: () => onCtrl() },
    { label: "^C", title: "Control-C", action: () => send("\x03") },
    { label: "↑", title: "Up", action: arrow("A") },
    { label: "↓", title: "Down", action: arrow("B") },
    { label: "←", title: "Left", action: arrow("D") },
    { label: "→", title: "Right", action: arrow("C") },
    { label: "⏎", title: "Return", action: () => send("\r") },
  ];
</script>

<div class="keybar" role="toolbar" aria-label="Terminal keys">
  {#each keys as k (k.label)}
    <button
      type="button"
      class:armed={k.label === "ctrl" && ctrl}
      title={k.title}
      aria-label={k.title}
      onpointerdown={(e) => {
        // Keep the focus (and the keyboard) on the Terminal.
        e.preventDefault();
        k.action();
      }}
    >
      {k.label}
    </button>
  {/each}
</div>

<style>
  .keybar {
    flex: none;
    display: flex;
    gap: 4px;
    padding: 6px 6px calc(6px + env(safe-area-inset-bottom));
    overflow-x: auto;
    background: var(--sidebar-bg);
    border-top: 1px solid var(--sidebar-border);
    scrollbar-width: none;
  }
  .keybar::-webkit-scrollbar {
    display: none;
  }
  button {
    flex: 1 0 auto;
    min-width: 40px;
    height: 36px;
    padding: 0 10px;
    border: 1px solid var(--sidebar-border);
    border-radius: 7px;
    background: var(--sidebar-bg-raised);
    color: var(--text-primary);
    font: inherit;
    font-size: 14px;
    font-family: "SF Mono", ui-monospace, Menlo, monospace;
    touch-action: manipulation;
    user-select: none;
    -webkit-user-select: none;
  }
  button:active {
    background: var(--sidebar-bg-active);
  }
  button.armed {
    border-color: var(--accent);
    background: var(--accent-dim);
    color: var(--accent-strong);
  }
</style>
