//! Background poller: every tick, probe each live Session and emit `EVENT_SESSION_INFO`
//! for Sessions whose `SessionInfo` changed since the last emit. OWNER: detection agent.

use crate::model::ProbeTarget;
use tauri::AppHandle;

/// Start the monitor thread. `targets` is called once per tick to get live Sessions.
/// Emit a `SessionInfo` for a Session the first time it is seen and whenever it changes.
/// Forget cached state for Sessions that disappear.
pub fn spawn<F>(_app: AppHandle, _targets: F)
where
    F: Fn() -> Vec<ProbeTarget> + Send + 'static,
{
}
