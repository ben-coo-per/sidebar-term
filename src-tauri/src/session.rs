//! Session lifecycle: spawn a login shell on a pty, stream its output to the webview,
//! accept input, resize, pause/resume for flow control, and tear down.
//! OWNER: pty agent. See docs/research/pty.md and docs/architecture.md.

use crate::model::{ProbeTarget, SessionId};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::AppHandle;

/// Registry of live Sessions. Managed as Tauri state (`app.manage(SessionManager::default())`).
#[derive(Default)]
pub struct SessionManager {
    // pty agent: add fields (e.g. Mutex<HashMap<SessionId, Session>>, AtomicU32 next id).
}

impl SessionManager {
    /// Spawn the user's login shell (`$SHELL -l`, fallback `/bin/zsh`) on a new pty of `cols`x`rows`
    /// in `cwd` (fallback `$HOME`). Output bytes go to `on_data` as `InvokeResponseBody::Raw`.
    /// When the shell exits, emit `EVENT_SESSION_EXIT` with `SessionExit` via `app`,
    /// then drop the Session from the registry.
    pub fn spawn(
        &self,
        _app: AppHandle,
        _cwd: Option<String>,
        _cols: u16,
        _rows: u16,
        _on_data: Channel<InvokeResponseBody>,
    ) -> Result<SessionId, String> {
        Err("session::spawn not implemented".into())
    }

    pub fn write(&self, _id: SessionId, _data: &[u8]) -> Result<(), String> {
        Err("session::write not implemented".into())
    }

    pub fn resize(&self, _id: SessionId, _cols: u16, _rows: u16) -> Result<(), String> {
        Err("session::resize not implemented".into())
    }

    /// Park the reader thread so the kernel applies back-pressure to the child.
    pub fn pause(&self, _id: SessionId) -> Result<(), String> {
        Err("session::pause not implemented".into())
    }

    pub fn resume(&self, _id: SessionId) -> Result<(), String> {
        Err("session::resume not implemented".into())
    }

    /// SIGHUP the Session's process group and clean up. Idempotent for unknown ids.
    pub fn kill(&self, _id: SessionId) -> Result<(), String> {
        Err("session::kill not implemented".into())
    }

    /// One `ProbeTarget` per live Session. Called by the monitor every tick; must be cheap
    /// and must not hold locks across the returned value.
    pub fn probe_targets(&self) -> Vec<ProbeTarget> {
        Vec::new()
    }

    pub fn probe_target(&self, id: SessionId) -> Option<ProbeTarget> {
        self.probe_targets().into_iter().find(|t| t.session_id == id)
    }

    /// Kill every Session. Called on app exit.
    pub fn kill_all(&self) {}
}
