<!-- The Settings page's Usage section: which agents the Panel's Usage view shows.
     State: src/lib/panel/usage/settings.svelte.ts. -->
<script lang="ts">
  import { AGENT_NAMES } from "../agentStatus";
  import { USAGE_AGENTS } from "../panel/usage/model";
  import { setUsageAgent, usageSettings } from "../panel/usage/settings.svelte";
  import type { AgentKind } from "../types";

  /** Where each agent's usage comes from, said next to its checkbox. */
  const USAGE_SOURCES: Record<AgentKind, string> = {
    claude: "Asks api.anthropic.com every minute, signed in as Claude Code (from your Keychain).",
    codex: "Read from Codex's session logs, which it updates on every turn.",
    gemini: "",
  };
</script>

<section>
  <div class="section-header">
    <div>
      <h2>Usage</h2>
      <p class="hint">Agents whose usage limits the Panel's Usage view shows.</p>
    </div>
  </div>
  <ul class="rows">
    {#each USAGE_AGENTS as agent (agent)}
      <li class="row">
        <label class="check">
          <span class="label">
            {AGENT_NAMES[agent]}
            <span class="detail">{USAGE_SOURCES[agent]}</span>
          </span>
          <input
            type="checkbox"
            checked={usageSettings.agents.includes(agent)}
            onchange={(e) => setUsageAgent(agent, e.currentTarget.checked)}
          />
        </label>
      </li>
    {/each}
  </ul>
</section>
