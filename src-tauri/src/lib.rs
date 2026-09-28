//! sidebar-term: the Mac app. Tauri's window, menu and IPC around the sidebar-term core
//! (`sidebar_term_core`, ADR 0002), which owns Sessions (ptys), the facts about them and the
//! layout (Groups, Tabs, the active Tab); the webview mirrors the layout and owns presentation.
//! This crate is the local Host; `daemon/` is the headless one. See docs/architecture.md.
//! CONTRACT: command names and signatures here are mirrored by `src/lib/ipc.ts`.

mod caffeinate;
mod drop;
mod host;

use host::AppHost;
use sidebar_term_core::agents::Agents;
use sidebar_term_core::journal::{self, Journal};
use sidebar_term_core::layout::{Layout, TabNew};
use sidebar_term_core::model::{
    AgentEvent, AgentKind, ClaudeConversation, ConversationFiles, Group, GuardSnapshot, HandoffProbe,
    LayoutSnapshot, Pairing, RemoteSnapshot, ResumeEntry, SessionId, SessionInfo, Tab,
    TabLink,
    EVENT_CAFFEINATE, EVENT_MEMORY_GUARD, EVENT_MENU_SETTINGS,
};
use sidebar_term_core::session::SessionManager;
use sidebar_term_core::{
    activity, detect, guard, handoff, monitor, paths, remote, resume, store, usage,
};
use std::sync::Arc;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::menu::{Menu, MenuItem, MenuItemKind, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

/// Id of the app menu's "Settings…" item.
const MENU_SETTINGS: &str = "settings";

/// The Session registry, shared with Remote and the core's threads.
type Sessions = Arc<SessionManager>;

/// The webview's Terminal for Session `session_id` takes its output from here on, as raw bytes
/// down `on_data`, starting with whatever the Session printed before (the shell's prompt).
#[tauri::command]
fn session_attach(
    layout: State<'_, Arc<Layout>>,
    session_id: SessionId,
    on_data: Channel<InvokeResponseBody>,
) -> Result<(), String> {
    layout.attach(
        session_id,
        Box::new(move |bytes| {
            // Err only when the webview is gone; nothing useful to do about it here.
            let _ = on_data.send(InvokeResponseBody::Raw(bytes));
        }),
    )
}

#[tauri::command]
fn session_write(sessions: State<'_, Sessions>, session_id: SessionId, data: String) -> Result<(), String> {
    sessions.write(session_id, data.as_bytes())
}

#[tauri::command]
fn session_resize(sessions: State<'_, Sessions>, session_id: SessionId, cols: u16, rows: u16) -> Result<(), String> {
    sessions.resize(session_id, cols, rows)
}

#[tauri::command]
fn session_pause(sessions: State<'_, Sessions>, session_id: SessionId) -> Result<(), String> {
    sessions.pause(session_id)
}

#[tauri::command]
fn session_resume(sessions: State<'_, Sessions>, session_id: SessionId) -> Result<(), String> {
    sessions.resume(session_id)
}

/// The webview is starting: replace every Session a previous page's Terminal was attached to
/// (a reload leaves those shells with nowhere to send output) with a fresh one in the same Tab,
/// and stop Activity and Usage reading. What the replaced shells were running becomes Resume
/// leftover, for the new page. At the app's first page nothing is attached yet, so the Sessions
/// the core spawned at launch are kept.
#[tauri::command]
fn session_reset(
    sessions: State<'_, Sessions>,
    layout: State<'_, Arc<Layout>>,
    activity: State<'_, activity::Activity>,
    usage: State<'_, usage::Usage>,
    resume: State<'_, resume::Resume>,
) {
    resume.end_run(resume::entries(&sessions.keyed_targets()));
    layout.respawn_attached();
    activity.watch(false);
    usage.watch(false, Vec::new());
}

/// On-demand probe, e.g. to decide whether closing a Tab needs confirmation.
#[tauri::command]
fn session_info(sessions: State<'_, Sessions>, session_id: SessionId) -> Option<SessionInfo> {
    sessions.info(session_id)
}

/// The last agent events, oldest first; later ones arrive on `agent-event`.
#[tauri::command]
fn agent_events(agents: State<'_, Agents>) -> Vec<AgentEvent> {
    agents.recent()
}

/// Answer the question Session `session_id`'s agent is waiting on with option `option` (0-based).
#[tauri::command]
fn agent_answer(agents: State<'_, Agents>, session_id: SessionId, pending_id: u64, option: usize) -> Result<(), String> {
    agents.answer(session_id, pending_id, option)
}

/// Stop holding that question: the agent asks it in its Terminal instead.
#[tauri::command]
fn agent_release(agents: State<'_, Agents>, session_id: SessionId, pending_id: u64) {
    agents.release(session_id, pending_id)
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
    sessions: State<'_, Sessions>,
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

// --- The layout: one snapshot, and the commands that change it (the core's `layout/`) --------

/// The whole layout, for the webview's first read; every change after that comes as `layout`.
#[tauri::command]
fn layout_get(layout: State<'_, Arc<Layout>>) -> LayoutSnapshot {
    layout.snapshot()
}

/// A new Tab with its Session, active. Defaults: the active Tab's Group, right after it, at its
/// cwd; `cols` / `rows` size the pty so the Terminal's first fit is a no-op.
#[tauri::command]
fn tab_new(
    layout: State<'_, Arc<Layout>>,
    group_id: Option<String>,
    after_tab_id: Option<String>,
    cwd: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<Tab, String> {
    layout.tab_new(TabNew {
        group_id,
        after_tab_id,
        cwd,
        cols,
        rows,
    })
}

/// Close a Tab and kill its Session, no questions asked: the webview confirms first
/// (`src/lib/sidebar/closeTabFlow.ts`).
#[tauri::command]
fn tab_close(layout: State<'_, Arc<Layout>>, tab_id: String) -> Result<(), String> {
    layout.tab_close(&tab_id)
}

/// Rename a Tab; an empty title restores the automatic Title.
#[tauri::command]
fn tab_rename(layout: State<'_, Arc<Layout>>, tab_id: String, title: String) -> Result<(), String> {
    layout.tab_rename(&tab_id, &title)
}

/// Move a Tab to `index` in a Group (default: its end).
#[tauri::command]
fn tab_move(
    layout: State<'_, Arc<Layout>>,
    tab_id: String,
    group_id: String,
    index: Option<usize>,
) -> Result<(), String> {
    layout.tab_move(&tab_id, &group_id, index)
}

#[tauri::command]
fn tab_activate(layout: State<'_, Arc<Layout>>, tab_id: String) -> Result<(), String> {
    layout.tab_activate(&tab_id)
}

/// Link paired Host `host_id`'s Tab `tab_id` into this layout (ADR 0003), right after
/// `after_tab_id`, else as a new Tab would go; not made active. An existing link is handed back.
#[tauri::command]
fn tab_link(
    layout: State<'_, Arc<Layout>>,
    host_id: String,
    tab_id: String,
    group_id: Option<String>,
    after_tab_id: Option<String>,
) -> Result<Tab, String> {
    layout.tab_link(TabLink { host_id, tab_id }, group_id.as_deref(), after_tab_id.as_deref())
}

/// Host `host_id`'s links follow the Tabs it has (`tab_ids`, in its order): gone Tabs' links go,
/// new Tabs are linked into the Group named `group_name`. Resolves to the Tabs linked now.
#[tauri::command]
fn links_reconcile(
    layout: State<'_, Arc<Layout>>,
    host_id: String,
    tab_ids: Vec<String>,
    group_name: String,
) -> Vec<String> {
    layout.links_reconcile(&host_id, &tab_ids, &group_name)
}

/// A new Group at the end ("New Group" unless named); with `tab_id`, that Tab moves into it.
#[tauri::command]
fn group_new(
    layout: State<'_, Arc<Layout>>,
    name: Option<String>,
    tab_id: Option<String>,
) -> Result<Group, String> {
    layout.group_new(name.as_deref(), tab_id.as_deref())
}

#[tauri::command]
fn group_rename(layout: State<'_, Arc<Layout>>, group_id: String, name: String) -> Result<(), String> {
    layout.group_rename(&group_id, &name)
}

#[tauri::command]
fn group_move(layout: State<'_, Arc<Layout>>, group_id: String, index: usize) -> Result<(), String> {
    layout.group_move(&group_id, index)
}

/// Delete a Group and close every Tab in it. Never the last Group.
#[tauri::command]
fn group_delete(layout: State<'_, Arc<Layout>>, group_id: String) -> Result<(), String> {
    layout.group_delete(&group_id)
}

#[tauri::command]
fn group_set_collapsed(
    layout: State<'_, Arc<Layout>>,
    group_id: String,
    collapsed: bool,
) -> Result<(), String> {
    layout.group_set_collapsed(&group_id, collapsed)
}

#[tauri::command]
fn settings_load(app: AppHandle) -> Result<Option<serde_json::Value>, String> {
    store::load(&AppHost(app), store::SETTINGS)
}

#[tauri::command]
fn settings_save(app: AppHandle, settings: serde_json::Value) -> Result<(), String> {
    store::save(&AppHost(app), store::SETTINGS, &settings)
}

/// For each path printed in the Session, the absolute path of the file or directory it names, or
/// null when there is none. Async so a slow disk stalls a worker thread, not the main thread.
#[tauri::command]
async fn path_resolve(
    sessions: State<'_, Sessions>,
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

/// Open a file or directory (an absolute path from `path_resolve`) in its default app, as a
/// double-click in Finder does.
#[tauri::command]
async fn path_open(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).is_absolute() {
        return Err(format!("not an absolute path: {path}"));
    }
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
}

// --- Handoff: what moving a Tab to another Host needs of this one (docs/architecture.md) ------

/// What Handoff needs to know about a local Session before its Tab moves: fresh facts, its
/// Resume entry, the Claude Code conversation running in it (files located), and its
/// checkout's git status. Async: runs `git`. `None` for a Session that is gone.
#[tauri::command]
async fn handoff_probe(
    sessions: State<'_, Sessions>,
    session_id: SessionId,
) -> Result<Option<HandoffProbe>, String> {
    let Some((key, target)) = sessions
        .keyed_targets()
        .into_iter()
        .find(|(_, t)| t.session_id == session_id)
    else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || {
        let info = detect::probe(&target);
        let entry = detect::resume::entry(&key, &target);
        let conversation = detect::resume::claude_conversation(&target);
        let git = match (&info.git, &info.cwd) {
            (Some(_), Some(cwd)) if !info.remote => handoff::git_status(cwd).ok(),
            _ => None,
        };
        Some(HandoffProbe { info, entry, conversation, git })
    })
    .await
    .map_err(|e| e.to_string())
}

/// A conversation's files, once its transcript has stopped changing (call after the Session is
/// killed): what goes to the Host.
#[tauri::command]
async fn handoff_conversation_read(conversation: ClaudeConversation) -> Result<ConversationFiles, String> {
    tauri::async_runtime::spawn_blocking(move || handoff::read(&conversation))
        .await
        .map_err(|e| e.to_string())?
}

/// Delete this Mac's copy of a conversation's transcript, once the Host has it.
#[tauri::command]
async fn handoff_conversation_forget(conversation: ClaudeConversation) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || handoff::forget(&conversation))
        .await
        .map_err(|e| e.to_string())?
}

/// Where Remote stands, after re-reading Tailscale's state. Async: runs the Tailscale CLI.
#[tauri::command]
async fn remote_state(app: AppHandle) -> Result<RemoteSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<remote::Remote>().refresh())
        .await
        .map_err(|e| e.to_string())
}

/// Turn Remote on or off. Async: binds the server and runs the Tailscale CLI.
#[tauri::command]
async fn remote_set(app: AppHandle, on: bool) -> Result<RemoteSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<remote::Remote>().set(on))
        .await
        .map_err(|e| e.to_string())?
}

/// Start a pairing: the code (and QR link) a phone presents once to be let in.
#[tauri::command]
fn remote_pair_begin(remote: State<'_, remote::Remote>) -> Pairing {
    remote.pair_begin()
}

#[tauri::command]
fn remote_pair_cancel(remote: State<'_, remote::Remote>) {
    remote.pair_cancel()
}

/// Forget a paired phone.
#[tauri::command]
fn remote_revoke(remote: State<'_, remote::Remote>, id: String) {
    remote.revoke(&id)
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
    let taps = Arc::new(remote::Taps::default());
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .menu(app_menu)
        .on_menu_event(|app, event| {
            if event.id() == MENU_SETTINGS {
                if let Err(e) = app.emit(EVENT_MENU_SETTINGS, ()) {
                    eprintln!("menu: emit failed: {e}");
                }
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            // The core runs with Tauri behind it (host.rs): every piece below is the core's.
            let host = host::host(&handle);
            let events = host.events.clone();
            let sessions: Sessions = Arc::new(SessionManager::new(taps.clone(), events.clone()));
            app.manage(sessions.clone());
            // Claude Code's hooks, up before the layout spawns the first Session, which gets
            // their environment like every later one.
            let agents = Agents::start(&host);
            let for_env = agents.clone();
            sessions.set_env_hook(Box::new(move |env| for_env.extend_env(env)));
            let for_exit = agents.clone();
            sessions.on_exit(Arc::new(move |id| for_exit.forget(id)));
            app.manage(agents.clone());
            // The Journal: what each Agent session does from here on, kept for Rewind.
            let journal_dir = store::path(&*host.paths, store::JOURNAL)
                .inspect_err(|e| eprintln!("journal: no app data dir ({e}); not kept"))
                .ok();
            let journal = Journal::open(journal_dir);
            let for_exit = journal.clone();
            sessions.on_exit(Arc::new(move |id| for_exit.end(id)));
            let for_steps = journal.clone();
            agents.watch_steps(Box::new(move |id, step, at| for_steps.step(id, step, at)));
            journal::spawn(journal.clone());
            app.manage(journal.clone());
            let frozen_file = store::path(&*host.paths, store::FROZEN)
                .inspect_err(|e| eprintln!("memory guard: no app data dir ({e}); not persisted"))
                .ok();
            let for_guard_events = events.clone();
            app.manage(guard::Guard::open(frozen_file, move |snapshot| {
                for_guard_events.emit(EVENT_MEMORY_GUARD, &snapshot);
            }));
            // The layout: Tabs and Groups from `layout.json`, each Tab's Session spawned now. A
            // Session frozen by Memory Guard is thawed before the layout kills it.
            let for_kill = handle.clone();
            let layout = Layout::open(
                host.paths.clone(),
                events.clone(),
                sessions.clone(),
                Box::new(move |id| for_kill.state::<guard::Guard>().release(id)),
            );
            app.manage(layout.clone());
            let for_targets = sessions.clone();
            let for_marks = sessions.clone();
            let for_observe = layout.clone();
            monitor::spawn(
                events.clone(),
                agents.clone(),
                move || for_targets.probe_targets(),
                move |id| for_marks.marks(id),
                move |infos| {
                    for_observe.observe(infos);
                    journal.observe(infos);
                },
            );
            let for_activity = sessions.clone();
            let for_guard = handle.clone();
            app.manage(activity::spawn(
                events.clone(),
                move || for_activity.probe_targets(),
                move |snapshot, targets| {
                    for_guard.state::<guard::Guard>().observe(snapshot, targets);
                    // Clients on the socket get each Session's CPU and memory too.
                    if let Some(remote) = for_guard.try_state::<remote::Remote>() {
                        remote.publish_activity(snapshot);
                    }
                },
            ));
            let for_caffeinate = events.clone();
            app.manage(caffeinate::Caffeinate::new(move || {
                for_caffeinate.emit(EVENT_CAFFEINATE, &false);
            }));
            let resume_file = store::path(&*host.paths, store::RESUME)
                .inspect_err(|e| eprintln!("resume: no app data dir ({e}); not persisted"))
                .ok();
            app.manage(resume::Resume::open(resume_file));
            let for_resume = handle.clone();
            let resume_sessions = sessions.clone();
            resume::spawn(move || {
                let targets = resume_sessions.keyed_targets();
                for_resume
                    .state::<resume::Resume>()
                    .record(resume::entries(&targets));
            });
            app.manage(usage::spawn(events.clone(), &*host.paths));
            let remote_file = store::path(&*host.paths, store::REMOTE)
                .inspect_err(|e| eprintln!("remote: no app data dir ({e}); pairings not persisted"))
                .ok();
            // Clients on the socket get the layout and each Session's facts through Remote,
            // which watches the layout.
            app.manage(remote::Remote::open(host, sessions, taps, layout, agents, remote_file));
            app.state::<remote::Remote>().start_if_enabled();
            // Dev aid: `SIDEBAR_TERM_REMOTE_PAIR=1 pnpm tauri dev` starts a pairing at launch and
            // prints its code, so a browser can pair without clicking through Settings.
            if cfg!(debug_assertions) && std::env::var_os("SIDEBAR_TERM_REMOTE_PAIR").is_some() {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let pairing = handle.state::<remote::Remote>().pair_begin();
                    eprintln!("remote: pairing code {} at {}", pairing.code, pairing.url.as_deref().unwrap_or("(no url)"));
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            session_attach,
            session_write,
            session_resize,
            session_pause,
            session_resume,
            session_reset,
            session_info,
            agent_events,
            agent_answer,
            agent_release,
            layout_get,
            tab_new,
            tab_close,
            tab_rename,
            tab_move,
            tab_activate,
            tab_link,
            links_reconcile,
            group_new,
            group_rename,
            group_move,
            group_delete,
            group_set_collapsed,
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
            settings_load,
            settings_save,
            path_resolve,
            path_open,
            handoff_probe,
            handoff_conversation_read,
            handoff_conversation_forget,
            drop_paths,
            drop_save,
            remote_state,
            remote_set,
            remote_pair_begin,
            remote_pair_cancel,
            remote_revoke,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            let sessions = handle.state::<Sessions>();
            // The layout as it stands, ahead of the saver's quiet period.
            if let Some(layout) = handle.try_state::<Arc<Layout>>() {
                layout.flush();
            }
            // Agent time ends here, not as the Sessions are killed.
            if let Some(journal) = handle.try_state::<Journal>() {
                journal.finish();
            }
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
