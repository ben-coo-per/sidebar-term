//! The sidebar-term core (ADR 0002): what a Host runs. Sessions (ptys) and the facts about them,
//! Memory Guard, Resume, and the Remote server phones connect to. One library linked into two
//! binaries: the Mac app (`src-tauri/src/lib.rs`, Tauri) and `sidebar-termd` (`daemon/`, headless).
//! See docs/architecture.md "Host daemon".
//!
//! Nothing here names Tauri. What the core needs from its binary comes through [`host`]: an
//! event emitter, the data dir, the phone page's assets and a tokio runtime, plus an output sink
//! per Session. The webview's layout (Groups, Tabs) is still the webview's (ADR 0001) until #20
//! moves it here.
//!
//! CONTRACT: `model.rs` is mirrored by `src/lib/types.ts`.

pub mod activity;
pub mod detect;
pub mod guard;
pub mod host;
pub mod layout;
pub mod model;
pub mod monitor;
pub mod paths;
pub mod remote;
pub mod resume;
pub mod session;
pub mod usage;
