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
| `session_info` | `sessionId` | `SessionInfo \| null` (fresh probe) |
| `layout_load` / `layout_save` | `layout: json` | opaque JSON blob in the app data dir |

| Event | Payload | When |
|---|---|---|
| `session-info` | `SessionInfo` | first probe of a Session, then on every change (monitor tick ~1 s) |
| `session-exit` | `SessionExit` | the shell exited or was killed |

Types: `src-tauri/src/model.rs` mirrored by `src/lib/types.ts`. Outside Tauri, `ipc.ts` routes to
`src/lib/mock.ts`, a fake backend for developing the UI in a browser (`pnpm dev`, then open
`http://127.0.0.1:1420`). Type `help` in a mock terminal.

## Rust modules

- `session.rs` — `SessionManager` (Tauri state): spawn `$SHELL -l` on a `portable-pty` pty, one
  reader thread per Session coalescing output into `InvokeResponseBody::Raw`, write, resize,
  pause/resume, kill, `probe_targets()`.
- `detect/` — `probe(&ProbeTarget) -> SessionInfo`: libproc for the Foreground process group,
  agent classification, remote-hop detection, cwd; `.git` file reading for repo / Worktree / branch.
- `monitor.rs` — thread ticking ~1 s: probe every target, emit `session-info` on change.
- `layout.rs` — atomic JSON read/write of `layout.json` in the app data dir.

## Webview modules

- `src/lib/terminal/manager.ts` — `terminals`: one xterm.js `Terminal` per Session, mount only the
  active one, WebGL on the mounted Terminal with DOM fallback, fit, flow control, title/bell events.
- `src/lib/terminal/TerminalPane.svelte` — shows the active Session's Terminal.
- `src/lib/layout.svelte.ts` — Groups/Tabs model, actions, persistence (debounced `layout_save`).
- `src/lib/sessions.svelte.ts` — reactive `SessionInfo` per Session plus derived Agent status.
- `src/lib/sidebar/*` — sidebar components. `src/routes/+page.svelte` — app shell.

## v1 product defaults (provisional)

- **Scope** (#7): one window, no split panes, no profiles, no settings UI, no quick switcher. Tabs
  move between Groups by drag-and-drop and by a context menu.
- **Persistence** (#8): Groups (name, order, collapsed), Tabs (order, custom Title, last cwd), the
  active Tab and sidebar width persist. On relaunch every Tab respawns a shell at its last cwd.
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
- **Interaction** (#13): Cmd-T new Tab, Cmd-Shift-N new Group, Cmd-W close Tab, Cmd-1..9 jump to
  the Nth visible Tab, Cmd-Shift-[ / ] previous / next Tab, Cmd-Opt-Up/Down move Tab, Cmd-B toggle
  sidebar. Closing a Tab whose Foreground process is not the shell asks for confirmation in an
  in-app dialog (never `window.confirm`). Sidebar width is draggable.
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

## Window

`titleBarStyle: Overlay`, hidden title: the sidebar runs to the top of the window, leaves ~28 px for
the traffic lights, and marks its header `data-tauri-drag-region`. `dragDropEnabled: false` so HTML5
drag-and-drop works in the sidebar.
