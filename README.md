# sidebar-term

A macOS terminal app with a vertical sidebar of tabs. Tabs can be renamed and arranged into named groups. A tab shows a robot icon when a coding agent (Claude Code, Codex, Gemini CLI) is running in it, and a badge for the repo, worktree and branch its shell is in.

Built on Tauri 2 + Svelte + xterm.js: an existing terminal emulator and pty, nothing custom in the terminal runtime.

Status: planning. The design is being charted as a wayfinder map on this repo's GitHub issues (label `wayfinder:map`).

## Install

```sh
pnpm install
pnpm app:install      # release build -> /Applications/sidebar-term.app, pinned to the Dock once
```

Run the same command to update the installed app after pulling or changing code. It's safe to run
from inside the app: it builds first, then quits the app, installs the new build and relaunches it
(log in `$TMPDIR/sidebar-term-install.log`). Tabs come back at their last cwd, but whatever was
running in them, agents included, is killed.

## App icon

The icon is generated from `src-tauri/icons/app-icon.png`, a 1024x1024 **full-bleed, fully opaque**
square (no rounded corners or transparent margins: macOS 26 applies its own mask, and puts icons with
transparent margins on a grey plate). The current one, an iridescent blob drawn as halftone dots on
the terminal's background, is drawn by `scripts/icon/make-icon.py`. To use your own design:

```sh
cp ~/Desktop/my-icon.png src-tauri/icons/app-icon.png
pnpm app:icon         # regenerate every size
pnpm app:install      # rebuild, reinstall, refresh the Dock
```

## Worktrees

Worktrees under `.claude/worktrees/` build Rust into the main checkout's `src-tauri/target`
(`.claude/worktrees/.cargo/config.toml`), so a new worktree doesn't compile and store its own copy of
every dependency (~2-4 GB). Builds in two checkouts at once wait for each other, and `cargo clean`
anywhere clears the shared build.

`scripts/demo/run.sh` starts the app in dev mode with fake Claude Code and Codex agents, to see the
agent icon and status without running a real agent.
