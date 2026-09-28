# sidebar-term

A macOS terminal app with a vertical sidebar of tabs. Tabs can be renamed and arranged into named groups. A tab shows a robot icon when a coding agent (Claude Code, Codex, Gemini CLI) is running in it, and a badge for the repo, worktree and branch its shell is in.

Built on Tauri 2 + Svelte + xterm.js: an existing terminal emulator and pty, nothing custom in the terminal runtime.
The Rust side is a workspace (`src-tauri/`): the core (`core/`), the Mac app around it, and
`sidebar-termd` (`daemon/`), a headless Host for a second machine.

Status: planning. The design is being charted as a wayfinder map on this repo's GitHub issues (label `wayfinder:map`).

## Install

```sh
pnpm install
pnpm app:install      # release build -> /Applications/sidebar-term.app, pinned to the Dock once
```

Run the same command to update the installed app after pulling or changing code. It's safe to run
from inside the app: it builds first, then quits the app, installs the new build and relaunches it
(log in `$TMPDIR/sidebar-term-install.log`). Tabs come back at their last cwd, but whatever was
running in them, agents included, is killed. The relaunched app then offers to bring it back: a banner at
the bottom resumes each Claude Code conversation (`claude --resume <id>`) and reruns other commands
(`npm run dev`, `uv run ...`) in their Tabs. It does the same after a crash or any quit.

## Phone

Your Sessions from your phone: see the sidebar, open any Tab, type, answer an agent's prompts.
It is a web page the Mac app serves, installed on the home screen (no App Store, no expiry), and
reachable only over your Tailscale network. Once:

1. Install [Tailscale](https://tailscale.com/download) on the Mac and on the phone, signed in to
   the same account. In the [admin console](https://login.tailscale.com/admin/dns), enable
   MagicDNS and HTTPS certificates.
2. In sidebar-term, Settings (⌘,) → Remote → turn on **Remote access**. It should say
   `Phones open https://<your-mac>.<tailnet>.ts.net/m`.
3. Press **Pair a phone…** and scan the QR code with the phone's camera (or open the link and
   type the code). Name the phone; it is now paired until you remove it here.
4. In Safari on the phone: Share → **Add to Home Screen**. Open it from there: full screen, and
   the pairing is kept.

Nothing listens while Remote is off. On, the server binds 127.0.0.1 only; Tailscale publishes it
to your tailnet (never the internet) with a real certificate. Each phone needs a one-time pairing
code and holds a token you can revoke in Settings. Details: `docs/architecture.md` "Host protocol".

**Status and next steps.** The phone can see the sidebar and drive any Session (attach-and-drive).
Still to do, in order:

1. First real run: install Tailscale on the Mac and the phone (step 1 above; this Mac had none
   when Remote was built, so Tailscale Serve has only been exercised against its CLI contract),
   turn Remote on, pair, and check the page installs from Safari. If `tailscale serve` refuses,
   Settings shows its message; HTTPS certificates must be enabled in the admin console.
2. Try it with a real Claude Code prompt: answer a permission question from the phone, use the
   key bar's Esc / Shift-Tab, and see whether the fit-to-width font is readable in portrait.
3. Passkey / Face ID re-lock after idle (WebAuthn; the page is on a real HTTPS origin, so it is
   cheap to add). Not built yet: today the token alone admits a paired phone.
4. Full control from the phone (create, close, rename, move Tabs): the Host protocol carries
   the commands (`docs/architecture.md` "Host protocol"); the phone's UI for them is #16.
5. Notifications when an agent needs input (web push works for installed pages on iOS 16.4+).

## Host daemon

`sidebar-termd` runs the same core as the app on a machine with no display (a Linux box on your
tailnet), so Sessions can live there and be driven from the Mac app (Settings > Hosts: its Tabs
join your Groups, tagged with the Host's name) and from a phone. It keeps its own Tabs and Groups
(`layout.json` in its data dir: one Tab on first run, every Tab's shell respawned at launch). It
is a second binary from the same workspace; `docs/architecture.md` "Host daemon" and "Hosts" say
what it does at this stage.

```sh
cargo build --release --bin sidebar-termd --manifest-path src-tauri/Cargo.toml   # no Tauri
install -m 755 src-tauri/target/release/sidebar-termd ~/.local/bin/
pnpm build && mkdir -p ~/.local/share/sidebar-term && cp -r build ~/.local/share/sidebar-term/web
mkdir -p ~/.config/systemd/user && cp packaging/systemd/sidebar-termd.service ~/.config/systemd/user/
systemctl --user daemon-reload && systemctl --user enable --now sidebar-termd
loginctl enable-linger "$USER"        # keep it running with no one logged in
```

It needs Tailscale running on that machine (with Serve available) for `https://<host>.<tailnet>.ts.net/m`;
without it the server still listens on `127.0.0.1:47611`. To pair a phone:
`systemctl --user kill --kill-whom=main -s USR1 sidebar-termd`, then read the code and link in
`journalctl --user -u sidebar-termd -n 3`. Flags (`sidebar-termd --help`): `--data-dir`, `--port`,
`--web-root`, `--pair`. On a Mac, for a smoke test, `cargo run --bin sidebar-termd -- --web-root build --pair`
keeps its files in `~/Library/Application Support/com.bencooper.sidebarterm/daemon`, apart from
the app's. It turns Remote on, so with Tailscale on that Mac it also points the Mac's Serve rule
at itself; `tailscale serve --https=443 off` undoes that afterwards.

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
