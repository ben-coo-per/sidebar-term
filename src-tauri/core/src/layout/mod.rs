//! The layout: Groups, Tabs, their order, custom Titles, each Tab's last cwd and Session, and the
//! active Tab, owned by the Host (ADR 0002) so that the Mac webview, a phone and the daemon's
//! clients all read one model and change it through the same commands.
//!
//! - `model.rs` is the pure state and its transitions, tested without a pty.
//! - `file.rs` is `layout.json` (version 3) and the migration of older versions.
//!
//! [`Layout`] wraps the model in a lock and adds the side effects: a Tab's Session is spawned as
//! the Tab is made (at launch for every persisted Tab, or on `tab_new`) and killed as it is
//! closed, with its Tab id as the Resume key; a Session that exits on its own takes its Tab with
//! it. A linked Tab (ADR 0003: the Mac app places a paired Host's Tab among its own Groups) has
//! no Session here, so it is never spawned, respawned, resumed or probed; the webview links and
//! reconciles them (`tab_link`, `links_reconcile`), and Remote serves the layout without them. Every change emits one `layout` event carrying the whole snapshot (under the model's lock,
//! so snapshots arrive in order; each carries a revision for clients that cannot rely on that),
//! rewrites `layout.json` after a 500 ms quiet period, and tells `watch`ers (Remote, which
//! serves the Host protocol). Session facts (`SessionInfo`) reach it through `observe`, from
//! the monitor, and are kept per live Session for clients that arrive later (`facts`).
//!
//! Output before a client attaches is held in an `outlet::Outlet` per Session (`attach`).

pub mod file;
pub mod model;

use crate::host::{Events, OutputSink, Paths};
use crate::model::{Group, LayoutSnapshot, SessionId, SessionInfo, Tab, TabLink, EVENT_LAYOUT};
use crate::outlet::{Outlet, Outlets};
use crate::session::SessionManager;
use model::{new_id, Model};
use std::collections::HashMap;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

/// How long after the last change `layout.json` is rewritten.
const SAVE_QUIET: Duration = Duration::from_millis(500);
/// The pty size a Session gets when no client says (a Terminal refits it on mount).
pub const DEFAULT_COLS: u16 = 80;
pub const DEFAULT_ROWS: u16 = 24;

/// What `tab_new` takes.
#[derive(Clone, Debug, Default)]
pub struct TabNew {
    /// The Group to add to; the active Tab's, else the first, when `None`.
    pub group_id: Option<String>,
    /// The Tab to go right after (in its Group); the active Tab when it is in the Group, else
    /// the end, when `None`.
    pub after_tab_id: Option<String>,
    /// Where the Session starts; the active Tab's last cwd, else `$HOME`, when `None`.
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

/// Runs before a Session is killed (the app thaws it if Memory Guard froze it).
pub type BeforeKill = Box<dyn Fn(SessionId) + Send + Sync>;

/// What a watcher is told: the layout changed, or a Session's facts did. Both are read back
/// from the `Layout` (`snapshot`, `fact`), so a watcher that is slow sees the latest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Update {
    Layout,
    Session(SessionId),
}

/// Told of every change, under the model's lock, so updates arrive in order (Remote relays them
/// to its clients).
pub type Watcher = Box<dyn Fn(Update) + Send + Sync>;

pub struct Layout {
    inner: Arc<Inner>,
}

struct Inner {
    paths: Arc<dyn Paths>,
    events: Arc<dyn Events>,
    sessions: Arc<SessionManager>,
    outlets: Outlets,
    state: Mutex<State>,
    saver: Mutex<Option<mpsc::Sender<()>>>,
    before_kill: BeforeKill,
    watchers: Mutex<Vec<Watcher>>,
}

struct State {
    model: Model,
    revision: u64,
    /// The latest facts about each live Session, from the monitor.
    facts: HashMap<SessionId, SessionInfo>,
}

impl Layout {
    /// Load `layout.json` (migrating a version-1 file, see `file.rs`), or start with one Group
    /// and one Tab, and spawn a Session for every Tab at its last cwd. Registers with
    /// `sessions` to hear Sessions exit.
    pub fn open(
        paths: Arc<dyn Paths>,
        events: Arc<dyn Events>,
        sessions: Arc<SessionManager>,
        before_kill: BeforeKill,
    ) -> Arc<Layout> {
        let loaded = match file::load(&*paths) {
            Ok(loaded) => loaded,
            Err(e) => {
                eprintln!("layout: {} unreadable ({e}); starting fresh", crate::store::LAYOUT);
                None
            }
        };
        let (model, fresh) = match loaded {
            Some(parsed) => {
                if let Some(client) = &parsed.client {
                    if let Err(e) = file::migrate_client_state(&*paths, client) {
                        eprintln!("layout: could not move the sidebar settings: {e}");
                    }
                }
                (parsed.model, false)
            }
            None => (Model::fresh(), true),
        };
        let layout = Arc::new(Layout {
            inner: Arc::new(Inner {
                paths,
                events,
                sessions: sessions.clone(),
                outlets: Outlets::default(),
                state: Mutex::new(State {
                    model,
                    revision: 0,
                    facts: HashMap::new(),
                }),
                saver: Mutex::new(None),
                before_kill,
                watchers: Mutex::new(Vec::new()),
            }),
        });

        // Hear exits before the first spawn, so a shell that dies at once takes its Tab with it.
        let weak = Arc::downgrade(&layout);
        sessions.on_exit(Arc::new(move |id| {
            if let Some(layout) = weak.upgrade() {
                layout.inner.session_exited(id);
            }
        }));

        {
            let mut s = lock(&layout.inner.state);
            if fresh {
                let group_id = s.model.groups[0].id.clone();
                let id = new_id("tab");
                let session_id = layout.inner.spawn(&id, None, DEFAULT_COLS, DEFAULT_ROWS);
                let _ = s.model.insert_tab(
                    Tab {
                        id,
                        group_id,
                        session_id,
                        custom_title: None,
                        last_cwd: None,
                        link: None,
                    },
                    None,
                );
            } else {
                for id in s.model.ordered_tab_ids() {
                    let tab = &s.model.tabs[&id];
                    if tab.is_linked() {
                        continue; // its Session is its Host's
                    }
                    let cwd = tab.last_cwd.clone();
                    let session_id = layout.inner.spawn(&id, cwd, DEFAULT_COLS, DEFAULT_ROWS);
                    let _ = s.model.set_session(&id, session_id);
                }
                if s.model.active_tab_id.is_none() {
                    s.model.active_tab_id = s.model.ordered_tab_ids().into_iter().next();
                }
            }
        }
        // A migrated or fresh layout is on disk in its current shape before anything else runs.
        layout.inner.save_now();
        layout
    }

    /// Told of every change from now on (under the lock, in order).
    pub fn watch(&self, watcher: Watcher) {
        let _s = lock(&self.inner.state);
        lock(&self.inner.watchers).push(watcher);
    }

    pub fn snapshot(&self) -> LayoutSnapshot {
        let s = lock(&self.inner.state);
        s.model.snapshot(s.revision)
    }

    /// The snapshot for Remote's clients: without linked Tabs (they cannot reach a paired Host
    /// through this one).
    pub fn client_snapshot(&self) -> LayoutSnapshot {
        let s = lock(&self.inner.state);
        s.model.snapshot_without_links(s.revision)
    }

    /// `Err` for a linked Tab: Remote's clients cannot see those, so a command naming one is
    /// refused as if it did not exist.
    pub fn refuse_linked(&self, tab_id: &str) -> Result<(), String> {
        if lock(&self.inner.state).model.is_linked(tab_id) {
            return Err(format!("no Tab {tab_id}"));
        }
        Ok(())
    }

    /// The latest facts about every live Session, by Session id.
    pub fn facts(&self) -> Vec<SessionInfo> {
        let s = lock(&self.inner.state);
        let mut facts: Vec<SessionInfo> = s.facts.values().cloned().collect();
        facts.sort_by_key(|i| i.session_id);
        facts
    }

    /// The latest facts about Session `id`; `None` for one the monitor has not seen, or that is gone.
    pub fn fact(&self, id: SessionId) -> Option<SessionInfo> {
        lock(&self.inner.state).facts.get(&id).cloned()
    }

    /// Whether a client with a Terminal has attached to Session `id` in process (the Mac webview),
    /// so a client on the socket is not the only one showing it.
    pub fn is_attached(&self, id: SessionId) -> bool {
        self.inner.outlets.get(id).is_some_and(|o| o.was_attached())
    }

    /// The monitor's word on Sessions that changed: keeps each Tab's last cwd and the facts
    /// clients are shown, and tells the watchers.
    pub fn observe(&self, infos: &[SessionInfo]) {
        let mut s = lock(&self.inner.state);
        let mut layout_changed = false;
        for info in infos {
            if !info.remote {
                if let Some(cwd) = &info.cwd {
                    let tab_id = s.model.tab_of_session(info.session_id).map(|t| t.id.clone());
                    if let Some(tab_id) = tab_id {
                        layout_changed |= s.model.set_last_cwd(&tab_id, cwd);
                    }
                }
            }
            s.facts.insert(info.session_id, info.clone());
        }
        if layout_changed {
            self.inner.changed(&mut s);
        }
        for info in infos {
            self.inner.notify(Update::Session(info.session_id));
        }
    }

    /// A client's Terminal takes Session `id`'s output from here on (what came before is handed
    /// over first). `Err` for a Session that is gone.
    pub fn attach(&self, id: SessionId, sink: OutputSink) -> Result<(), String> {
        self.inner.outlets.attach(id, sink)
    }

    /// The Mac webview reloaded: the Sessions its Terminals were attached to are replaced by
    /// fresh ones in the same Tabs (their Terminals are gone with the old page), as a relaunch
    /// would. Sessions no client attached to are left as they are.
    pub fn respawn_attached(&self) {
        let attached = self.inner.outlets.attached();
        if attached.is_empty() {
            return;
        }
        let mut kills = Vec::new();
        {
            let mut s = lock(&self.inner.state);
            for old in attached {
                let Some(tab) = s.model.tab_of_session(old).cloned() else {
                    continue;
                };
                let session_id = self
                    .inner
                    .spawn(&tab.id, tab.last_cwd.clone(), DEFAULT_COLS, DEFAULT_ROWS);
                let _ = s.model.set_session(&tab.id, session_id);
                s.facts.remove(&old);
                kills.push(old);
            }
            self.inner.changed(&mut s);
        }
        for id in kills {
            self.inner.kill(id);
        }
    }

    /// Write `layout.json` now (on exit, ahead of the quiet period).
    pub fn flush(&self) {
        self.inner.save_now();
    }

    // --- Commands: pure transitions on the model; Session spawn and kill their only effects ----

    /// Make a Tab, with its Session, and go to it.
    pub fn tab_new(&self, opts: TabNew) -> Result<Tab, String> {
        let mut s = lock(&self.inner.state);
        let placement = s
            .model
            .placement_after(opts.group_id.as_deref(), opts.after_tab_id.as_deref(), opts.cwd)?;
        let id = new_id("tab");
        let session_id = self.inner.spawn(
            &id,
            placement.cwd.clone(),
            opts.cols.unwrap_or(DEFAULT_COLS),
            opts.rows.unwrap_or(DEFAULT_ROWS),
        );
        let tab = Tab {
            id,
            group_id: placement.group_id,
            session_id,
            custom_title: None,
            last_cwd: placement.cwd,
            link: None,
        };
        s.model.insert_tab(tab.clone(), placement.after.as_deref())?;
        self.inner.changed(&mut s);
        Ok(tab)
    }

    /// Link a paired Host's Tab into this layout (the Mac app, for a Tab it made there): placed
    /// after `after_tab_id`, else as a new Tab would be, and not made active. Linking a Tab
    /// already linked hands back that Tab.
    pub fn tab_link(&self, link: TabLink, group_id: Option<&str>, after_tab_id: Option<&str>) -> Result<Tab, String> {
        let mut s = lock(&self.inner.state);
        let before = s.model.linked_tab(&link.host_id, &link.tab_id).is_some();
        let tab = s.model.link_tab(link, group_id, after_tab_id)?;
        if !before {
            self.inner.changed(&mut s);
        }
        Ok(tab)
    }

    /// Host `host_id`'s links follow the Tabs it has (`host_tabs`, in its order): see
    /// `Model::reconcile_links`. Returns the ids of the Tabs linked just now (the webview marks
    /// them Unread).
    pub fn links_reconcile(&self, host_id: &str, host_tabs: &[String], stray_group: &str) -> Vec<String> {
        let mut s = lock(&self.inner.state);
        let r = s.model.reconcile_links(host_id, host_tabs, stray_group);
        if !r.linked.is_empty() || !r.unlinked.is_empty() {
            self.inner.changed(&mut s);
        }
        r.linked
    }

    /// Close a Tab: it goes at once, and its Session is killed. No confirmation here: a client
    /// asks first, using `session_info`.
    pub fn tab_close(&self, tab_id: &str) -> Result<(), String> {
        let session_id = {
            let mut s = lock(&self.inner.state);
            let tab = s.model.remove_tab(tab_id).ok_or_else(|| format!("no Tab {tab_id}"))?;
            if let Some(sid) = tab.session_id {
                s.facts.remove(&sid);
            }
            self.inner.changed(&mut s);
            tab.session_id
        };
        if let Some(sid) = session_id {
            self.inner.kill(sid);
        }
        Ok(())
    }

    /// Rename a Tab; an empty title restores the automatic Title.
    pub fn tab_rename(&self, tab_id: &str, title: &str) -> Result<(), String> {
        let mut s = lock(&self.inner.state);
        s.model.rename_tab(tab_id, title)?;
        self.inner.changed(&mut s);
        Ok(())
    }

    /// Move a Tab within or across Groups to `index` (default: the end of the Group).
    pub fn tab_move(&self, tab_id: &str, group_id: &str, index: Option<usize>) -> Result<(), String> {
        let mut s = lock(&self.inner.state);
        s.model.move_tab(tab_id, group_id, index)?;
        self.inner.changed(&mut s);
        Ok(())
    }

    /// `tab_move` for a Remote client, which sees no linked Tabs: `index` counts the Group's
    /// other Tabs only, and a linked Tab is not there to move.
    pub fn client_tab_move(&self, tab_id: &str, group_id: &str, index: Option<usize>) -> Result<(), String> {
        let mut s = lock(&self.inner.state);
        if s.model.is_linked(tab_id) {
            return Err(format!("no Tab {tab_id}"));
        }
        let index = index.map(|i| s.model.index_skipping_links(group_id, i));
        s.model.move_tab(tab_id, group_id, index)?;
        self.inner.changed(&mut s);
        Ok(())
    }

    pub fn tab_activate(&self, tab_id: &str) -> Result<(), String> {
        let mut s = lock(&self.inner.state);
        if s.model.activate(tab_id)? {
            self.inner.changed(&mut s);
        }
        Ok(())
    }

    /// A new Group at the end, named `name` (else "New Group"); with `tab_id`, that Tab moves
    /// into it ("New Group from Tab").
    pub fn group_new(&self, name: Option<&str>, tab_id: Option<&str>) -> Result<Group, String> {
        let mut s = lock(&self.inner.state);
        if let Some(tab_id) = tab_id {
            if !s.model.tabs.contains_key(tab_id) {
                return Err(format!("no Tab {tab_id}"));
            }
        }
        let id = s.model.new_group(name);
        if let Some(tab_id) = tab_id {
            s.model.move_tab(tab_id, &id, None)?;
        }
        let group = s.model.group(&id).cloned().ok_or("no Group")?;
        self.inner.changed(&mut s);
        Ok(group)
    }

    pub fn group_rename(&self, group_id: &str, name: &str) -> Result<(), String> {
        let mut s = lock(&self.inner.state);
        s.model.rename_group(group_id, name)?;
        self.inner.changed(&mut s);
        Ok(())
    }

    pub fn group_move(&self, group_id: &str, index: usize) -> Result<(), String> {
        let mut s = lock(&self.inner.state);
        s.model.move_group(group_id, index)?;
        self.inner.changed(&mut s);
        Ok(())
    }

    pub fn group_set_collapsed(&self, group_id: &str, collapsed: bool) -> Result<(), String> {
        let mut s = lock(&self.inner.state);
        if s.model.set_collapsed(group_id, collapsed)? {
            self.inner.changed(&mut s);
        }
        Ok(())
    }

    /// Delete a Group and close every Tab in it (a linked Tab's link goes; closing it on its Host
    /// is the webview's). Never the last Group.
    pub fn group_delete(&self, group_id: &str) -> Result<(), String> {
        let kills: Vec<SessionId> = {
            let mut s = lock(&self.inner.state);
            let tabs = s.model.delete_group(group_id)?;
            let kills: Vec<SessionId> = tabs.iter().filter_map(|t| t.session_id).collect();
            for sid in &kills {
                s.facts.remove(sid);
            }
            self.inner.changed(&mut s);
            kills
        };
        for sid in kills {
            self.inner.kill(sid);
        }
        Ok(())
    }
}

impl Inner {
    /// Spawn Tab `tab_id`'s Session; `None` (logged) when it cannot be, and the Tab stays
    /// Session-less.
    fn spawn(&self, tab_id: &str, cwd: Option<String>, cols: u16, rows: u16) -> Option<SessionId> {
        let outlet = Arc::new(Outlet::default());
        let for_sink = outlet.clone();
        let sink: OutputSink = Box::new(move |bytes| for_sink.deliver(bytes));
        match self
            .sessions
            .spawn(cwd, cols, rows, Some(tab_id.to_owned()), sink)
        {
            Ok(id) => {
                self.outlets.insert(id, outlet);
                Some(id)
            }
            Err(e) => {
                eprintln!("layout: spawning the Session of Tab {tab_id} failed: {e}");
                None
            }
        }
    }

    fn kill(&self, id: SessionId) {
        (self.before_kill)(id);
        if let Err(e) = self.sessions.kill(id) {
            eprintln!("layout: killing Session {id} failed: {e}");
        }
    }

    /// A Session is gone: its Tab goes with it, unless the Tab has moved on to another Session
    /// (a close, a respawn).
    fn session_exited(self: &Arc<Self>, id: SessionId) {
        self.outlets.remove(id);
        let mut s = lock(&self.state);
        s.facts.remove(&id);
        let tab_id = s.model.tab_of_session(id).map(|t| t.id.clone());
        if let Some(tab_id) = tab_id {
            s.model.remove_tab(&tab_id);
            self.changed(&mut s);
        }
    }

    /// The model changed: bump the revision, emit the snapshot, tell the watchers, and save
    /// once things go quiet. Called with the lock held so snapshots go out in order.
    fn changed(self: &Arc<Self>, s: &mut MutexGuard<'_, State>) {
        s.revision += 1;
        self.events
            .emit(EVENT_LAYOUT, &s.model.snapshot(s.revision));
        self.notify(Update::Layout);
        self.schedule_save();
    }

    /// Called with the state lock held, so watchers hear of changes in order.
    fn notify(&self, update: Update) {
        for w in lock(&self.watchers).iter() {
            w(update);
        }
    }

    fn save_now(&self) {
        let model = lock(&self.state).model.clone();
        if let Err(e) = file::save(&*self.paths, &model) {
            eprintln!("layout: writing {} failed: {e}", crate::store::LAYOUT);
        }
    }

    /// Nudge the saver thread (started on the first nudge); it writes once nothing has changed
    /// for `SAVE_QUIET`, and ends with the layout.
    fn schedule_save(self: &Arc<Self>) {
        let mut saver = lock(&self.saver);
        if let Some(tx) = saver.as_ref() {
            if tx.send(()).is_ok() {
                return;
            }
        }
        let (tx, rx) = mpsc::channel::<()>();
        let _ = tx.send(());
        let weak = Arc::downgrade(self);
        let started = thread::Builder::new()
            .name("layout-saver".into())
            .spawn(move || loop {
                if rx.recv().is_err() {
                    return;
                }
                loop {
                    match rx.recv_timeout(SAVE_QUIET) {
                        Ok(()) => continue,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                let Some(inner) = weak.upgrade() else { return };
                inner.save_now();
            });
        match started {
            Ok(_) => *saver = Some(tx),
            Err(e) => eprintln!("layout: could not start the saver thread: {e}"),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    //! The owner with real ptys: the pure rules are `model.rs`'s, this is the glue.

    use super::*;
    use crate::detect::testutil::TempDir;
    use crate::host::testing::Recorder;
    use crate::model::EVENT_SESSION_EXIT;
    use crate::remote::Taps;
    use crate::session::PTY_SERIAL;
    use std::path::PathBuf;
    use std::time::Instant;

    struct Dir(PathBuf);

    impl Paths for Dir {
        fn data_dir(&self) -> Result<PathBuf, String> {
            Ok(self.0.clone())
        }
    }

    fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        cond()
    }

    const T: Duration = Duration::from_secs(10);

    #[test]
    fn the_layout_spawns_sessions_for_its_tabs_and_follows_their_lives() {
        let _serial = lock(&PTY_SERIAL);
        let dir = TempDir::new("layout-owner");
        let paths: Arc<dyn Paths> = Arc::new(Dir(dir.path().to_path_buf()));
        let recorder = Arc::new(Recorder::default());
        let sessions = Arc::new(SessionManager::new(
            Arc::new(Taps::default()),
            recorder.clone(),
        ));
        let killed: Arc<Mutex<Vec<SessionId>>> = Arc::default();
        let for_hook = killed.clone();
        let layout = Layout::open(
            paths.clone(),
            recorder.clone(),
            sessions.clone(),
            Box::new(move |id| lock(&for_hook).push(id)),
        );

        // A fresh install: one Group "Tabs", one Tab with a live Session, on disk as version 3.
        let snap = layout.snapshot();
        assert_eq!(snap.groups.len(), 1);
        assert_eq!(snap.groups[0].name, "Tabs");
        assert_eq!(snap.tabs.len(), 1);
        let first = snap.tabs.values().next().unwrap().clone();
        let sid = first.session_id.expect("the Tab has a Session");
        assert_eq!(snap.active_tab_id, Some(first.id.clone()));
        assert_eq!(sessions.probe_targets().len(), 1);
        assert_eq!(sessions.keyed_targets()[0].0, first.id, "the Tab id is the Resume key");
        let on_disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join(crate::store::LAYOUT)).unwrap()).unwrap();
        assert_eq!(on_disk["version"], 3);

        // Watchers (Remote) hear of every change, layout and facts alike.
        let seen: Arc<Mutex<Vec<Update>>> = Arc::default();
        let for_watch = seen.clone();
        layout.watch(Box::new(move |u| lock(&for_watch).push(u)));
        assert!(lock(&seen).is_empty());
        assert!(layout.facts().is_empty(), "the monitor has not spoken yet");

        // A Terminal attaches: what the shell printed so far comes first, then the live output.
        let out: Arc<Mutex<Vec<u8>>> = Arc::default();
        let sink = out.clone();
        layout
            .attach(sid, Box::new(move |bytes| lock(&sink).extend_from_slice(&bytes)))
            .unwrap();
        sessions.write(sid, b"echo HI_$((6*7))\r").unwrap();
        let seen_text = |needle: &str| String::from_utf8_lossy(&lock(&out)).contains(needle);
        assert!(wait_until(T, || seen_text("HI_42")), "no output through the outlet");
        assert!(layout.attach(999_999, Box::new(|_| {})).is_err());

        // A new Tab: its own Session, active, right after the first, and one `layout` event.
        let events_before = recorder.named(EVENT_LAYOUT).len();
        let second = layout
            .tab_new(TabNew {
                cwd: Some(std::env::temp_dir().to_string_lossy().into_owned()),
                cols: Some(100),
                rows: Some(30),
                ..TabNew::default()
            })
            .unwrap();
        let sid2 = second.session_id.expect("spawned");
        let snap = layout.snapshot();
        assert_eq!(snap.groups[0].tab_ids, [first.id.clone(), second.id.clone()]);
        assert_eq!(snap.active_tab_id, Some(second.id.clone()));
        assert_eq!(recorder.named(EVENT_LAYOUT).len(), events_before + 1);
        assert_eq!(recorder.named(EVENT_LAYOUT).last().unwrap()["revision"], snap.revision);
        assert_eq!(*lock(&seen), [Update::Layout]);
        assert!(layout.is_attached(sid) && !layout.is_attached(sid2));

        // The monitor's facts keep the Tab's last cwd, are kept for late clients, and are
        // announced after the layout change they caused.
        let mut info = SessionInfo::empty(sid2);
        info.cwd = Some("/tmp/elsewhere".into());
        layout.observe(&[info.clone()]);
        assert_eq!(layout.snapshot().tabs[&second.id].last_cwd.as_deref(), Some("/tmp/elsewhere"));
        assert_eq!(&lock(&seen)[1..], [Update::Layout, Update::Session(sid2)]);
        assert_eq!(layout.fact(sid2), Some(info.clone()));
        assert_eq!(layout.facts(), [info]);
        assert_eq!(layout.fact(sid), None);

        // Closing a Tab: gone at once, its Session killed (after the hook), its facts dropped
        // and its exit ignored.
        layout.tab_close(&second.id).unwrap();
        assert_eq!(layout.snapshot().tabs.len(), 1);
        assert_eq!(*lock(&killed), [sid2]);
        assert!(layout.facts().is_empty());
        assert!(
            wait_until(T, || recorder
                .named(EVENT_SESSION_EXIT)
                .iter()
                .any(|e| e["sessionId"] == sid2)),
            "no exit for the closed Session"
        );
        assert_eq!(layout.snapshot().tabs.len(), 1, "the exit changes nothing more");
        assert!(layout.tab_close(&second.id).is_err());

        // A webview reload: the attached Session is replaced in its Tab; the Tab stays.
        let revision = layout.snapshot().revision;
        layout.respawn_attached();
        let snap = layout.snapshot();
        assert_eq!(snap.tabs.len(), 1);
        let sid3 = snap.tabs[&first.id].session_id.expect("respawned");
        assert_ne!(sid3, sid);
        assert!(snap.revision > revision);
        assert!(
            wait_until(T, || sessions.probe_target(sid).is_none()),
            "the old Session was not killed"
        );
        assert_eq!(layout.snapshot().tabs.len(), 1, "its exit did not take the Tab");
        layout.respawn_attached();
        assert_eq!(
            layout.snapshot().tabs[&first.id].session_id,
            Some(sid3),
            "nothing attached to the new Session: it is kept"
        );

        // The shell exiting on its own takes its Tab with it.
        sessions.write(sid3, b"exit\r").unwrap();
        assert!(
            wait_until(T, || layout.snapshot().tabs.is_empty()),
            "the Tab outlived its Session"
        );
        assert_eq!(layout.snapshot().active_tab_id, None);

        // The quiet period passes: the file follows.
        assert!(
            wait_until(T, || {
                let v: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(dir.path().join(crate::store::LAYOUT)).unwrap(),
                )
                .unwrap();
                v["tabs"].as_array().is_some_and(Vec::is_empty)
            }),
            "layout.json was not rewritten"
        );
    }

    #[test]
    fn a_persisted_layout_respawns_every_tab_at_its_last_cwd() {
        let _serial = lock(&PTY_SERIAL);
        let dir = TempDir::new("layout-respawn");
        let paths: Arc<dyn Paths> = Arc::new(Dir(dir.path().to_path_buf()));
        let cwd = std::env::temp_dir().to_string_lossy().into_owned();
        crate::store::save(
            &*paths,
            crate::store::LAYOUT,
            &serde_json::json!({
                "version": 1,
                "groups": [{ "id": "g", "name": "Work", "tabIds": ["a", "b"] }],
                "tabs": [
                    { "id": "a", "groupId": "g", "lastCwd": cwd, "unread": true },
                    { "id": "b", "groupId": "g", "customTitle": "Two" }
                ],
                "activeTabId": "b",
                "sidebarWidth": 300
            }),
        )
        .unwrap();
        let recorder = Arc::new(Recorder::default());
        let sessions = Arc::new(SessionManager::new(Arc::new(Taps::default()), recorder.clone()));
        let layout = Layout::open(paths.clone(), recorder, sessions.clone(), Box::new(|_| {}));
        let snap = layout.snapshot();
        assert_eq!(snap.tabs.len(), 2);
        assert!(snap.tabs.values().all(|t| t.session_id.is_some()));
        assert_eq!(snap.active_tab_id.as_deref(), Some("b"));
        assert_eq!(snap.tabs["b"].custom_title.as_deref(), Some("Two"));
        let keys: Vec<String> = sessions.keyed_targets().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["a", "b"]);
        // The version-1 presentation state moved into the webview's settings section.
        let settings = crate::store::load(&*paths, crate::store::SETTINGS).unwrap().unwrap();
        assert_eq!(settings["sidebar"]["width"], 300);
        assert_eq!(settings["sidebar"]["unread"], serde_json::json!(["a"]));
        // And the file is the current version now.
        let on_disk = crate::store::load(&*paths, crate::store::LAYOUT).unwrap().unwrap();
        assert_eq!(on_disk["version"], 3);
        layout.group_delete("g").unwrap_err(); // the last Group stays
        let other = layout.group_new(Some("Other"), Some("a")).unwrap();
        assert_eq!(layout.snapshot().tabs["a"].group_id, other.id);
        layout.group_delete(&other.id).unwrap();
        assert!(
            wait_until(T, || sessions.probe_targets().len() == 1),
            "deleting the Group did not kill its Tab's Session"
        );
        assert_eq!(layout.snapshot().tabs.len(), 1);
        sessions.kill_all();
        assert!(wait_until(T, || sessions.probe_targets().is_empty()));
    }

    #[test]
    fn linked_tabs_get_no_session_and_keep_their_place_across_a_relaunch() {
        let _serial = lock(&PTY_SERIAL);
        let dir = TempDir::new("layout-linked");
        let paths: Arc<dyn Paths> = Arc::new(Dir(dir.path().to_path_buf()));
        crate::store::save(
            &*paths,
            crate::store::LAYOUT,
            &serde_json::json!({
                "version": 3,
                "groups": [{ "id": "g", "name": "Work", "tabIds": ["a", "l", "b"] }],
                "tabs": [
                    { "id": "a", "groupId": "g" },
                    { "id": "l", "groupId": "g", "link": { "hostId": "h", "tabId": "r1" } },
                    { "id": "b", "groupId": "g" }
                ],
                "activeTabId": "l"
            }),
        )
        .unwrap();
        let open = || {
            let recorder = Arc::new(Recorder::default());
            let sessions = Arc::new(SessionManager::new(Arc::new(Taps::default()), recorder.clone()));
            let layout = Layout::open(paths.clone(), recorder.clone(), sessions.clone(), Box::new(|_| {}));
            (layout, sessions, recorder)
        };
        let order = |s: &LayoutSnapshot| -> Vec<String> { s.groups.iter().flat_map(|g| g.tab_ids.clone()).collect() };
        let (layout, sessions, recorder) = open();
        let snap = layout.snapshot();
        assert_eq!(order(&snap), ["a", "l", "b"]);
        assert_eq!(snap.tabs["l"].session_id, None, "a linked Tab is never spawned");
        assert!(snap.tabs["a"].session_id.is_some() && snap.tabs["b"].session_id.is_some());
        let keys: Vec<String> = sessions.keyed_targets().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["a", "b"], "no Resume key for the linked Tab");
        assert_eq!(snap.active_tab_id.as_deref(), Some("l"));

        // Remote's clients do not see it, and cannot name it.
        let client = layout.client_snapshot();
        assert_eq!(order(&client), ["a", "b"]);
        assert_eq!(client.active_tab_id, None);
        assert!(layout.refuse_linked("l").is_err() && layout.refuse_linked("a").is_ok());
        assert!(layout.client_tab_move("l", "g", None).is_err());
        layout.client_tab_move("b", "g", Some(0)).unwrap();
        assert_eq!(order(&layout.snapshot()), ["b", "a", "l"], "index 0 among [a, b]");

        // Linking: one event per change, none when nothing changes.
        let events = recorder.named(EVENT_LAYOUT).len();
        let link = TabLink { host_id: "h".into(), tab_id: "r2".into() };
        let made = layout.tab_link(link.clone(), None, Some("b")).unwrap();
        assert_eq!(order(&layout.snapshot()), ["b", made.id.as_str(), "a", "l"]);
        assert_eq!(layout.tab_link(link, None, None).unwrap().id, made.id);
        assert_eq!(recorder.named(EVENT_LAYOUT).len(), events + 1);
        let linked = layout.links_reconcile("h", &["r2".into(), "r3".into()], "dell");
        assert_eq!(linked.len(), 1, "r3 was a stray");
        let snap = layout.snapshot();
        assert!(!snap.tabs.contains_key("l"), "r1 is gone from the Host");
        assert_eq!(snap.groups[1].name, "dell");
        assert!(layout.links_reconcile("h", &["r2".into(), "r3".into()], "dell").is_empty());
        assert_eq!(recorder.named(EVENT_LAYOUT).len(), events + 2);
        // Closing a linked Tab here kills nothing.
        layout.tab_close(&made.id).unwrap();
        assert_eq!(sessions.probe_targets().len(), 2);

        // A relaunch: the same order, links and all. (The layout goes before its Sessions, so
        // their exits change nothing on disk.)
        layout.flush();
        let before = order(&layout.snapshot());
        drop(layout);
        sessions.kill_all();
        assert!(wait_until(T, || sessions.probe_targets().is_empty()));
        let (layout, sessions, _) = open();
        assert_eq!(order(&layout.snapshot()), before);
        assert_eq!(sessions.probe_targets().len(), 2);
        drop(layout);
        sessions.kill_all();
        assert!(wait_until(T, || sessions.probe_targets().is_empty()));
    }
}
