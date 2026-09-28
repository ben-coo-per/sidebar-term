<!-- Where an agent runs, compactly: its repo's dot and repo (or repo/worktree, in italics for a
     linked Worktree), as the Badge shows them without the branch. A remote hop reads "remote". -->
<script lang="ts">
  import type { GitInfo } from "../types";
  import { repoColorVar } from "../sidebar/repoColor";

  let { git, remote }: { git: GitInfo | null; remote: boolean } = $props();

  const place = $derived(git ? (git.worktreeName ? `${git.repoName}/${git.worktreeName}` : git.repoName) : "");
</script>

{#if remote}
  <span class="place-badge"><span class="dot remote"></span><span class="name">remote</span></span>
{:else if git}
  <span class="place-badge" title={git.worktreeRoot}>
    <span class="dot" style:background={repoColorVar(git.commonDir)}></span>
    <span class="name" class:linked={!!git.worktreeName}>{place}</span>
  </span>
{/if}

<style>
  .place-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    font-size: 11px;
    line-height: 1.3;
    color: var(--text-tertiary);
  }
  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }
  .dot.remote {
    background: var(--text-tertiary);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name.linked {
    font-style: italic;
  }
</style>
