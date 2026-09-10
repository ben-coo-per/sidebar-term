<!-- Throwaway manual test page for the terminal host (terminal agent). Not part of the app shell.
     pnpm dev --port 1432 --strictPort, then open http://127.0.0.1:1432/terminal-harness -->
<script lang="ts">
  import { onMount } from "svelte";
  import TerminalPane from "$lib/terminal/TerminalPane.svelte";
  import { terminals } from "$lib/terminal/manager";
  import type { SessionId } from "$lib/types";

  let sessions = $state<SessionId[]>([]);
  let active = $state<SessionId | null>(null);
  let log = $state<string[]>([]);
  let wide = $state(true);
  let status = $state("");

  function note(line: string) {
    log = [`${new Date().toLocaleTimeString()} ${line}`, ...log].slice(0, 40);
  }

  async function add() {
    const id = await terminals.create();
    sessions = [...sessions, id];
    active = id;
  }

  async function closeActive() {
    if (active !== null) await terminals.close(active);
  }

  onMount(() => {
    const offs = [
      terminals.on("title", (id, t) => note(`#${id} title ${JSON.stringify(t)}`)),
      terminals.on("bell", (id) => note(`#${id} bell`)),
      terminals.on("activity", (id) => note(`#${id} activity`)),
      terminals.on("exit", (id, code) => {
        note(`#${id} exit ${code}`);
        sessions = sessions.filter((s) => s !== id);
        if (active === id) active = sessions.at(-1) ?? null;
      }),
    ];
    void add();
    const timer = setInterval(() => {
      const screens = document.querySelectorAll(".xterm-screen").length;
      const canvases = document.querySelectorAll(".xterm-screen > canvas:not([class])").length;
      status = `xterm in DOM: ${screens}, WebGL canvases: ${canvases}`;
    }, 500);
    return () => {
      clearInterval(timer);
      offs.forEach((off) => off());
    };
  });
</script>

<div class="harness">
  <header>
    <button onclick={add}>New Session</button>
    {#each sessions as id (id)}
      <button class:on={id === active} onclick={() => (active = id)}>#{id}</button>
    {/each}
    <button onclick={() => (active = null)}>Hide</button>
    <button onclick={closeActive} disabled={active === null}>Close active</button>
    <button onclick={() => (wide = !wide)}>Toggle width</button>
    <span class="count">{status}</span>
  </header>
  <div class="body">
    <section class="pane" style:width={wide ? "100%" : "60%"}>
      <TerminalPane sessionId={active} />
    </section>
    <pre class="log">{log.join("\n")}</pre>
  </div>
</div>

<style>
  :global(body) {
    margin: 0;
  }
  .harness {
    display: flex;
    flex-direction: column;
    height: 100vh;
    font: 12px system-ui;
    background: #1a1d23;
    color: #ccc;
  }
  header {
    display: flex;
    gap: 6px;
    padding: 8px;
    flex-wrap: wrap;
  }
  .on {
    font-weight: bold;
    outline: 2px solid #81a1c1;
  }
  .body {
    display: grid;
    grid-template-columns: 1fr 280px;
    flex: 1;
    min-height: 0;
  }
  .pane {
    height: 100%;
    min-width: 0;
  }
  .log {
    margin: 0;
    padding: 8px;
    overflow: auto;
    font-size: 11px;
  }
  .count {
    opacity: 0.6;
  }
</style>
