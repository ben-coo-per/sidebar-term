# Architecture and v1 build contract

Status: **provisional**. The v1 build started before every decision ticket on the wayfinder map
(issue #1) was resolved. Where a ticket is still open, this doc states the default v1 uses and
names the ticket; the user may revise it there. Vocabulary: `CONTEXT.md`.

## Split of responsibility

| Side | Owns | Never does |
|---|---|---|
| Rust (`src-tauri/src`) | Sessions (ptys, shells), facts about Sessions (Foreground process, Agent session, cwd, git), layout file I/O | Knows nothing about Tabs, Groups or Titles |
| Webview (`src`) | The layout model: Groups, Tabs, Titles, order, active Tab; xterm.js Terminals; all UI | Never shells out or reads the filesystem |

A **Tab** points at a **Session** by `SessionId`. Session ids are per app run; the persisted layout
stores each Tab's last cwd instead and respawns a shell there on relaunch.

## IPC (contract: `src-tauri/src/lib.rs` <-> `src/lib/ipc.ts`)

| Command | Args (JS names) | Returns |
|---|---|---|
| `session_spawn` | `cwd?, cols, rows, onData: Channel` | `SessionId`; output bytes stream on `onData` as raw `ArrayBuffer` |
| `session_write` | `sessionId, data: string` | - |
| `session_resize` | `sessionId, cols, rows` | - |
| `session_pause` / `session_resume` | `sessionId` | - (flow control, see `docs/research/pty.md`) |
| `session_kill` | `sessionId` | - (then `session-exit` fires) |
| `session_reset` | - | - (kills every Session and stops Activity and Usage reading; called once at webview startup so a reload leaves no orphans) |
| `session_info` | `sessionId` | `SessionInfo \| null` (fresh probe) |
| `activity_watch` | `on: boolean` | - (start / stop sampling Activity) |
| `usage_watch` | `on: boolean, agents: AgentKind[]` | - (start, change the agents of, or stop reading Usage) |
| `layout_load` / `layout_save` | `layout: json` | opaque JSON blob in the app data dir |
| `settings_load` / `settings_save` | `settings: json` | opaque JSON blob in the app data dir; one section per owner (`hotkeys`, `usage`), merged by `src/lib/settings/store.ts` |

| Event | Payload | When |
|---|---|---|
| `session-info` | `SessionInfo` | first probe of a Session, then on every change (monitor tick 500 ms) |
| `session-exit` | `SessionExit` | the shell exited or was killed |
| `activity` | `ActivitySnapshot` | every 2 s while `activity_watch(true)`; the first right away |
| `usage` | `UsageSnapshot` | right away on `usage_watch(true, ..)`, then whenever a number changes (checked every 5 s) |
| `menu-settings` | - | the app menu's "Settings…" was chosen |

Types: `src-tauri/src/model.rs` mirrored by `src/lib/types.ts`. Outside Tauri, `ipc.ts` routes to
`src/lib/mock.ts`, a fake backend for developing the UI in a browser (`pnpm dev`, then open
`http://127.0.0.1:1420`). Type `help` in a mock terminal.

## Rust modules

- `session.rs` — `SessionManager` (Tauri state): spawn `$SHELL -l` on a `portable-pty` pty, one
  reader thread per Session coalescing output into `InvokeResponseBody::Raw`, write, resize,
  pause/resume, kill, `probe_targets()`.
- `detect/` — `probe(&ProbeTarget) -> SessionInfo`: libproc for the Foreground process group,
  agent classification, remote-hop detection, cwd; `.git` file reading for repo / Worktree / branch.
- `monitor.rs` — thread ticking every 500 ms: probe every target, emit `session-info` on change.
- `activity.rs` — `Activity` (Tauri state): thread idle until watched, then every 2 s runs
  `/bin/ps` over every process, attributes each to a Session by ppid descent from its shell, and
  emits `activity` (see "Panel").
- `usage.rs` — `Usage` (Tauri state): thread idle until watched, then every 5 s reads the chosen
  agents' usage limits and emits `usage` on change (see "Panel").
- `lib.rs` also builds the app menu: Tauri's default plus "Settings…" (no key equivalent: the
  Settings Hotkey stays the webview's, rebindable).
- `layout.rs` — atomic JSON read/write of `layout.json` and `settings.json` in the app data dir.

## Webview modules

- `src/lib/terminal/manager.ts` — `terminals`: one xterm.js `Terminal` per Session, mount only the
  active one, WebGL on the mounted Terminal with DOM fallback, fit, flow control, title/bell events.
- `src/lib/terminal/TerminalPane.svelte` — shows the active Session's Terminal.
- `src/lib/layout.svelte.ts` — Groups/Tabs model, actions, persistence (debounced `layout_save`).
- `src/lib/sessions.svelte.ts` — reactive `SessionInfo` per Session plus derived Agent status.
- `src/lib/hotkeys.ts` — Hotkey actions, defaults and the pure rules for combos;
  `src/lib/hotkeys.svelte.ts` — the live bindings (persisted overrides); `src/lib/shortcuts.ts` —
  the window listener that dispatches them.
- `src/lib/settings/*` — the Settings page (Usage agents, Hotkeys), shown over the Terminal; the
  settings blob's per-section store (`store.ts`).
- `src/lib/sidebar/*` — sidebar components. `src/routes/+page.svelte` — app shell.
- `src/lib/panel/*` — the Panel (`Panel.svelte`), its view list (`views.ts`), the Activity view
  (`activity/`: snapshot store, pure sorting / formatting / meter maths, components) and the Usage
  view (`usage/`: snapshot store, chosen agents, pure formatting, components).

## v1 product defaults (provisional)

- **Scope** (#7): one window, no split panes, no profiles, no settings UI beyond Usage agents and Hotkeys, no quick
  switcher. Tabs move between Groups by drag-and-drop and by a context menu.
- **Persistence** (#8): Groups (name, order, collapsed), Tabs (order, custom Title, last cwd), the
  active Tab, sidebar width and the Panel (view, collapsed, height) persist. On relaunch every Tab respawns a shell at its last cwd.
- **Naming** (#10): automatic Title priority: agent name ("Claude Code", "Codex", "Gemini") when an
  Agent session; else the OSC title if the Foreground process set one; else the Foreground process
  name when it is not the shell; else the cwd basename (`~` for home). A rename sticks until the
  user clears it (renaming to empty restores the automatic Title). New Tabs join the active Tab's
  Group, directly after it. A fresh install has one Group named "Tabs". Double-click or context menu
  renames Tabs and Groups; Enter commits, Escape cancels.
- **Worktrees** (#11): Badge shows `repo · branch`; in a linked Worktree it shows the Worktree name
  too (`repo ⎇ worktree · branch`); detached HEAD shows the short sha. Every Tab in the same repo
  (same `commonDir`) gets the same colour dot, so Worktrees of one repo read as related across
  Groups. Remote sessions show a remote marker instead of a Badge. No dirty/ahead-behind in v1.
- **Agent icon** (#12): one robot icon for any agent (tooltip names it); a plain-session icon
  otherwise. The icon reverts when the agent exits.
- **Agent status** (new issue, see map): every Agent session shows Running / Needs input / Done
  in the Tab's icon slot (see "Agent status" below).
- **Interaction** (#13): Cmd-T new Tab, Cmd-Shift-N new Group, Cmd-W close Tab, Cmd-1..9 go to
  the Nth Group (the Tab last active in it, else its first; expands a collapsed Group), Cmd-` /
  Cmd-Shift-` next / previous Tab within the active Tab's Group (wrapping), Cmd-Shift-[ / ] previous
  / next Tab across all Groups, Cmd-Opt-Up/Down move Tab, Cmd-B toggle sidebar, Cmd-, Settings
  (also the app menu's "Settings…"; the sidebar has no Settings button).
  These are defaults: every one is a Hotkey the user can rebind on the Settings page
  (`src/lib/hotkeys.ts` holds the actions and rules; overrides persist in `settings.json` next to
  `layout.json`). A Group header shows its go-to-Group Hotkey and its Tab count as `NAME (2)  ⌘1`.
  Closing a Tab whose Foreground process is not the shell asks for confirmation in an in-app dialog
  (never `window.confirm`). Sidebar width is draggable.
- **Architecture** (#14): as above; ADR `docs/adr/0001-rust-owns-sessions-webview-owns-layout.md`.

## Agent status

Derived in the webview from `SessionInfo.agent`, the Terminal's OSC title, BEL and output activity
(sources: `docs/research/agent-detection.md`).

| Status | Codex | Gemini | Claude Code |
|---|---|---|---|
| Running | title starts with a braille spinner char (U+2800-U+28FF) | title starts with `✦` | title starts with `◐` or `◑` |
| Needs input | title contains `Action Required` | title starts with `✋` | BEL while the agent is foreground |
| Done | title without spinner / after activity stops | title starts with `◇` | title starts with `✳` |

Claude Code (2.1.267) prefixes its title with `◐`/`◑` while busy, alternating every ~1 s while its
terminal is focused, frozen on one frame otherwise. It uses `✳` when idle or waiting on a prompt.
Read from its bundled source. There is no prefix under tmux (always `✳`), with
`CLAUDE_CODE_DISABLE_TERMINAL_TITLE`, or in older versions. With no prefix we fall back to "output
within the last ~3 s", which keystroke echo and redraws also trip.

The Tab's icon slot shows the status: a spinner while Running, the robot once stopped (amber for
Needs input). When the agent exits, the Tab stops being an Agent session; if that happens while the
Tab is not active, the icon is a check until the Tab is next activated. A Done or Needs-input status
on a background Tab is highlighted until the Tab is activated.

## Panel

The Panel sits at the bottom of the sidebar, beneath the Groups and the New Tab / New Group
buttons. It is an accordion: every view has its own header, and at most one view is open.
Clicking a closed view's header opens it and closes the open one; clicking the open view's header
closes it, leaving only headers. A closed view's header shows its summary, so every view reads its
data while the Panel is shown. With a view open, the Panel's top edge drags to resize, up to 70%
of the sidebar. It is hidden while the sidebar is narrower than 220 px. The open view (`view`,
with `collapsed` when none is) and the height persist in the layout (`panel`).

Views are listed in `src/lib/panel/views.ts` and rendered by `Panel.svelte`: Activity, then Usage.

**Activity** shows two meters (CPU out of every core, memory out of physical memory), each split
into one segment per Session in its Tab colour, in sidebar order, then one muted segment for
everything else; and a list of processes sortable by CPU or memory. A Session's processes show in
its Tab colour, and clicking one goes to its Tab. Every other process is muted grey. Collapsed, the
header shows CPU and Memory Used instead.

- Source: `/bin/ps -axo pid,ppid,rss,time,%cpu,comm`, every 2 s, only while the Panel is shown. libproc's task info is EPERM for other users' processes, about a
  third of all processes and usually the busiest (WindowServer, kernel_task); `ps` is setuid root.
  One run costs ~20 ms.
- CPU% is the change in CPU time between samples over wall time, 100% = one core, as in Activity
  Monitor. A process seen for the first time uses `ps`'s own decaying %cpu.
- A process belongs to a Session if it is the Session's shell or descends from it by ppid, so
  background jobs count and a daemon that detaches (reparents to launchd) does not.
- Memory Used is Activity Monitor's: app memory + wired + compressed (`host_statistics64`).
- Rust sends every Session process plus the top 40 others by CPU and the top 40 by memory.
- Tab colour: the repo's Badge-dot colour, or `--tab-color-plain` outside a repo.

**Usage** shows, for each agent chosen on the Settings page (Claude Code and Codex by default),
one thin bar per limit window: its label (`5h`, `Week`), percent used and time until it resets.
Bars are neutral grey, amber from 80% and red from 95%. A window whose reset time has passed reads
0%. Numbers older than 5 minutes say how old; an agent whose numbers could not be read says why,
with its last numbers dimmed. Closed, the header shows each agent's fullest window
(`Claude 48%  Codex 3%`). Gemini CLI has no usage source yet.

- Claude Code: `GET https://api.anthropic.com/api/oauth/usage` (undocumented; what Claude Code's
  `/usage` calls; `anthropic-beta: oauth-2025-04-20`), every 60 s (5 min after a 429), with the
  OAuth access token from the login Keychain item `Claude Code-credentials` (read with
  `/usr/bin/security`; `~/.claude/.credentials.json` as fallback). Answer: `five_hour`,
  `seven_day`, `seven_day_opus`, `seven_day_sonnet`, each `{utilization: percent, resets_at: RFC
  3339}` or null. The token is only read, never refreshed: refreshing would rotate Claude Code's
  refresh token and sign it out. It reaches `/usr/bin/curl` on stdin, never in argv.
- Codex: the last `payload.rate_limits` record (`primary` / `secondary`: `used_percent`,
  `window_minutes`, `resets_at` epoch s) in the most recently written of its session logs,
  `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`, newest 14 day directories. Codex writes one per
  turn from its API's rate-limit headers, so the numbers are as fresh as the last Codex turn on
  this Mac. A log is re-parsed only when its mtime or size changes.

## Window

`titleBarStyle: Overlay`, hidden title: the sidebar runs to the top of the window, leaves ~28 px for
the traffic lights, and marks its header `data-tauri-drag-region`. `dragDropEnabled: false` so HTML5
drag-and-drop works in the sidebar: on macOS Tauri's handler claims every drag, including the
webview's own. Files dropped on a Terminal therefore arrive as DOM `File`s; they paste as
shell-escaped paths read off the drag pasteboard, or saved to a temp dir when they have none
(`src/lib/terminal/drop.ts`, `src-tauri/src/drop.rs`).
