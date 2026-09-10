//! sidebar-term: Tauri shell. Rust owns Sessions (ptys) and the facts about them;
//! the webview owns the sidebar layout. See docs/architecture.md.
//! CONTRACT: command names and signatures here are mirrored by `src/lib/ipc.ts`.

mod detect;
mod layout;
mod model;
mod monitor;
mod session;

use model::{SessionId, SessionInfo};
use session::SessionManager;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager, RunEvent, State};

#[tauri::command]
fn session_spawn(
    app: AppHandle,
    sessions: State<'_, SessionManager>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
) -> Result<SessionId, String> {
    sessions.spawn(app, cwd, cols, rows, on_data)
}

#[tauri::command]
fn session_write(sessions: State<'_, SessionManager>, session_id: SessionId, data: String) -> Result<(), String> {
    sessions.write(session_id, data.as_bytes())
}

#[tauri::command]
fn session_resize(sessions: State<'_, SessionManager>, session_id: SessionId, cols: u16, rows: u16) -> Result<(), String> {
    sessions.resize(session_id, cols, rows)
}

#[tauri::command]
fn session_pause(sessions: State<'_, SessionManager>, session_id: SessionId) -> Result<(), String> {
    sessions.pause(session_id)
}

#[tauri::command]
fn session_resume(sessions: State<'_, SessionManager>, session_id: SessionId) -> Result<(), String> {
    sessions.resume(session_id)
}

#[tauri::command]
fn session_kill(sessions: State<'_, SessionManager>, session_id: SessionId) -> Result<(), String> {
    sessions.kill(session_id)
}

/// On-demand probe, e.g. to decide whether closing a Tab needs confirmation.
#[tauri::command]
fn session_info(sessions: State<'_, SessionManager>, session_id: SessionId) -> Option<SessionInfo> {
    sessions.probe_target(session_id).map(|t| detect::probe(&t))
}

#[tauri::command]
fn layout_load(app: AppHandle) -> Result<Option<serde_json::Value>, String> {
    layout::load(&app)
}

#[tauri::command]
fn layout_save(app: AppHandle, layout: serde_json::Value) -> Result<(), String> {
    layout::save(&app, &layout)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(SessionManager::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let for_targets = handle.clone();
            monitor::spawn(handle, move || for_targets.state::<SessionManager>().probe_targets());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            session_spawn,
            session_write,
            session_resize,
            session_pause,
            session_resume,
            session_kill,
            session_info,
            layout_load,
            layout_save,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            handle.state::<SessionManager>().kill_all();
        }
    });
}
