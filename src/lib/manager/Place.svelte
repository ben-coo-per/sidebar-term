<!-- Where a lane's agent runs, under its project: its repo's dot and the branch (or `@sha` when
     detached), left out when it only repeats the linked Worktree's name. A remote hop reads
     "remote". The repo itself is on the lane's first line. -->
<script lang="ts">
  import type { GitInfo } from "../types";
  import { repoColorVar } from "../sidebar/repoColor";

  let { git, remote }: { git: GitInfo | null; remote: boolean } = $props();

  const ref = $derived(git ? (git.branch ?? (git.headShort ? `@${git.headShort}` : "")) : "");
  const showRef = $derived(!!ref && !(git?.worktreeName && git.branch === git.worktreeName));
</script>

{#if remote}
  <span class="place-badge"><span class="dot remote"></span><span class="name">remote</span></span>
{:else if git}
  <span class="place-badge" title={git.worktreeRoot}>
    <span class="dot" style:background={repoColorVar(git.commonDir)}></span>
    {#if showRef}<span class="name">{ref}</span>{/if}
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
</style>
