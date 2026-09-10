<!-- The Badge: repo / worktree / branch indicator, or a remote marker. See CONTEXT.md "Badge"
     and docs/architecture.md "Worktrees". Renders nothing when there's simply no git info. -->
<script lang="ts">
  import type { GitInfo } from "../types";
  import { repoColorVar } from "./repoColor";
  import RemoteIcon from "./icons/RemoteIcon.svelte";

  let { git, remote }: { git: GitInfo | null; remote: boolean } = $props();

  const refLabel = $derived(git ? (git.branch ?? (git.headShort ? `#${git.headShort}` : "?")) : "");
  const summary = $derived(
    git ? (git.worktreeName ? `${git.repoName} ⎇ ${git.worktreeName} · ${refLabel}` : `${git.repoName} · ${refLabel}`) : "",
  );
  const tooltip = $derived(
    git
      ? [
          git.worktreeName ? `${git.repoName} — worktree ${git.worktreeName}` : git.repoName,
          git.worktreeRoot,
          git.branch ? `branch ${git.branch}` : git.headShort ? `detached at ${git.headShort}` : "",
        ]
          .filter(Boolean)
          .join("\n")
      : "",
  );
</script>

{#if remote}
  <span class="badge remote" title="Remote session: cwd and git reflect this Mac, not the remote host.">
    <RemoteIcon size={10} />
    <span class="label">remote</span>
  </span>
{:else if git}
  <span class="badge" title={tooltip}>
    <span class="dot" style:background={repoColorVar(git.commonDir)}></span>
    <span class="label">{summary}</span>
  </span>
{/if}

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    font-size: 11px;
    color: var(--text-tertiary);
    line-height: 1;
  }
  .badge.remote {
    color: var(--text-secondary);
  }
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
