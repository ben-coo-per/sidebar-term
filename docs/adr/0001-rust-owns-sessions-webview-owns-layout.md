# 1. Rust owns Sessions; the webview owns the layout

Status: **superseded** by `0002-host-daemon-owns-sessions-and-layout.md` (epic #24, done for the
layout in #20): the layout moved into a core that a headless Host daemon owns too. Kept for the
reasoning; nothing below describes the code any more.

## Context

Two halves must agree on Sessions, Tabs and Groups. Either could be the source of truth.

## Decision

Rust owns only what needs the OS: ptys, shells, and facts derived from libproc and `.git` files.
It pushes those facts as `SessionInfo` events keyed by `SessionId`. The webview owns the whole
layout model (Groups, Tabs, Titles, order) and asks Rust only to store it as an opaque JSON blob.

## Consequences

- The layout model can change shape without touching Rust or IPC.
- Rust never needs to know a Tab exists; closing a Tab is "kill Session N".
- Session ids are not persisted. Relaunch respawns shells from each Tab's last cwd.
- Resume (see `docs/architecture.md` "Resume") needs Rust to say which Session was running what
  as the app died, after the webview can no longer save. The webview gives each Session an opaque
  key at spawn (its Tab id); Rust records Resume entries by that key and never interprets it.
- A second window would need the layout model moved or shared; out of v1 scope anyway.
