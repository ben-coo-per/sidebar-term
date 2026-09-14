# 2. Remote clients attach through the Mac app, over Tailscale, as a web page

Status: **accepted** (issue #16; the v1 scope is attach-and-drive, full control follows #20)

## Context

Work should keep going from a phone: see the sidebar, open any Session, type, answer an agent's
prompts. Three things had to be decided: how the phone gets the app, how it reaches the Mac, and
what serves it.

- **Getting the app on the phone.** Every way to sideload a native iOS app expires: a free Apple
  ID build after 7 days, a paid certificate after a year, TestFlight after 90 days. A web page
  added to the home screen is the one route that is permanent, needs no App Store and no signing,
  and is the same Svelte code as the Mac's webview. It needs HTTPS with a certificate the phone
  trusts.
- **Reaching the Mac.** A shell reachable from the internet is the wrong default whatever the
  login in front of it. Tailscale gives every device a private address reachable from any network,
  a WireGuard tunnel end to end, and (`tailscale serve`) an HTTPS name with a real certificate,
  visible only to the tailnet. Cloudflare Tunnel + Access would work from a browser with no VPN,
  at the price of putting the shell one login away from the open internet.
- **What serves it.** Sessions live in the Mac app's process (ADR 0001). A separate daemon would
  keep Sessions alive with the app closed, but means moving the ptys out of the app: out of scope.

## Decision

The Mac app grows a server (`src-tauri/src/remote/`): axum on Tauri's tokio runtime, bound to
**127.0.0.1 only**, off by default. Tailscale Serve publishes it to the tailnet as
`https://<mac>.<tailnet>.ts.net`; the app never binds a LAN address and never uses Funnel. The
phone's page is `/m`, the same SvelteKit build, with a route of its own (`src/lib/mobile/`) and
a service worker so it installs from Safari's "Add to Home Screen".

Access needs two things: being on the tailnet, and a **token** from a one-time pairing (a code
shown in Settings, presented once by the phone; the token is random, stored hashed, revocable in
Settings). Tailscale's identity headers are recorded on the pairing for display, not trusted: a
local process can forge them.

Rust learns nothing about Tabs. Each Session gets an output **tap** (a ring of recent output, its
pty size, and the phones attached); the phone attaches to a Session by id and types into it. The
sidebar the phone lists (Groups, Tabs, Titles, Agent status) is a snapshot the Mac webview
publishes whenever it changes (`remote_sidebar`), relayed opaque. The phone renders at the Mac's
grid and never resizes the pty, so attaching disturbs nothing on the Mac.

## Consequences

- Nothing works with the Mac app closed: no daemon in v1 (open question in #16).
- The phone can drive Sessions but not create, close, rename or move Tabs: those mutate the
  webview's layout, which Rust does not own. #20 moves the layout into Rust; full control follows.
- Two viewers share one pty at the Mac's size; a phone in portrait shrinks the font to fit ~80+
  columns, which is legible for agent prompts and small edits, not for long work.
- Without Tailscale installed and signed in on both ends, Remote listens on localhost only and
  says so in Settings; the pairing link then only works from a browser on the Mac.
- A future headless daemon or a native client can reuse the protocol
  (`src/lib/mobile/protocol.ts`), which is independent of the page.
