# sidebar-term

See `README.md` for what this is and `CONTEXT.md` for the vocabulary. Use the glossary's terms in issues, code and docs.

## File names must differ by more than case

This app is built on macOS, whose default filesystem is case-insensitive. Never add two paths that differ only by case, and that includes the extensionless form an import names. A `Foo.svelte` component next to a `foo.svelte.ts` rune module breaks the macOS build: `import … from "./foo.svelte"` resolves to the component, and every named import fails with `MISSING_EXPORT`. Linux and case-sensitive volumes build it fine, so CI and your own build won't catch it. Name the module something else (e.g. `src/lib/manager/state.svelte.ts` next to `Manager.svelte`).

## Agent skills

### Issue tracker

GitHub Issues on this repo, via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
