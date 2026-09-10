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
| `session_reset` | - | - (kills every Session and stops Activity sampling; called once at webview startup so a reload leaves no orphans) |
| `session_info` | `sessionId` | `SessionInfo \| null` (fresh probe) |
| `activity_watch` | `on: boolean` | - (start / stop sampling Activity) |
| `layout_load` / `layout_save` | `layout: json` | opaque JSON blob in the app data dir |
| `settings_load` / `settings_save` | `settings: json` | opaque JSON blob (Hotkey overrides) in the app data dir |

| Event | Payload | When |
|---|---|---|
| `session-info` | `SessionInfo` | first probe of a Session, then on every change (monitor tick 500 ms) |
| `session-exit` | `SessionExit` | the shell exited or was killed |
| `activity` | `ActivitySnapshot` | every 2 s while `activity_watch(true)`; the first right away |

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
- `src/lib/settings/*` — the Settings page (Hotkeys), shown over the Terminal.
- `src/lib/sidebar/*` — sidebar components. `src/routes/+page.svelte` — app shell.
- `src/lib/panel/*` — the Panel (`Panel.svelte`), its view list (`views.ts`) and the Activity
  view (`activity/`: snapshot store, pure sorting / formatting / meter maths, components).

## v1 product defaults (provisional)

- **Scope** (#7): one window, no split panes, no profiles, no settings UI beyond Hotkeys, no quick
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
  (see "Agent status" below).
- **Interaction** (#13): Cmd-T new Tab, Cmd-Shift-N new Group, Cmd-W close Tab, Cmd-1..9 go to
  the Nth Group (the Tab last active in it, else its first; expands a collapsed Group), Cmd-` /
  Cmd-Shift-` next / previous Tab within the active Tab's Group (wrapping), Cmd-Shift-[ / ] previous
  / next Tab across all Groups, Cmd-Opt-Up/Down move Tab, Cmd-B toggle sidebar, Cmd-, Settings.
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
| Running | title starts with a braille spinner char (U+2800-U+28FF) | title starts with `✦` | output activity within the last ~3 s |
| Needs input | title contains `Action Required` | title starts with `✋` | BEL while the agent is foreground |
| Done | title without spinner / after activity stops | title starts with `◇` | no output for ~3 s after Running |

When the agent exits, the Tab stops being an Agent session; if that happens while the Tab is not
active, the Tab keeps a "finished" marker until it is next activated. A Done or Needs-input status
on a background Tab is highlighted until the Tab is activated.

## Panel

The Panel sits at the bottom of the sidebar, beneath the Groups and the New Tab / New Group
buttons. Its header holds a strip of view tabs; clicking the shown view's tab (or the header)
collapses the Panel to that header, and clicking another view's tab switches to it and expands.
Its top edge drags to resize, up to 70% of the sidebar. It is hidden while the sidebar is narrower
than 220 px. Which view, collapsed or not, and the height persist in the layout (`panel`).

Views are listed in `src/lib/panel/views.ts` and rendered by `Panel.svelte`. Activity is the only
one so far; a view of coding agents' usage limits is planned.

**Activity** shows two meters (CPU out of every core, memory out of physical memory), each split
into one segment per Session in its Tab colour, in sidebar order, then one muted segment for
everything else; and a list of processes sortable by CPU or memory. A Session's processes show in
its Tab colour, and clicking one goes to its Tab. Every other process is muted grey. Collapsed, the
header shows CPU and Memory Used instead.

- Source: `/bin/ps -axo pid,ppid,rss,time,%cpu,comm`, every 2 s, only while the Panel shows
  Activity (collapsed included). libproc's task info is EPERM for other users' processes, about a
  third of all processes and usually the busiest (WindowServer, kernel_task); `ps` is setuid root.
  One run costs ~20 ms.
- CPU% is the change in CPU time between samples over wall time, 100% = one core, as in Activity
  Monitor. A process seen for the first time uses `ps`'s own decaying %cpu.
- A process belongs to a Session if it is the Session's shell or descends from it by ppid, so
  background jobs count and a daemon that detaches (reparents to launchd) does not.
- Memory Used is Activity Monitor's: app memory + wired + compressed (`host_statistics64`).
- Rust sends every Session process plus the top 40 others by CPU and the top 40 by memory.
- Tab colour: the repo's Badge-dot colour, or `--tab-color-plain` outside a repo.

## Window

`titleBarStyle: Overlay`, hidden title: the sidebar runs to the top of the window, leaves ~28 px for
the traffic lights, and marks its header `data-tauri-drag-region`. `dragDropEnabled: false` so HTML5
drag-and-drop works in the sidebar: on macOS Tauri's handler claims every drag, including the
webview's own. Files dropped on a Terminal therefore arrive as DOM `File`s; they paste as
shell-escaped paths read off the drag pasteboard, or saved to a temp dir when they have none
(`src/lib/terminal/drop.ts`, `src-tauri/src/drop.rs`).
