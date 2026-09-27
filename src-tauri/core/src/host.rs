//! What the core needs from the binary that links it, and nothing more (ADR 0002). The Mac app
//! answers with Tauri (`src-tauri/src/host.rs`: the webview gets the events, the app-data dir
//! holds the files, the bundled assets are the phone's page); `sidebar-termd` answers with an
//! event bus, an XDG data dir and a directory of built assets.
//!
//! The traits are object-safe on purpose: the core keeps `Arc<dyn Events>` and friends and hands
//! them to its threads.

use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;

/// Where the core's events go: `session-info`, `session-exit`, `activity`, `memory-guard`,
/// `remote`, ... (names and payloads in `model.rs`). The app forwards them to the webview; the
/// daemon puts them on its bus.
pub trait Events: Send + Sync + 'static {
    /// Deliver `payload` under `name`. Called from the core's threads (a Session's reader, the
    /// monitor, ...), so it must not block for long, and it must never panic.
    fn emit_value(&self, name: &str, payload: serde_json::Value);
}

impl dyn Events {
    /// Serialize `payload` and emit it. A payload that will not serialize is a bug in `model.rs`;
    /// it is logged and dropped rather than taking a core thread down.
    pub fn emit<T: Serialize + ?Sized>(&self, name: &str, payload: &T) {
        match serde_json::to_value(payload) {
            Ok(value) => self.emit_value(name, value),
            Err(e) => eprintln!("events: {name} payload does not serialize: {e}"),
        }
    }
}

/// Where the core keeps its files: `layout.json`, `settings.json`, `resume.json`, `remote.json`,
/// `frozen.json` (`layout.rs`).
pub trait Paths: Send + Sync + 'static {
    /// The data dir, created if missing. `Err` when there is none (then nothing persists and the
    /// core says so once, at startup).
    fn data_dir(&self) -> Result<PathBuf, String>;
}

/// One file of the phone's page, as the Remote server sends it.
pub struct Asset {
    pub bytes: Vec<u8>,
    /// A MIME type, `text/html` and the like.
    pub mime: String,
}

/// The phone's page and its built assets (`remote/server.rs`): `/index.html` and `/_app/...`.
pub trait Assets: Send + Sync + 'static {
    /// The file at `path` (absolute, `/`-rooted), or `None` when there is none.
    fn asset(&self, path: &str) -> Option<Asset>;
}

/// Where one Session's output goes, in order, in chunks of at most `session::CHUNK_MAX` bytes,
/// on the Session's reader thread. The app sends each chunk down the Tab's Tauri channel; the
/// daemon has no Terminal, so its sink is empty (phones read the Session's tap instead).
pub type OutputSink = Box<dyn FnMut(Vec<u8>) + Send + 'static>;

/// Everything the core takes from its binary, handed in once at startup and cloned into whatever
/// needs it.
#[derive(Clone)]
pub struct Host {
    pub events: Arc<dyn Events>,
    pub paths: Arc<dyn Paths>,
    pub assets: Arc<dyn Assets>,
    /// The tokio runtime the Remote server runs on (the app's is Tauri's; the daemon makes one).
    pub runtime: tokio::runtime::Handle,
}

#[cfg(test)]
pub(crate) mod testing {
    //! Events recorded for tests to read back.

    use super::*;
    use std::sync::Mutex;

    /// Records every event emitted through it.
    #[derive(Default)]
    pub struct Recorder {
        pub events: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl Events for Recorder {
        fn emit_value(&self, name: &str, payload: serde_json::Value) {
            self.events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((name.to_owned(), payload));
        }
    }

    impl Recorder {
        pub fn named(&self, name: &str) -> Vec<serde_json::Value> {
            self.events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .filter(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .collect()
        }
    }
}
