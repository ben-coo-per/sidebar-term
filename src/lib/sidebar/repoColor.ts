// Stable colour-dot assignment per repo, keyed by GitInfo.commonDir (identity of a repo across
// its Worktrees). Same repo -> same dot everywhere in the sidebar, even across Groups.

const PALETTE_SIZE = 8; // matches --repo-color-0..7 in theme.css

function hash(input: string): number {
  let h = 2166136261; // FNV-1a
  for (let i = 0; i < input.length; i++) {
    h ^= input.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/** CSS custom property to use as this repo's dot colour, e.g. "var(--repo-color-3)". */
export function repoColorVar(commonDir: string): string {
  return `var(--repo-color-${hash(commonDir) % PALETTE_SIZE})`;
}
