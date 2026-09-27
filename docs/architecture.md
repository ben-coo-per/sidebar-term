# Architecture and v1 build contract

Status: **provisional**. The v1 build started before every decision ticket on the wayfinder map
(issue #1) was resolved. Where a ticket is still open, this doc states the default v1 uses and
names the ticket; the user may revise it there. Vocabulary: `CONTEXT.md`.

## Split of responsibility

| Side | Owns | Never does |
|---|---|---|
| The core (`src-tauri/core`, Rust) | Sessions (ptys, shells), facts about Sessions (Foreground process, Agent session, cwd, git, the OSC title and BELs read in the output, Agent status), the layout (Groups, Tabs, order, custom Titles, each Tab's last cwd and Session, the active Tab; `layout.json`), Memory Guard, Resume, the Remote server and the Host protocol it serves | Knows nothing about automatic Titles or Unread (a client's); never names Tauri |
| The app (`src-tauri/src`, Rust + Tauri) | The window, the menu, the IPC commands, Caffeinate, file drops; links the core as the local Host | - |
| `sidebar-termd` (`src-tauri/daemon`, Rust) | The headless Host: links the core, serves it over Remote, runs under systemd | Never needs a display |
| Webview (`src`) | A mirror of every Host's layout and Session facts (the local Host's in process, each paired Host's over the Host protocol), and what is presentation: the Tab in view, sidebar width and visibility, the Panel, automatic Titles, Unread, drag-and-drop, the close-Tab confirmation (`settings.json`, section `sidebar`), which Hosts it is paired with (section `hosts`); xterm.js Terminals; all UI | Never shells out or reads the filesystem; never changes a layout except through its Host's commands; never derives Agent status itself |

A **Tab** points at a **Session** by `SessionId`. Session ids are per app run; the persisted layout
stores each Tab's last cwd instead and the Host respawns a shell there at launch, with its Tab.
A Session is spawned by the Host as its Tab is made; the webview *attaches* its Terminal to it
(`session_attach`) and gets what the Session printed before, then the live output.

## IPC (contract: `src-tauri/src/lib.rs` <-> `src/lib/ipc.ts`)

| Command | Args (JS names) | Returns |
|---|---|---|
| `session_attach` | `sessionId, onData: Channel` | - ; the Session's output streams on `onData` as raw `ArrayBuffer`, starting with what it printed before the Terminal attached; rejects for a Session that is gone |
| `session_write` | `sessionId, data: string` | - |
| `session_resize` | `sessionId, cols, rows` | - |
| `session_pause` / `session_resume` | `sessionId` | - (flow control, see `docs/research/pty.md`) |
| `session_reset` | - | - (called once at webview startup: every Session a previous page's Terminal was attached to is replaced by a fresh one in its Tab, so a reload leaves no orphans, and Activity and Usage reading stop; what the replaced Sessions ran becomes Resume leftover; the app's first page finds nothing attached and keeps what the Host spawned at launch) |
| `session_info` | `sessionId` | `SessionInfo \| null` (a fresh probe, with the Session's title, BELs and Agent status as the monitor would send them) |
| `layout_get` | - | `LayoutSnapshot`: the whole layout, for the first read; every change after that is a `layout` event |
| `tab_new` | `groupId?, afterTabId?, cwd?, cols?, rows?` | `Tab`, with its Session, active; defaults: the active Tab's Group, right after it, at its cwd (see "Naming") |
| `tab_close` | `tabId` | - (the Tab goes at once and its Session is killed, thawed first if frozen; no confirmation: the webview asks, `src/lib/sidebar/closeTabFlow.ts`) |
| `tab_rename` | `tabId, title` | - (an empty title restores the automatic Title) |
| `tab_move` | `tabId, groupId, index?` | - (default: the end of the Group) |
| `tab_activate` | `tabId` | - |
| `group_new` | `name?, tabId?` | `Group` (at the end, "New Group" unless named; with `tabId` that Tab moves into it) |
| `group_rename` / `group_move` / `group_set_collapsed` | `groupId, name` / `groupId, index` / `groupId, collapsed` | - |
| `group_delete` | `groupId` | - (closes every Tab in it; rejects for the last Group) |
| `activity_watch` | `on: boolean` | - (start / stop sampling Activity) |
| `guard_state` | - | `GuardSnapshot`: Memory Guard's state |
| `guard_set` | `on: boolean, limitPercent: number` | `GuardSnapshot` (turn Memory Guard on or off, set its limit, clamped to 50..95; off thaws every Tab it froze, not those frozen by hand) |
| `guard_visible` | `sessionId: SessionId \| null` | - (the Session in view: never frozen, thawed if frozen) |
| `guard_freeze` | `sessionId` | `GuardSnapshot` (freeze a Session by hand, Memory Guard on or not; an error for the Session in view) |
| `guard_thaw` | `sessionId` | `GuardSnapshot` (thaw a Session without going to its Tab; spares it like going to it) |
| `usage_watch` | `on: boolean, agents: AgentKind[]` | - (start, change the agents of, or stop reading Usage) |
| `caffeinate_state` | - | `boolean`: whether Caffeinate is on |
| `caffeinate_set` | `on: boolean` | `boolean`: whether Caffeinate is on now |
| `resume_leftover` | - | `ResumeEntry[]`: what earlier runs left running, not yet resumed or dismissed (see "Resume") |
| `resume_forget` | `keys: string[]` | - (drop leftover entries: resumed, dismissed, or their Tab is gone) |
| `settings_load` / `settings_save` | `settings: json` | opaque JSON blob in the app data dir; one section per owner (`hotkeys`, `usage`, `activity`, `memoryGuard`, `sidebar`, `hosts`), merged by `src/lib/settings/store.ts` |
| `remote_state` | - | `RemoteSnapshot` after re-reading Tailscale's state (async: runs its CLI) |
| `remote_set` | `on: boolean` | `RemoteSnapshot` (turn Remote on or off; rejects with why the server could not start) |
| `remote_pair_begin` / `remote_pair_cancel` | - | `Pairing` (a code, its QR link and expiry) / - |
| `remote_revoke` | `id: string` | - (forget a paired phone) |

| Event | Payload | When |
|---|---|---|
| `layout` | `LayoutSnapshot` | the layout changed: a Tab or Group made, closed, renamed, moved, collapsed or activated, a Tab's Session or last cwd changed. The whole model each time, with a revision |
| `session-info` | `SessionInfo` | first probe of a Session, then on every change (monitor tick 500 ms): the Foreground process, agent, cwd, git, and the OSC title, BEL count and Agent status the Host read in its output |
| `session-exit` | `SessionExit` | the shell exited or was killed (its Tab is already gone from the layout) |
| `activity` | `ActivitySnapshot` | every 2 s while `activity_watch(true)`; the first right away |
| `usage` | `UsageSnapshot` | right away on `usage_watch(true, ..)`, then whenever a number changes (checked every 5 s) |
| `menu-settings` | - | the app menu's "Settings…" was chosen |
| `caffeinate` | `false` | Caffeinate's `caffeinate` run ended without being turned off |
| `remote` | `RemoteSnapshot` | Remote turned on or off, a phone connected, paired or was removed, a pairing began or ended |
| `memory-guard` | `GuardSnapshot` | Memory Guard froze or thawed a Tab, or was turned on or off |

Types: `src-tauri/core/src/model.rs` mirrored by `src/lib/types.ts`. Outside Tauri, `ipc.ts` routes to
`src/lib/mock.ts`, a fake backend for developing the UI in a browser (`pnpm dev`, then open
`http://127.0.0.1:1420`). Type `help` in a mock terminal.

## Rust modules

One Cargo workspace in `src-tauri/`: the core library, the app, and the daemon (ADR 0002). The
core never names Tauri; what it needs from its binary is `core/src/host.rs`, and each binary
answers it (see "Host daemon" for the daemon's answer).

**The core, `src-tauri/core/src/` (`sidebar_term_core`)**

- `host.rs` — what the core takes from its binary: `Events` (emit a named JSON event), `Paths`
  (the data dir), `Assets` (the phone page's files), an `OutputSink` per Session, and a tokio
  runtime handle, bundled as `Host`.
- `layout/` — the layout, owned by the Host (ADR 0002). `mod.rs`: `Layout`, the owner: loads
  `layout.json`, spawns each Tab's Session at launch (Tab id as Resume key) and on `tab_new`,
  kills it on `tab_close`, drops the Tab when its Session exits, emits `layout` on every change
  (under its lock, so snapshots arrive in order), rewrites the file 500 ms after the last change,
  takes each Session's facts from the monitor (`observe`: the Tab's last cwd; the facts are kept
  per live Session for clients, `facts` / `fact`), tells its watchers (Remote) of every change
  to the layout or to a Session's facts, and hands the webview's Terminal a Session's output
  (`attach`). `model.rs`: the pure state and every transition, tested without a pty. `file.rs`:
  the version-2 file, the defensive read, and the one-time move of a version-1 file's
  presentation fields into `settings.json`.
- `outlet.rs` — where a Session's output goes before a Terminal attaches: held, bounded, handed
  over first on attach under the same lock the reader delivers through.
- `session.rs` — `SessionManager`: spawn `$SHELL -l` on a `portable-pty` pty, one reader thread
  per Session coalescing output into chunks for the Session's `OutputSink` and its tap, write,
  resize, pause/resume, kill, `probe_targets()`, exit hooks (the layout's); `session-exit`
  through `Events`.
- `detect/` — `probe(&ProbeTarget) -> SessionInfo`: libproc for the Foreground process group,
  agent classification, remote-hop detection, cwd; `.git` file reading for repo / Worktree / branch.
  `detect/resume.rs`: the Resume entry of a Session's Foreground job (see "Resume"). The libproc
  readers are macOS-only; elsewhere they are stubs that read nothing until #26.
- `monitor.rs` — thread ticking every 500 ms: probe every target, add what the Host read in the
  Session's output (`Taps::marks`: the OSC title, BELs) and the Agent status (`status.rs`), hand
  the changed infos to the layout, emit `session-info` for each.
- `status.rs` — Agent status: Running / Needs input / Done from the agent kind, the title, when
  output last arrived and when the last BEL rang (the table in "Agent status"). Pure, the clock
  passed in; the cases `src/lib/agentStatus.ts` used to test are here.
- `activity.rs` — `Activity`: thread idle until watched (by the webview, or by Memory Guard), then
  every 2 s runs `/bin/ps` over every process, reads this user's processes' footprints, attributes
  each to a Session by ppid descent from its shell, emits `activity` if the webview watches and
  hands the sample to Memory Guard if it is on (see "Panel"). The `ps` / Mach / sysctl reading is
  macOS-only; elsewhere it reads nothing until #27.
- `guard.rs` — `Guard`: Memory Guard's policy and the SIGSTOP / SIGCONT of a Session's process
  tree; `frozen.json` for thawing after a crash (see "Tray").
- `usage.rs` — `Usage`: thread idle until watched, then every 5 s reads the chosen agents' usage
  limits and emits `usage` on change (see "Panel"). App-only: the daemon never starts it.
- `remote/` — Remote (see "Host protocol"): `mod.rs` the `Remote` state (on/off, pairing, the
  relay of layout and Session changes to clients, the upload dir), `server.rs` the axum routes
  and the WebSocket protocol on the Host's runtime, `tap.rs` each Session's recent output,
  attached clients and the `Scanner` that reads OSC 0 / 2 titles and BELs out of the output as
  it passes (fed by `session.rs`), `auth.rs` paired clients and pairing codes (`remote.json`),
  `tailscale.rs` the Tailscale CLI.
- `resume.rs` — `Resume`: a thread records every keyed Session's Resume entry to `resume.json`
  each second it changes, and a last time on exit (see "Resume").
- `store.rs` — atomic JSON read/write of `layout.json`, `settings.json`, `resume.json`,
  `remote.json` and `frozen.json` in the Host's data dir (`Paths`).
- `paths.rs` — which paths printed in a Terminal name a file on this Host.
- `model.rs` — the types every event and command carries; mirrored by `src/lib/types.ts`.

**The app, `src-tauri/src/` (`sidebar_term_lib`, Tauri)**

- `lib.rs` — the IPC commands (the table above), the app menu (Tauri's default plus "Settings…";
  no key equivalent: the Settings Hotkey stays the webview's, rebindable), and the wiring of the
  core at startup and exit.
- `host.rs` — `AppHost`: the core's `Host` with an `AppHandle` behind it (events to the webview,
  the app-data dir, the bundled assets, Tauri's tokio runtime).
- `caffeinate.rs` — `Caffeinate`: the background `caffeinate` run behind the Tray's Caffeinate
  button (see "Tray"). App-only.
- `drop.rs` — files dropped on a Terminal: the drag pasteboard and the temp-dir save (see
  "Window"). App-only.

**The daemon, `src-tauri/daemon/src/main.rs` (`sidebar-termd`)**: see "Host daemon".

## Webview modules

- `src/lib/terminal/manager.ts` — `terminals`: one xterm.js `Terminal` per Session on any Host,
  keyed by `SessionKey` (`src/lib/host/ids.ts`: Host and Session id), attached to the Session's
  output through a `SessionTransport` as the layout names it (`localTransport`, IPC, for this
  Mac's Host; `hostTransport` from `src/lib/host/hosts.svelte.ts` for a paired one), mount only
  the one in view, WebGL on the mounted Terminal with DOM fallback, fit and resize, flow control,
  dropped files through the transport, title/bell events.
- `src/lib/terminal/TerminalPane.svelte` — shows the Session in view's Terminal.
- `src/lib/layout.svelte.ts` — the mirror of every Host's layout: the local Host's (`layout_get`,
  then every `layout` event) as `layout.groups`, each paired Host's as a section
  (`layout.sections`, fed by `src/lib/host/hosts.svelte.ts`), every Host's Tabs in `layout.tabs`
  (each knows its Host), older revisions ignored per Host; the actions that send the `tab_*` /
  `group_*` commands to the right Host; plus this client's own state: the Tab in view (on any
  Host), sidebar width and visibility, the Panel, the user's unread marks (persisted, debounced,
  as the `sidebar` settings section: `src/lib/sidebar/settings.ts`).
- `src/lib/sessions.svelte.ts` — reactive `SessionInfo` per Session on any Host (the Host's Agent
  status included) plus this client's markers (finished, highlight) and the automatic Title
  (with each Host's home for `~`).
- `src/lib/hotkeys.ts` — Hotkey actions, defaults and the pure rules for combos;
  `src/lib/hotkeys.svelte.ts` — the live bindings (persisted overrides); `src/lib/shortcuts.ts` —
  the window listener that dispatches them.
- `src/lib/settings/*` — the Settings page (Usage agents, Remote, Hosts, Memory, Hotkeys), shown
  over the Terminal; the settings blob's per-section store (`store.ts`); `HostsSection.svelte`
  pairs with a Host and lists the paired ones.
- `src/lib/sidebar/*` — sidebar components: Group headers and Tab rows (the same for every
  Host's), `HostHeader.svelte` above each paired Host's Groups, drag-and-drop (never across
  Hosts), the close-Tab flow. `src/routes/+page.svelte` — app shell.
- `src/lib/host/*` — a Host as a client sees it: `protocol.ts` (below), `client.ts` (the
  connection, both clients'), `ids.ts` (`HostId`, `SessionKey`), `settings.ts` (the `hosts`
  section and a Host's URL, pure), `hosts.svelte.ts` (the Mac app's paired Hosts: one connection
  each, their state, the transport and commands for a Host; see "Hosts"), `connect.ts` (the real
  connection, or `mock.ts`'s fake Host under `pnpm dev`).
- `src/lib/tray/*` — the Tray (`Tray.svelte`), its `TrayButton`, Caffeinate (state mirror and
  button) and the Memory Guard button.
- `src/lib/guard/*` — Memory Guard's state mirror, settings and visible-Session reporting
  (`memoryGuard.svelte.ts`) and pure rules (`model.ts`).
- `src/lib/resume/*` — the Resume banner (`ResumeBanner.svelte`), its state and actions
  (`resume.svelte.ts`) and the pure rule for what to type (`model.ts`).
- `src/lib/remote/*` — Remote on the Mac: the state mirror (`remote.svelte.ts`); the Settings
  section is `src/lib/settings/RemoteSection.svelte`.
- `src/lib/host/protocol.ts` — the Host protocol's message types, output framing and reply
  correlation (`Replies`), pure and tested; every client of a Host imports from here.
- `src/lib/mobile/*` + `src/routes/m` — the phone's page: its state (`store.svelte.ts`: the
  connection from `src/lib/host/client.ts`, the Host's layout and Session facts, mirrored), what
  a row shows (`rows.ts`, pure), font fitting (`fit.ts`) and the screens (pairing, Tab list,
  `TerminalScreen` with `KeyBar`). `src/service-worker.ts` caches it.
- `src/lib/panel/*` — the Panel (`Panel.svelte`), its view list (`views.ts`), the Activity view
  (`activity/`: snapshot store, the Tab stats setting, pure sorting / formatting / meter maths,
  components) and the Usage
  view (`usage/`: snapshot store, chosen agents, pure formatting, components).

## v1 product defaults (provisional)

- **Scope** (#7): one window, no split panes, no profiles, no settings UI beyond Usage agents and Hotkeys, no quick
  switcher. Tabs move between Groups by drag-and-drop and by a context menu.
- **Persistence** (#8): Groups (name, order, collapsed), Tabs (order, custom Title, last cwd) and
  the active Tab persist in the Host's `layout.json`; the sidebar width, the Panel (view,
  collapsed, height) and the user's unread marks in the webview's `settings.json` (`sidebar`).
  On relaunch the Host respawns every Tab's shell at its last cwd.
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
  / next Tab across all Groups, Cmd-Opt-Up/Down move Tab, Cmd-Shift-U mark the active Tab unread,
  Cmd-B toggle sidebar, Cmd-, Settings
  (also the app menu's "Settings…"; the sidebar has no Settings button).
  These are defaults: every one is a Hotkey the user can rebind on the Settings page
  (`src/lib/hotkeys.ts` holds the actions and rules; overrides persist in `settings.json` next to
  `layout.json`). A Group header shows its go-to-Group Hotkey and its Tab count as `NAME (2)  ⌘1`.
  With paired Hosts in the sidebar, next / previous Tab walk every section in order; go-to-Group
  numbers count the local Host's Groups only, and a Host's Group headers show no Hotkey (see
  "Hosts").
  Closing a Tab whose Foreground process is not the shell asks for confirmation in an in-app dialog
  (never `window.confirm`). Sidebar width is draggable.
- **Architecture** (#14): as above; ADR `docs/adr/0002-host-daemon-owns-sessions-and-layout.md`
  (0001 is superseded).

## Agent status

Derived on the Host (`core/src/status.rs`) from `SessionInfo.agent`, the Session's OSC 0 / 2
title, BEL and output activity, which the Host reads out of the Session's output as it passes
its tap (`remote/tap.rs`, `Scanner`: the same bytes xterm.js parses, so no Terminal is needed;
sequences split across reads are joined). The monitor puts the title, the BEL count and the
status in every `SessionInfo`, re-derived each tick so Claude Code's time-based Running window
expires on its own, and every client shows that status: the Mac webview, a phone, and whatever
shows a headless Host's Tabs. `src/lib/agentStatus.ts` keeps only the Title rules. The rules
(sources: `docs/research/agent-detection.md`):

| Status | Codex | Gemini | Claude Code |
|---|---|---|---|
| Running | title starts with a braille spinner char (U+2800-U+28FF) | title starts with `✦` | title starts with `◐` or `◑` |
| Needs input | title contains `Action Required` | title starts with `✋` | BEL while the agent is foreground |
| Done | title without spinner / after activity stops | title starts with `◇` | title starts with `✳` |

Claude Code (2.1.267) prefixes its title with `◐`/`◑` while busy, alternating every ~1 s while its
terminal is focused, frozen on one frame otherwise. It uses `✳` when idle or waiting on a prompt.
Read from its bundled source. There is no prefix under tmux (always `✳`), with
`CLAUDE_CODE_DISABLE_TERMINAL_TITLE`, or in older versions. With no prefix we fall back to "output
within the last ~3 s", which keystroke echo and redraws also trip; "when output last arrived"
moves at most every 250 ms, so a redraw right after a BEL does not cancel Needs input.

The Tab's icon slot shows the status: a spinner while Running, the robot once stopped (amber for
Needs input). When the agent exits, the Tab stops being an Agent session; if that happens while the
Tab is not active, the icon is a check until the Tab is next activated. A Done or Needs-input status
on a background Tab is highlighted until the Tab is activated.

A Tab reads as unread (bold Title) while it has one of those markers or the user has marked it
unread (the Tab's context menu, or Cmd-Shift-U for the active Tab). The user's mark shows on the
active Tab too, persists in the layout, and clears the next time the user goes to the Tab (not on
relaunch). "Mark as Read" in the context menu clears every marker.

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
header shows CPU and Memory Used instead. Hovering the muted memory segment breaks it down into
macOS's wired memory, the compressor and other apps.

With "CPU and memory on Tabs" on in Settings (the default), each Tab row shows its Session's CPU and
memory (`35% · 1.21 GB`), from the same samples.

- Source: `/bin/ps -axo pid,ppid,rss,time,%cpu,comm`, every 2 s, only while the Panel or the Tab
  stats show it or Memory Guard is on. libproc's task info is EPERM for other users' processes, about a
  third of all processes and usually the busiest (WindowServer, kernel_task); `ps` is setuid root.
  One run costs ~20 ms.
- Memory is each process's physical footprint (`proc_pid_rusage`), Activity Monitor's "Memory"
  column: compressed and swapped pages included, shared pages not. `ps`'s resident size gets both
  wrong (a shared executable counts in full; what the compressor holds does not count, so under
  pressure a 200 MB process reads 5 MB). Footprints are readable for this user's processes, which
  includes every Session's; other users' processes fall back to resident size.
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

## Tray

The Tray is a row of small icon buttons and indicators at the top of the sidebar, in the titlebar
row, right of the traffic lights (which it never runs under: `--traffic-lights-width`). It shows
whenever the sidebar does, whatever its width and the Panel's state. Items are listed in order in
`src/lib/tray/Tray.svelte`; a button is a `TrayButton` (muted, lit in `--tray-on` while a toggle is
on). Items so far: Caffeinate, Memory Guard.

**Caffeinate** keeps this Mac awake while on: Rust runs `/usr/bin/caffeinate -d -i -w <app pid>`
as a hidden child of the app, in no Session, so no Terminal shows it (`-d` display, `-i` idle sleep;
`-w` ends it with the app, a crash included). Off at launch, not persisted, and a webview reload
leaves it as it was (the webview reads `caffeinate_state` at startup). If the run ends on its own
(`killall caffeinate`), the next 1 s check turns Caffeinate off and sends `caffeinate`.

**Memory Guard** freezes the Tab using the most memory when memory gets tight, and thaws it once
memory frees up, so heavy Tabs take turns instead of making the Mac swap. The button is lit while on
and shows how many Tabs it has frozen; a frozen Tab shows a snowflake. On/off and the limit persist in
settings (`memoryGuard`); the limit is set on the Settings page. The policy is `guard.rs`, run on
every Activity sample while on:

- Memory Used over the limit (default 85% of physical memory) for 4 s: freeze the Session using the
  most memory (sum of footprints), if at least 128 MB. Never the Session in view, reported by the
  webview (`guard_visible`).
- Memory Used 10 points under the limit: thaw the Session frozen first.
- After a freeze or thaw, wait 10 s before the next, so memory shows the effect.
- Going to a frozen Tab thaws it and spares it until memory falls under the thaw line.
- The Tab context menu's Freeze freezes a background Tab by hand (`guard_freeze`), whether Memory
  Guard is on or not; Thaw (`guard_thaw`) or going to the Tab thaws it. Memory Guard never thaws a
  Tab frozen by hand, nor does turning it off, and the Tray count leaves such Tabs out.
- Freezing is SIGSTOP to the shell first, then its descendants, parents first (a stopped foreground
  job would make the shell take the tty back, "zsh: suspended"); thawing is SIGCONT in reverse, the
  shell last. A frozen process keeps its memory: freezing stops growth and CPU, not what is held.
  An agent's in-flight request or a command it waits on may time out after a long freeze; the agent
  retries.
- A frozen Session is thawed before a kill (closing its Tab, a reload, quitting), since SIGHUP does
  not reach a stopped process. After a crash, `frozen.json` (pids with start times) lets the next
  launch thaw what was left stopped; a pid whose start time changed is not touched.

## Resume

When the app closes with Tabs still running something, the next launch offers to start it again
in the same Tabs. Every way of closing counts: a crash, Cmd-Q, `pnpm app:install`'s restart, a
webview reload. A close with every shell at its prompt offers nothing.

**Recording** (Rust, `resume.rs` + `detect/resume.rs`). The layout spawns each Tab's Session with
the Tab id as its Resume key. Every second a thread works out each keyed Session's `ResumeEntry` (`kind`,
`line`, `cwd`) from its Foreground process group, and rewrites `resume.json` when the list
changed. On `RunEvent::Exit` Rust records once more, before killing the shells, then writes no
more, so the dying shells cannot empty it. A crash leaves the last second's list. Rust, not the
webview, keeps it because only Rust is still running at exit: the webview's debounced save would
lose a job that ended in the last half second, and see the shells die.

- **Claude Code** (`kind: "claude"`): Claude Code 2.1+ writes `<config dir>/sessions/<pid>.json`
  for each running instance (`sessionId`, `cwd`, `kind`), and rewrites it as the conversation
  changes (it follows `/clear`). The config dir is the process's own `$CLAUDE_CONFIG_DIR`, else
  `$HOME/.claude`. Line: `claude --resume <sessionId>`, with the flags that shape a session carried
  over (`--dangerously-skip-permissions`, `--model`, `--permission-mode`, `--effort`, `--agent`,
  ...), in the file's `cwd`: Claude Code only finds a conversation from the directory it started in.
  No file (older versions, `--print`) means no entry.
- **Anything else** (`kind: "command"`): the argv of each pipeline stage (the group's members
  whose parent is the shell), quoted for zsh/bash and joined with ` | `, in the first stage's
  cwd. A script run through its `#!` line (`node /opt/homebrew/bin/npm run dev`) is typed as its
  name (`npm run dev`) when that name finds the same file on the process's `PATH` (read from its
  environment; Apple's platform binaries withhold theirs, so they stay as run).
- **No entry**: a shell at its prompt; Codex and Gemini (not resumable by id yet); a job of
  another uid (`sudo`: argv unreadable); a line with control characters or over 4 KiB.
  Environment variables set on the command line (`PORT=1 npm run dev`) are not recovered.

**Leftover**. `resume.json` holds `running` (this run) and `leftover`. At launch and on
`session_reset`, `running` moves into `leftover`, replacing older entries per key. Leftover stays
until the webview forgets it (resumed, dismissed, Tab gone), so quitting again before acting on
the banner loses nothing.

**Resume banner** (webview, `src/lib/resume/`). After `initLayout`, `resume_leftover` gives the
rows; entries whose Tab is gone are forgotten. The banner sits under the Terminal, which gives it
its height and refits. It shows the Tab's Title and the line per row. Buttons: "Resume Claude
Code" (every `claude` entry), "Rerun commands" (every `command` entry), per row "Resume" /
"Rerun", and Dismiss. Resuming types into the Tab's shell: Ctrl-U (clear the prompt), then
`cd -- <cwd> && ` when the shell is elsewhere, the line, Enter. It first re-probes the Session
and never types into one that is not at its prompt: that row stays, marked Busy. An entry is
dropped once its Tab closes or becomes an Agent session (resumed by hand). Other jobs do not
count, since shell startup files run commands too.

## Host protocol

How a Host serves its Sessions, Tabs and Groups to a client (ADR 0002; vocabulary in
`CONTEXT.md`): the phone's page today, the Mac app for a remote Host next (#29). Everything a
client needs to show a Host's sidebar and drive it comes from the Host's core, never from
another client. Remote is the switch: off by default, on in Settings (the daemon turns it on
at launch).

**Server** (`src-tauri/core/src/remote/`). `remote_set(true)` binds `127.0.0.1:<port>` (47611 unless
`remote.json` says otherwise; never a LAN address) and runs axum on the Host's tokio runtime. It
then asks Tailscale to publish it: `tailscale serve --bg --https=443 http://127.0.0.1:<port>`,
which gives `https://<host>.<tailnet>.ts.net` with a real certificate, reachable only from the
tailnet (Funnel is never used). The CLI is looked for in the Tailscale app and Homebrew. Without
Tailscale the server still listens on localhost and Settings says what is missing. `enabled`
persists in `remote.json`, so Remote comes back on at launch. Turning off closes every client
(close code 1001), removes the Serve rule and ends the pairing.

Routes: `/` → `/m`; `/m…` → `index.html` (the SPA routes to `src/routes/m`); other paths are
built assets through `host::Assets` (the app: Tauri's asset resolver, embedded in a release
build, `../build` on disk in dev, so run `pnpm build` first; the daemon: `--web-root`).
`POST /api/pair {code, name}` pairs a client. `POST /api/upload` (multipart, `Authorization:
Bearer <token>`) writes the first file part under `<data dir>/uploads/<stamp>/<name>` (the name's
final component only) and answers `{path}`, so a screenshot dragged onto a remote Tab can be
attached by path as `drop.rs` does locally; 64 MiB at most. Both answer CORS preflights for the
Mac app's webview only (`tauri://localhost`, and `http://localhost:1420` in dev), since its page
is another origin than the Host; what admits a client is still the code, then the token. `GET
/ws` is the connection.

**Messages** (`src/lib/host/protocol.ts` mirrors `remote/server.rs`). Text frames are JSON
tagged by `t`, camelCase fields; output is binary. The first text frame must be `auth` within
five seconds, or the Host closes with 4408 (4401 for a token it does not know).

| From the client | Fields | The Host answers |
|---|---|---|
| `auth` | `token` | `hello`, or a close |
| `attach` | `sessionId` | `attached {sessionId, cols, rows}`, then a binary replay of the Session's recent output; `exit` for a Session that is gone |
| `detach` | `sessionId` | - |
| `input` | `sessionId, data` | `error {message}` if the write failed |
| `ping` | - | `pong` |
| `resize` | `id, sessionId, cols, rows` | `ok` / `error`: sizes the pty, only for a client attached to the Session with no other client on the socket attached and no Terminal attached in process (the Mac webview's); the phone never sends it |
| `tab_new` | `id, groupId?, afterTabId?, cwd?, cols?, rows?` | `ok {result: Tab}` |
| `tab_close` / `tab_rename` / `tab_move` / `tab_activate` | `id, tabId` (+ `title` / `groupId, index?`) | `ok` |
| `group_new` | `id, name?, tabId?` | `ok {result: Group}` |
| `group_rename` / `group_move` / `group_delete` / `group_set_collapsed` | `id, groupId` (+ `name` / `index` / `collapsed`) | `ok` |

The commands are the layout's (the IPC table above), one to one, and take a client-chosen `id`
(a number) that the reply echoes: `ok {id, result?}` or `error {id, message}`. What they change
arrives as `layout` like any other change, to every client.

| From the Host | Fields | When |
|---|---|---|
| `hello` | `host {name, version, home}, device, layout, sessions` | right after `auth`: the whole layout (`LayoutSnapshot`) and every live Session's facts (`SessionInfo[]`); `device` is this client's name as the Host knows it; `home` is for the `~` in automatic Titles |
| `layout` | `layout` | the layout changed; the whole `LayoutSnapshot`, with its revision (an older one is ignored) |
| `session` | `session` | a Session's facts changed: `SessionInfo` whole (Foreground process, agent, cwd, git, remote, the OSC title, the BEL count, Agent status) |
| `activity` | `sessions` | each Session's CPU and memory (`ActivitySession[]`), every sample while the Host samples Activity (the Mac's Panel or Memory Guard on; the daemon does not yet) |
| `attached` / `resized` | `sessionId, cols, rows` | after `attach`; the pty was resized |
| `exit` | `sessionId` | an attached Session ended (its Tab left the layout with it) |
| `ok` / `error` | `id, result?` / `id, message` | a command's reply |
| `error` | `message` (no `id`) | a message the Host could not read, or `input` failed |
| `pong` | - | after `ping` |
| binary frame | a big-endian u32 Session id, then the bytes | the Session's output, live, after the replay |

Close codes: 4401 unknown token, 4408 no auth in time, 4429 the client fell too far behind
(reconnect and replay), 1001 Remote turned off. `layout` and `session` carry no state of their
own on the hub: a connection reads the latest from the layout when it relays, so a slow client
sees the newest, and one that lags past the hub's buffer gets the whole layout and every
Session's facts again.

**Taps** (`remote/tap.rs`). `session.rs` gives every Session's output to `Taps` as well as to
its outlet: a 256 KiB ring of recent output (trimmed to a line so a replay does not start
inside an escape sequence), the pty's size (from spawn and every resize), the clients attached,
and a `Scanner` that reads OSC 0 / 2 titles and BELs out of the bytes as they pass (see "Agent
status"). An attach reads the ring and registers the subscriber under one lock, so nothing falls
between the replay and the live frames. A client that falls 512 frames behind is dropped and
reconnects (close 4429); the connection's 5 s ping notices.

**Access**. Unchanged: two gates, the tailnet (Tailscale's own device identity and WireGuard),
then a token (`remote/auth.rs`). Pairing: `remote_pair_begin` makes an 8-character code
(32-symbol alphabet, 40 bits, ten minutes, five wrong tries) shown in Settings as a QR code of
`<url>#pair=<code>` and as text. The client posts it with its name and gets a 256-bit token;
`remote.json` stores its SHA-256. Every WebSocket sends the token first, every upload carries it
as a bearer; Settings lists paired phones with when each was last seen and removes them.
Tailscale Serve's `Tailscale-User-Login` header is recorded on the pairing for display only: a
local process could set it, so it is never what admits a client. The pairing endpoint and the
page are reachable without a token by design (the page has no secrets).

**The phone** (`src/routes/m`, `src/lib/mobile/`). `store.svelte.ts`: paired or not (token in
`localStorage`), the connection (`src/lib/host/client.ts`, shared with the Mac app's Hosts: one
WebSocket to the Host's `/ws`, backoff 1–15 s, re-attaches what was attached, tries at once when
the page returns to the foreground, pairs each command with its reply, uploads to the Host's
`/api/upload`), the Host's layout and Session facts mirrored as the Mac's webview mirrors its local
Host's, the open Tab. What a row shows is derived from those (`rows.ts`): the Title as the Mac
derives it (a rename, else the agent's name, the OSC title of a running program, the Foreground
process, the cwd's basename with `~` for the Host's home), the agent and its status from the
Host, the Badge. Unread is per client and the phone keeps none. Screens: pairing (code prefilled
from the QR link), the Tab list (same icons and Badge as the Mac's rows, the Host's name as its
title), and `TerminalScreen`: an xterm.js Terminal at the Host's grid, `t.reset()` before each
replay, font size chosen so the Host's columns fit the width (`fit.ts`, from a measured cell;
below 6 px the grid scrolls sideways), the screen sized to the visual viewport so the key bar
(Esc, Tab, Shift-Tab, a one-shot Ctrl, arrows, ^C, Return; DECCKM-aware arrows) sits above the
keyboard. The phone never resizes the pty. `service-worker.ts` caches the page and assets
(registered only on `/m` over HTTPS; the Mac's webview never has it) and `manifest.webmanifest`
makes "Add to Home Screen" a full-screen app with its own icon; an installed page keeps its
storage, so the pairing lasts. The phone sends no layout command yet (UI: #16).

**Dev loop**. `pnpm build` (the server serves `../build`), then `pnpm tauri dev`; open
`http://127.0.0.1:<port>/m` in a browser. `SIDEBAR_TERM_REMOTE_PAIR=1 pnpm tauri dev` (debug
builds) starts a pairing at launch and prints its code and link, so a browser can pair without
clicking through Settings. Against the daemon: `sidebar-termd --data-dir <tmp> --port <n>
--web-root build --pair` prints the link; the Mac app pairs with it from Settings ("Hosts").
Under plain `pnpm dev` in a browser the sidebar shows a fake paired Host, "dell" (`mock.ts`,
URL `mock://dell`; type `offline` in one of its Tabs to see its section grey out and come back).

## Host daemon

`sidebar-termd` (`src-tauri/daemon/`) is the core with no window: a Host on a machine with no
display (ADR 0002; the Dell of #24), or a second Host on a Mac for a smoke test. It links the same
`sidebar_term_core` as the app and answers `core/src/host.rs` itself: events go on a broadcast
bus (which the log reads; Remote hears of changes from the layout directly), the data dir is
`$XDG_DATA_HOME/sidebar-term` (`~/.local/share/sidebar-term`), the phone page's assets are read
from a directory, and the Remote server runs on the daemon's own tokio runtime. On a Mac the
default data dir is the app's app-data dir with `daemon/` appended
(`~/Library/Application Support/com.bencooper.sidebarterm/daemon`), so a daemon and the app on
one Mac never read each other's `remote.json`, `resume.json` or `layout.json`.

**What it does at this stage (#25, #20).** At launch it starts the Session core, Resume, the
layout (its own `layout.json`: a Session is spawned for every Tab at its last cwd, or one Tab in
one Group on a fresh install, exactly as the app does), the monitor, loads `remote.json`, and
turns Remote on exactly as `remote_set(true)` does in the app: binds `127.0.0.1:<port>` and asks
Tailscale Serve to publish it. Clients get its layout and every Session's facts (Agent status
included) in `hello` and on every change, and drive it with the same commands as the app. `--port`
changes the port and keeps it in `remote.json`; `--web-root` names the built phone page (`pnpm
build`'s `build/`; default `<data dir>/web`, and without it `/m` is 404 while pairing and `/ws`
still work); `--pair` starts a pairing at launch and prints its code and link; SIGUSR1 starts
one at any time, so on a headless box `systemctl --user kill -s USR1 sidebar-termd` puts a code
in the journal. It logs to stderr (each layout change in one line). On SIGTERM (or SIGINT) it
writes the layout, records Resume entries a last time and hangs up every Session, as the app does
on quit; it does not turn Remote off, so Tailscale's Serve rule stays for the next run. Not
started, because a Host has no use for them yet: Activity and Memory Guard (#27 brings their
Linux reading), Usage and Caffeinate (app-only).

Nothing attaches to its Sessions' outlets, so each holds its last 256 KiB of output for good
(clients replay the tap instead). A client can drive its Sessions, create, close, rename and
move its Tabs and Groups, size a pty it alone shows, and upload files to it ("Host protocol").

**Linux.** `cargo build --release --bin sidebar-termd` builds only the core and the daemon (no
Tauri). The macOS-only reading in `detect/process.rs` and `activity.rs` is behind
`cfg(target_os = "macos")` with stubs elsewhere, so until #26 and #27 land a Linux Host reports
every Session as "shell at its prompt, no cwd, no Badge" and no Activity.
`packaging/systemd/sidebar-termd.service` runs it under `systemctl --user`; with
`loginctl enable-linger` it runs with no one logged in (README "Host daemon").

## Hosts

The Mac app as a client of other Hosts (#29, ADR 0002): each paired Host is a section of the
sidebar, with the Host's Groups and Tabs under it, and any of its Tabs opens as a live Terminal.
The Mac is its own local Host, reached in process; a paired Host is reached over the Host
protocol, and nothing about it comes from anywhere but its own core.

**Pairing** (`src/lib/settings/HostsSection.svelte`, `src/lib/host/hosts.svelte.ts`). Settings
lists the paired Hosts with their connection state and a Remove button, and pairs a new one from
its address (`https://dell.tail1234.ts.net`, `http://127.0.0.1:47611`; a pasted pairing link fills
both fields) and the code its Settings page shows or `sidebar-termd --pair` prints. The Host
answers with a token; it lands in the `hosts` section of this Mac's `settings.json`
(`src/lib/host/settings.ts`: `[{id, url, token, name}]`, `id` this client's own, `name` the
Host's as last heard) and is sent first on every connection. The Host records this Mac as "Mac
app" in its own list of paired clients. Removing a Host closes its connection and drops its
section; the Host still lists this Mac until it is removed there. A Host that refuses the token
(4401) stays listed as unpaired until removed and paired again.

**Connection.** One `RemoteClient` (`src/lib/host/client.ts`, the phone's too) per Host, from
launch, reconnecting with backoff 1–15 s and at once when the window comes back to the
foreground. `hello` brings the Host's name, home, whole layout and every Session's facts;
`layout` and `session` keep them current. A Host that is not connected keeps its section, greyed,
with its last snapshot (its Tabs cannot be driven until it is back; the header says why and a
click retries). On `hello` after a drop the Host may have restarted: Sessions it no longer has end
for their Terminals, and every Tab's Session is attached again, with a replay.

**Sidebar** (`src/lib/layout.svelte.ts`). The local Host's Groups come first with no header;
each paired Host follows, in Settings order, under a `HostHeader` (name, connection dot). Tab
and Group ids are random per Host, so every Host's Tabs share one map and a Tab knows its Host;
Sessions are keyed by Host and id (`SessionKey`). The Tab in view is this client's (`activeTabId`,
on any Host), apart from each Host's own active Tab: going to a Tab tells its Host (`tab_activate`),
so `tab_new` there lands next to it; a Host's snapshot moves the view only while the view is on
that Host (after a `tab_new` or a close, the Host's word on what shows next wins), or to a Tab
this client just made there. Another Host, or another client of the same Host, changing its
active Tab never pulls the view away. Decisions: next / previous Tab (`Cmd-Shift-[ / ]`) walk
every section; go-to-Group numbers (`Cmd-1..9`) count the local Host's Groups only, so they do
not shift as Hosts connect, and a Host's Group headers show no Hotkey. New Tab and New Group
(Hotkeys, the footer buttons) go to the Host of the Tab in view; a Group's context menu and a
Host header's go to theirs. Delete Group refuses a Host's last Group. Dragging a Tab or a Group
onto another Host's rows shows no drop target and drops nowhere: a Session cannot change
machines (Handoff, #30); "Move to Group" lists the Tab's Host's Groups only.

**Terminals** (`src/lib/terminal/manager.ts`). A paired Host's Session gets the same xterm.js
Terminal as a local one, through the Host's transport: `attach` (the Host replays its recent
output at its grid; the Terminal clears first, takes that grid while hidden, and refits when
shown), `input`, `resize` (the Mac sends it after every fit; the Host honours it only for the
one client attached, so a phone showing the Session leaves the Mac's request refused, which is
ignored), WebGL as local. No flow control: the Host's ring and drop-behind rules stand in for
`session_pause`. Files dropped on a remote Tab's Terminal are uploaded (`POST /api/upload`) and
their paths on the Host pasted, shell-escaped, as local drops are. Paths printed in a remote
Terminal are not links (they name files on the Host).

**Tab rows.** Identical to local, from the forwarded `SessionInfo`: icon, Agent status, Badge,
the automatic Title (with the Host's home for `~`), Unread by the same rules (its agent finished,
stopped or asked for input while the Tab was not in view; the user's marks persist by Tab id),
CPU and memory from the Host's `activity` messages while it samples (the daemon does not until
#27). Closing asks the same confirmation, from the forwarded Foreground process (the local Host
is re-probed on the spot). Not on a paired Host's Tabs: Freeze (Memory Guard is this Mac's; the
protocol has no freeze), Resume, the Panel's Activity.

**Mock.** `pnpm dev` in a browser fakes one paired Host, "dell", behind a fake connection
(`src/lib/mock.ts` `hostClient`, chosen by `src/lib/host/connect.ts` for a `mock://` URL): its
own layout in `localStorage`, a running fake agent, and `offline` typed in one of its Tabs drops
the connection for a few seconds. Any 8-character code pairs it again after a Remove.

## Window

`titleBarStyle: Overlay`, hidden title: the sidebar runs to the top of the window, leaves ~28 px for
the traffic lights, and marks its header `data-tauri-drag-region`. `dragDropEnabled: false` so HTML5
drag-and-drop works in the sidebar: on macOS Tauri's handler claims every drag, including the
webview's own. Files dropped on a Terminal therefore arrive as DOM `File`s; they paste as
shell-escaped paths read off the drag pasteboard, or saved to a temp dir when they have none
(`src/lib/terminal/drop.ts`, `src-tauri/src/drop.rs`). A pasteboard path in a `TemporaryItems`
staging dir, or one the app cannot open, counts as none. The screenshot thumbnail's drag carries its
staging copy (`TemporaryItems/NSIRD_screencaptureui_*`), which macOS keeps shells from reading, so
the screenshot is saved from the bytes WebKit received for its file promise.
