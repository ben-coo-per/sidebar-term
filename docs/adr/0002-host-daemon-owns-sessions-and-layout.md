# 2. A headless Host daemon owns Sessions and the layout; clients own presentation

Status: **proposed** (epic #24; revise there). Supersedes 0001 once accepted.

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
Session, and the active Tab live in the core and persist in the Host's `layout.json`. The core
emits one `layout` snapshot event on change and exposes the layout commands (`tab_new`,
`tab_close`, `tab_rename`, `tab_move`, `tab_activate`, `group_*`) as pure state transitions with
Session spawn and kill as their only side effects. This is issue #20, done in the core.

**The Host serves Session facts.** `SessionInfo` (Foreground process, Agent session, cwd, git,
remote hop, and CPU / memory when Activity is on) is probed on the Host, where it is true, and
forwarded whole. Because a headless Host has no Terminal, the core also scans each Session's output
for OSC titles and BEL, so Agent status can be derived without a webview.

**Clients own presentation.** A client (the Mac webview, later the phone) owns sidebar width and
visibility, the Panel, automatic Titles, Unread, drag-and-drop mechanics, the close-Tab
confirmation, and which Hosts it is paired with. The Mac webview's layout `$state` becomes a mirror
of the local Host's snapshot plus its presentation state; a remote Host's snapshot is mirrored the
same way under a Host section.

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
- A Host's Sessions outlive any client. On the Dell the daemon is what keeps agents running; on the
  Mac the app is still the local Host, so quitting it still kills local Sessions (Resume covers
  that, as today).
- Unread is per client: a Tab the Mac has not looked at can be one the phone has.
- The wayfinder map's "macOS only" (#1) now applies to the app, not to the daemon.
- The phone gets create / close / rename / move for free once the commands are on the protocol,
  by adding UI; nothing is relayed through the Mac webview any more.
