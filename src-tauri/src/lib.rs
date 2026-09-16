//! sidebar-term: Tauri shell. Rust owns Sessions (ptys) and the facts about them;
//! the webview owns the sidebar layout. See docs/architecture.md.
//! CONTRACT: command names and signatures here are mirrored by `src/lib/ipc.ts`.

mod activity;
mod caffeinate;
mod detect;
mod drop;
mod guard;
mod layout;
mod model;
mod monitor;
mod paths;
mod resume;
mod session;
mod usage;

use model::{
    AgentKind, GuardSnapshot, ResumeEntry, SessionId, SessionInfo, EVENT_CAFFEINATE,
    EVENT_MEMORY_GUARD, EVENT_MENU_SETTINGS,
};
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
    resume_key: Option<String>,
    on_data: Channel<InvokeResponseBody>,
) -> Result<SessionId, String> {
    sessions.spawn(app, cwd, cols, rows, resume_key, on_data)
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
fn session_kill(
    sessions: State<'_, SessionManager>,
    guard: State<'_, guard::Guard>,
    session_id: SessionId,
) -> Result<(), String> {
    guard.release(session_id); // a frozen Session would not get the hangup
    sessions.kill(session_id)
}

/// Kill every Session and stop Activity and Usage reading. The webview calls this once at startup
/// so a webview reload does not leave the previous page's shells (or `ps` runs) going with nowhere
/// to send output. What those shells were running becomes Resume leftover, for the new page.
#[tauri::command]
fn session_reset(
    sessions: State<'_, SessionManager>,
    activity: State<'_, activity::Activity>,
    usage: State<'_, usage::Usage>,
    resume: State<'_, resume::Resume>,
    guard: State<'_, guard::Guard>,
) {
    resume.end_run(resume::entries(&sessions.keyed_targets()));
    guard.release_all();
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

/// Memory Guard's state.
#[tauri::command]
fn guard_state(guard: State<'_, guard::Guard>) -> GuardSnapshot {
    guard.snapshot()
}

/// Turn Memory Guard on or off and set its limit (percent of physical memory); off thaws every
/// frozen Tab. Returns the new state, which also goes out as `memory-guard`.
#[tauri::command]
fn guard_set(
    guard: State<'_, guard::Guard>,
    activity: State<'_, activity::Activity>,
    on: bool,
    limit_percent: u8,
) -> GuardSnapshot {
    let snapshot = guard.set(on, limit_percent);
    activity.sample_for_guard(on);
    snapshot
}

/// The Session whose Tab is in view (null: none). Memory Guard never freezes it, and thaws it if
/// it was frozen.
#[tauri::command]
fn guard_visible(guard: State<'_, guard::Guard>, session_id: Option<SessionId>) {
    guard.set_visible(session_id);
}

/// Freeze a Session by hand, whether Memory Guard is on or not. Fails for the Session in view.
#[tauri::command]
fn guard_freeze(
    sessions: State<'_, SessionManager>,
    guard: State<'_, guard::Guard>,
    session_id: SessionId,
) -> Result<GuardSnapshot, String> {
    let target = sessions
        .probe_target(session_id)
        .ok_or_else(|| format!("no Session {session_id}"))?;
    guard.freeze(&target)
}

/// Thaw a frozen Session without going to its Tab.
#[tauri::command]
fn guard_thaw(guard: State<'_, guard::Guard>, session_id: SessionId) -> GuardSnapshot {
    guard.thaw(session_id)
}

/// Start (or change the agents of) or stop reading Usage; while on, `usage` fires on each change.
#[tauri::command]
fn usage_watch(usage: State<'_, usage::Usage>, on: bool, agents: Vec<AgentKind>) {
    usage.watch(on, agents);
}

/// Whether Caffeinate is keeping this Mac awake.
#[tauri::command]
fn caffeinate_state(caffeinate: State<'_, caffeinate::Caffeinate>) -> bool {
    caffeinate.is_on()
}

/// Turn Caffeinate on or off; returns whether it is on now.
#[tauri::command]
fn caffeinate_set(caffeinate: State<'_, caffeinate::Caffeinate>, on: bool) -> Result<bool, String> {
    caffeinate.set(on)
}

/// What earlier runs left running and the webview has not yet resumed or dismissed (Resume).
#[tauri::command]
fn resume_leftover(resume: State<'_, resume::Resume>) -> Vec<ResumeEntry> {
    resume.leftover()
}

/// Drop leftover Resume entries by key: resumed, dismissed, or their Tab is gone.
#[tauri::command]
fn resume_forget(resume: State<'_, resume::Resume>, keys: Vec<String>) {
    resume.forget(&keys);
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

/// For each path printed in the Session, the absolute path of the file or directory it names, or
/// null when there is none. Async so a slow disk stalls a worker thread, not the main thread.
#[tauri::command]
async fn path_resolve(
    sessions: State<'_, SessionManager>,
    session_id: SessionId,
    candidates: Vec<String>,
) -> Result<Vec<Option<String>>, String> {
    let bases = sessions
        .probe_target(session_id)
        .map(|t| paths::bases(&detect::probe(&t)))
        .unwrap_or_default();
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    Ok(candidates
        .iter()
        .map(|c| paths::resolve(c, &bases, home.as_deref()))
        .collect())
}

/// Open a file or directory (an absolute path from `path_resolve`) in its default app.
#[tauri::command]
async fn path_open(path: String) -> Result<(), String> {
    paths::open(&path)
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
            let frozen_file = layout::path(&handle, layout::FROZEN)
                .inspect_err(|e| eprintln!("memory guard: no app data dir ({e}); not persisted"))
                .ok();
            let for_guard_events = handle.clone();
            app.manage(guard::Guard::open(frozen_file, move |snapshot| {
                if let Err(e) = for_guard_events.emit(EVENT_MEMORY_GUARD, snapshot) {
                    eprintln!("memory guard: emit failed: {e}");
                }
            }));
            let for_activity = handle.clone();
            let for_guard = handle.clone();
            app.manage(activity::spawn(
                handle.clone(),
                move || for_activity.state::<SessionManager>().probe_targets(),
                move |snapshot, targets| {
                    for_guard.state::<guard::Guard>().observe(snapshot, targets)
                },
            ));
            let for_caffeinate = handle.clone();
            app.manage(caffeinate::Caffeinate::new(move || {
                if let Err(e) = for_caffeinate.emit(EVENT_CAFFEINATE, false) {
                    eprintln!("caffeinate: emit failed: {e}");
                }
            }));
            let resume_file = layout::path(&handle, layout::RESUME)
                .inspect_err(|e| eprintln!("resume: no app data dir ({e}); not persisted"))
                .ok();
            app.manage(resume::Resume::open(resume_file));
            let for_resume = handle.clone();
            resume::spawn(move || {
                let targets = for_resume.state::<SessionManager>().keyed_targets();
                for_resume
                    .state::<resume::Resume>()
                    .record(resume::entries(&targets));
            });
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
            guard_state,
            guard_set,
            guard_visible,
            guard_freeze,
            guard_thaw,
            usage_watch,
            caffeinate_state,
            caffeinate_set,
            resume_leftover,
            resume_forget,
            layout_load,
            layout_save,
            settings_load,
            settings_save,
            path_resolve,
            path_open,
            drop_paths,
            drop_save,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            let sessions = handle.state::<SessionManager>();
            // Record what is running before killing it, so the next launch can resume it.
            if let Some(resume) = handle.try_state::<resume::Resume>() {
                resume.finish(resume::entries(&sessions.keyed_targets()));
            }
            if let Some(guard) = handle.try_state::<guard::Guard>() {
                guard.release_all(); // stopped processes would never get the hangup
            }
            sessions.kill_all();
        }
    });
}
