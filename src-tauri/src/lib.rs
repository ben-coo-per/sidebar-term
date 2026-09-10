//! sidebar-term: Tauri shell. Rust owns Sessions (ptys) and the facts about them;
//! the webview owns the sidebar layout. See docs/architecture.md.
//! CONTRACT: command names and signatures here are mirrored by `src/lib/ipc.ts`.

mod activity;
mod detect;
mod drop;
mod layout;
mod model;
mod monitor;
mod session;
mod usage;

use model::{AgentKind, SessionId, SessionInfo, EVENT_MENU_SETTINGS};
use session::SessionManager;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::menu::{Menu, MenuItem, MenuItemKind, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

/// Id of the app menu's "Settings…" item.
const MENU_SETTINGS: &str = "settings";

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

/// Kill every Session and stop Activity and Usage reading. The webview calls this once at startup
/// so a webview reload does not leave the previous page's shells (or `ps` runs) going with nowhere
/// to send output.
#[tauri::command]
fn session_reset(
    sessions: State<'_, SessionManager>,
    activity: State<'_, activity::Activity>,
    usage: State<'_, usage::Usage>,
) {
    sessions.kill_all();
    activity.watch(false);
    usage.watch(false, Vec::new());
}

/// On-demand probe, e.g. to decide whether closing a Tab needs confirmation.
#[tauri::command]
fn session_info(sessions: State<'_, SessionManager>, session_id: SessionId) -> Option<SessionInfo> {
    sessions.probe_target(session_id).map(|t| detect::probe(&t))
}

/// Start or stop the Activity sampler; while on, `activity` fires every 2 s.
#[tauri::command]
fn activity_watch(activity: State<'_, activity::Activity>, on: bool) {
    activity.watch(on);
}

/// Start (or change the agents of) or stop reading Usage; while on, `usage` fires on each change.
#[tauri::command]
fn usage_watch(usage: State<'_, usage::Usage>, on: bool, agents: Vec<AgentKind>) {
    usage.watch(on, agents);
}

#[tauri::command]
fn layout_load(app: AppHandle) -> Result<Option<serde_json::Value>, String> {
    layout::load(&app, layout::LAYOUT)
}

#[tauri::command]
fn layout_save(app: AppHandle, layout: serde_json::Value) -> Result<(), String> {
    layout::save(&app, layout::LAYOUT, &layout)
}

#[tauri::command]
fn settings_load(app: AppHandle) -> Result<Option<serde_json::Value>, String> {
    layout::load(&app, layout::SETTINGS)
}

#[tauri::command]
fn settings_save(app: AppHandle, settings: serde_json::Value) -> Result<(), String> {
    layout::save(&app, layout::SETTINGS, &settings)
}

/// Paths of the files on the macOS drag pasteboard, i.e. those of the drop just received.
#[tauri::command]
fn drop_paths() -> Vec<String> {
    drop::pasteboard_paths()
}

/// Save a dropped file that has no path (raw body, name in `x-file-name`); returns its new path.
#[tauri::command]
fn drop_save(request: tauri::ipc::Request<'_>) -> Result<String, String> {
    drop::save(request)
}

/// Tauri's default menu with "Settings…" in the app menu, after About, where macOS apps put it.
/// No key equivalent: the Settings Hotkey is the webview's, and rebindable (src/lib/hotkeys.ts).
fn app_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::default(app)?;
    if let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() {
        let settings = MenuItem::with_id(app, MENU_SETTINGS, "Settings…", true, None::<&str>)?;
        app_menu.insert_items(&[&settings, &PredefinedMenuItem::separator(app)?], 2)?;
    }
    Ok(menu)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(SessionManager::default())
        .menu(app_menu)
        .on_menu_event(|app, event| {
            if event.id() == MENU_SETTINGS {
                if let Err(e) = app.emit(EVENT_MENU_SETTINGS, ()) {
                    eprintln!("menu: emit failed: {e}");
                }
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let for_targets = handle.clone();
            monitor::spawn(handle.clone(), move || {
                for_targets.state::<SessionManager>().probe_targets()
            });
            let for_activity = handle.clone();
            app.manage(activity::spawn(handle.clone(), move || {
                for_activity.state::<SessionManager>().probe_targets()
            }));
            app.manage(usage::spawn(handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            session_spawn,
            session_write,
            session_resize,
            session_pause,
            session_resume,
            session_kill,
            session_reset,
            session_info,
            activity_watch,
            usage_watch,
            layout_load,
            layout_save,
            settings_load,
            settings_save,
            drop_paths,
            drop_save,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            handle.state::<SessionManager>().kill_all();
        }
    });
}
