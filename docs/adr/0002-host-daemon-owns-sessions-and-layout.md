# 2. A headless Host daemon owns Sessions and the layout; clients own presentation

Status: **proposed** (epic #24; revise there). Supersedes 0001, which is marked so: the layout
part of this decision is built (#20, after #25 split the core), and the code follows this ADR.

## Context

A second machine, a Dell Mini running Linux with no display, will run Sessions that the Mac app
must show and drive as if they were local: same sidebar, same icon and Agent status, same Unread,
same Badge, plus creating, closing, renaming and moving Tabs there, and handing a Tab off from the
Mac to it. Driving that machine from the laptop is a higher priority than the phone (#16).

ADR 0001 put ptys in Rust and the whole layout model (Groups, Tabs, order, Titles, the active Tab)
in the webview, so the layout could change shape without touching Rust. That held for one window
on one machine. It fails for a Host without a display: there is no webview to own the layout, and
the `worktree-mobile` branch already had to relay a webview-built sidebar snapshot to phones
(`remote_sidebar`), which only works while the Mac webview is alive.

The Rust core's dependence on Tauri is thin: an event emitter, an output channel per Session and
the app-data path. Its dependence on macOS is wider (libproc in `detect/`, `ps` and Mach calls in
`activity.rs`, one `ps` in `guard.rs`) but sits behind pure, tested logic in every module.

## Decision

**A Host is a machine running the sidebar-term core.** The core (Sessions, Session facts, Tabs,
Groups, the active Tab, Resume entries, the Remote server, output rings) is one Rust library linked
into two binaries: the Mac app (Tauri window, the local Host) and `sidebar-termd`, a headless daemon
(systemd user service on Linux). The core talks to its binary through a small trait (events, output
sink, data dir), not through Tauri types.

**The Host owns the layout.** Groups, Tabs, their order, custom Titles, each Tab's last cwd and
Session, and the active Tab live in the core (`core/src/layout/`) and persist in the Host's
`layout.json` (version 2). The core emits one `layout` snapshot event on change (the whole model,
with a revision, so a client never shows an older one) and exposes the layout commands (`tab_new`,
`tab_close`, `tab_rename`, `tab_move`, `tab_activate`, `group_new`, `group_rename`, `group_move`,
`group_delete`, `group_set_collapsed`) as pure state transitions (`layout/model.rs`, tested
without a pty) with Session spawn and kill as their only side effects. This is issue #20, done in
the core. Decisions made there:

- A Tab's Session is spawned by the Host as the Tab is made (at launch for every persisted Tab,
  with its Tab id as the Resume key; on `tab_new`), before any Terminal exists for it. A client
  with a Terminal then *attaches* to the Session's output (`session_attach` in the app, in
  process); what the Session printed before is held per Session (`core/src/outlet.rs`, bounded)
  and handed over first. Phones keep reading the tap. So the daemon spawns a shell per Tab at
  launch too, and one Tab on a fresh install, as the app does.
- `tab_close` takes the Tab out at once and kills its Session; a Session that exits on its own
  takes its Tab with it. Killing a Session directly is no longer a command.
- A webview reload replaces the Sessions the old page's Terminals were attached to (a Terminal's
  state is gone with the page; Resume records what they ran, as for a quit) and keeps the rest,
  so the app's first page keeps what the Host spawned at launch.
- What was in the webview's `layout.json` (version 1) but is presentation stays the client's, in
  `settings.json` under a `sidebar` section (sidebar width, the Panel, the user's unread marks),
  not in an opaque section of the Host's file: one file per owner. The Host's version-1 read moves
  those fields there once and rewrites the file as version 2.
- Automatic Titles, Agent status and "finished" stay the client's until the core scans output
  for OSC titles and BEL (#28). The Host derives what it can for phones: a rename, else the
  agent's name, the Foreground process, the cwd's basename.

**The Host serves Session facts.** `SessionInfo` (Foreground process, Agent session, cwd, git,
remote hop, and CPU / memory when Activity is on) is probed on the Host, where it is true, and
forwarded whole. Because a headless Host has no Terminal, the core also scans each Session's output
for OSC titles and BEL, so Agent status can be derived without a webview.

**Clients own presentation.** A client (the Mac webview, later the phone) owns sidebar width and
visibility, the Panel, automatic Titles, Unread, drag-and-drop mechanics, the close-Tab
confirmation, and which Hosts it is paired with. The Mac webview's layout `$state` is a mirror of
the local Host's snapshot plus its presentation state (`src/lib/layout.svelte.ts`; the
presentation state persists in `settings.json`, section `sidebar`); a remote Host's snapshot is
mirrored the same way under a Host section.

**One protocol for every client.** The Host protocol (from the `worktree-mobile` branch: WebSocket
over a localhost server that Tailscale Serve publishes tailnet-only, pairing codes and hashed
tokens, per-Session output rings with replay) carries the layout snapshot, Session facts, output
frames, input, resize, the layout commands and a file upload. The Mac app and the phone are both
clients of it. The local Host on the Mac is reached in-process, not over the socket.

**Handoff reuses Resume.** Moving a Tab to another Host kills the local Session after recording its
Resume entry and reruns that entry on the Host at a mapped cwd. Nothing is rsynced; code moves by
push and checkout. Whether a Claude Code conversation can be resumed on another machine is a
research question (#31), not an assumption.

## Consequences

- The layout model's shape is now a Rust and protocol concern; changing it touches the core,
  `model.rs`, `types.ts` and the protocol types. That is the price of one source of truth.
- `detect/` and `activity.rs` need a Linux backend (`/proc`) behind their existing logic (#26, #27).
  `caffeinate.rs`, `drop.rs` and `usage.rs` stay app-only.
- Session ids stay per Host run; Tabs persist by cwd and respawn at launch, as before, on each Host.
- The Host holds each Session's output until a Terminal attaches (256 KiB, then the oldest goes),
  which on the daemon is forever: the cost of one attach path for every client.
- Unread is per client, so the user's mark persists with the client (`settings.json`), not the Host.
- A Host's Sessions outlive any client. On the Dell the daemon is what keeps agents running; on the
  Mac the app is still the local Host, so quitting it still kills local Sessions (Resume covers
  that, as today).
- Unread is per client: a Tab the Mac has not looked at can be one the phone has.
- The wayfinder map's "macOS only" (#1) now applies to the app, not to the daemon.
- The phone gets create / close / rename / move for free once the commands are on the protocol,
  by adding UI; nothing is relayed through the Mac webview any more.
