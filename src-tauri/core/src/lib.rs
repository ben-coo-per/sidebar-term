//! The sidebar-term core (ADR 0002): what a Host runs. Sessions (ptys) and the facts about them,
//! the layout (Groups, Tabs, the active Tab), Memory Guard, Resume, and the Remote server phones
//! connect to. One library linked into two binaries: the Mac app (`src-tauri/src/lib.rs`, Tauri)
//! and `sidebar-termd` (`daemon/`, headless). See docs/architecture.md "Host daemon".
//!
//! Nothing here names Tauri. What the core needs from its binary comes through [`host`]: an
//! event emitter, the data dir, the phone page's assets and a tokio runtime. A client with a
//! Terminal attaches to a Session's output ([`outlet`]); every client mirrors the layout from
//! the `layout` event and changes it through [`layout::Layout`]'s commands.
//!
//! CONTRACT: `model.rs` is mirrored by `src/lib/types.ts`.

pub mod activity;
pub mod agents;
pub mod detect;
pub mod guard;
pub mod handoff;
pub mod host;
pub mod layout;
pub mod model;
pub mod monitor;
pub mod outlet;
pub mod paths;
pub mod remote;
pub mod resume;
pub mod session;
pub mod status;
pub mod store;
pub mod usage;
