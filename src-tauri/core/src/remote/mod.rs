//! Remote: a Host serving its Sessions, Tabs and Groups to clients over the Host protocol. While
//! on, an HTTP + WebSocket server listens on 127.0.0.1 (never on the LAN) and Tailscale Serve
//! publishes it to the tailnet as `https://<this Host>.ts.net`, with a real certificate, so a
//! phone's browser can install the page as an app. A client must pair once (a code shown in
//! Settings on the Mac, or printed by `sidebar-termd`) and then sends its token on every
//! connection. See docs/architecture.md "Host protocol".
//!
//! - `tap.rs`: each Session's recent output, attached clients and the marks read in its output
//!   (the OSC title, BELs), fed by `session.rs`.
//! - `auth.rs`: the store of paired clients (`remote.json`) and pairing codes.
//! - `tailscale.rs`: the Tailscale CLI (status, Serve on / off).
//! - `server.rs`: the axum routes: the page, pairing, the upload, the WebSocket.
//!
//! What a client shows is the Host's own: the layout (`layout/`) tells `publish` of every
//! change to itself or to a Session's facts (`SessionInfo`, Agent status included), and Remote
//! relays it to every client, which reads the latest back from the layout; `hello` carries the
//! whole of both. The layout's commands are served over the socket, one to one.

mod auth;
mod server;
mod tailscale;
pub mod tap;

use crate::agents::Agents;
use crate::host::{Assets, Host};
use crate::layout::{Layout, Update};
use crate::model::{
    ActivitySnapshot, HostInfo, LinkedHost, Pairing, RemoteDevice, RemoteSnapshot, SessionId,
    TailscaleState, EVENT_REMOTE,
};
use crate::session::SessionManager;
use crate::store;
use auth::{PairingCode, Store, Verdict};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

pub use tap::Taps;

/// Where `POST /api/upload` puts files, under the Host's data dir.
pub const UPLOAD_DIR: &str = "uploads";

/// Remote (app state, and the daemon's). Everything lives in `Inner`, shared with the server's
/// tasks.
pub struct Remote {
    inner: Arc<Inner>,
}

/// What the hub pushes to every connected client. Layout and Session changes carry no data:
/// the connection reads the latest from the layout, so a client that lags sees the newest.
#[derive(Clone)]
pub(crate) enum HubMsg {
    /// The layout changed.
    Layout,
    /// A Session's facts changed.
    Session(SessionId),
    /// An Activity sample: each Session's CPU and memory, while the Host samples.
    Activity(Arc<serde_json::Value>),
    /// An agent did something (`agents/`): one `AgentEvent`.
    AgentEvent(Arc<serde_json::Value>),
    /// The Hosts this one's linked Tabs point at changed (`Remote::set_hosts`).
    Hosts,
    /// Remote is turning off: every connection closes.
    Shutdown,
}

pub(crate) struct Inner {
    host: Host,
    sessions: Arc<SessionManager>,
    taps: Arc<Taps>,
    layout: Arc<Layout>,
    agents: Agents,
    /// `remote.json`; `None` when the app data dir is unavailable (pairings then last one run).
    path: Option<PathBuf>,
    store: Mutex<Store>,
    pairing: Mutex<Option<PairingCode>>,
    hub: broadcast::Sender<HubMsg>,
    clients: AtomicU32,
    server: Mutex<Option<server::Handle>>,
    /// Why the server is not listening although Remote is on.
    error: Mutex<Option<String>>,
    tailscale: Mutex<TailscaleState>,
    /// `https://<dns name>/m` while Serve publishes the server.
    url: Mutex<Option<String>>,
    /// The Hosts this one's linked Tabs point at, as the Mac app's webview last said; none on
    /// the daemon, which links no Tabs.
    hosts: Mutex<Vec<LinkedHost>>,
}

/// Why a phone's pairing request was refused.
pub(crate) enum PairError {
    NoPairing,
    Wrong { left: u32 },
    Locked,
    Expired,
}

impl Remote {
    /// Load `remote.json` at `path`. The server is not started: see `start_if_enabled` and `set`.
    /// Watches `layout` for what to relay to clients.
    pub fn open(
        host: Host,
        sessions: Arc<SessionManager>,
        taps: Arc<Taps>,
        layout: Arc<Layout>,
        agents: Agents,
        path: Option<PathBuf>,
    ) -> Self {
        let store = path.as_deref().and_then(read_store).unwrap_or_default();
        let (hub, _) = broadcast::channel(256);
        let for_watch = hub.clone();
        layout.watch(Box::new(move |update| {
            // Err means no client right now; the updates are not owed to anyone.
            let _ = for_watch.send(match update {
                Update::Layout => HubMsg::Layout,
                Update::Session(id) => HubMsg::Session(id),
            });
        }));
        let for_events = hub.clone();
        agents.watch(Box::new(move |event| {
            if let Ok(value) = serde_json::to_value(event) {
                let _ = for_events.send(HubMsg::AgentEvent(Arc::new(value)));
            }
        }));
        Self {
            inner: Arc::new(Inner {
                host,
                sessions,
                taps,
                layout,
                agents,
                path,
                store: Mutex::new(store),
                pairing: Mutex::new(None),
                hub,
                clients: AtomicU32::new(0),
                server: Mutex::new(None),
                error: Mutex::new(None),
                tailscale: Mutex::new(TailscaleState::default()),
                url: Mutex::new(None),
                hosts: Mutex::new(Vec::new()),
            }),
        }
    }

    /// At launch: if Remote was on when the app last ran, turn it on again (on a thread: the
    /// Tailscale CLI may take a moment).
    pub fn start_if_enabled(&self) {
        if !lock(&self.inner.store).enabled {
            return;
        }
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            if let Err(e) = inner.set(true) {
                eprintln!("remote: could not start at launch: {e}");
            }
        });
    }

    /// Turn Remote on or off. Blocking (runs the Tailscale CLI): call off the main thread.
    pub fn set(&self, on: bool) -> Result<RemoteSnapshot, String> {
        self.inner.set(on)
    }

    /// The port the server binds from now on, kept in `remote.json` (`sidebar-termd --port`).
    /// Takes effect the next time Remote is turned on.
    pub fn set_port(&self, port: u16) {
        let mut store = lock(&self.inner.store);
        if store.port() != port {
            store.port = port;
            self.inner.save(&store);
        }
    }

    /// Re-read Tailscale's state (blocking) and say where things stand.
    pub fn refresh(&self) -> RemoteSnapshot {
        self.inner.refresh_tailscale();
        self.inner.snapshot()
    }

    /// Start a pairing: the code a phone must present, valid for ten minutes.
    pub fn pair_begin(&self) -> Pairing {
        let now = now_ms();
        let code = PairingCode::new(now);
        let port = lock(&self.inner.store).port();
        let public = self.inner.pairing_public(&code, port);
        *lock(&self.inner.pairing) = Some(code);
        self.inner.changed();
        public
    }

    pub fn pair_cancel(&self) {
        *lock(&self.inner.pairing) = None;
        self.inner.changed();
    }

    /// Forget a paired phone; its token stops working at its next connection.
    pub fn revoke(&self, id: &str) {
        let removed = {
            let mut store = lock(&self.inner.store);
            let removed = store.revoke(id);
            if removed {
                self.inner.save(&store);
            }
            removed
        };
        if removed {
            self.inner.changed();
        }
    }

    /// The Hosts this one's linked Tabs point at (the Mac app's webview holds the pairings and
    /// says so at launch and on every change). A client that shows linked Tabs is told, so it
    /// can reach those Hosts itself, with a pairing of its own.
    pub fn set_hosts(&self, hosts: Vec<LinkedHost>) {
        {
            let mut held = lock(&self.inner.hosts);
            if *held == hosts {
                return;
            }
            *held = hosts;
        }
        let _ = self.inner.hub.send(HubMsg::Hosts);
    }

    /// An Activity sample (while the Host samples: the Mac's Panel, Memory Guard): each
    /// Session's CPU and memory goes to every client as `activity`.
    pub fn publish_activity(&self, snapshot: &ActivitySnapshot) {
        if self.inner.hub.receiver_count() == 0 {
            return;
        }
        match serde_json::to_value(&snapshot.sessions) {
            Ok(v) => {
                let _ = self.inner.hub.send(HubMsg::Activity(Arc::new(v)));
            }
            Err(e) => eprintln!("remote: the activity sample does not serialize: {e}"),
        }
    }
}

impl Inner {
    fn set(self: &Arc<Self>, on: bool) -> Result<RemoteSnapshot, String> {
        let result = if on { self.turn_on() } else { self.turn_off() };
        {
            let mut store = lock(&self.store);
            store.enabled = on;
            self.save(&store);
        }
        self.changed();
        result.map(|_| self.snapshot())
    }

    fn turn_on(self: &Arc<Self>) -> Result<(), String> {
        let port = lock(&self.store).port();
        {
            let mut server = lock(&self.server);
            if server.is_none() {
                match server::start(self.clone(), port) {
                    Ok(handle) => {
                        *server = Some(handle);
                        *lock(&self.error) = None;
                    }
                    Err(e) => {
                        *lock(&self.error) = Some(e.clone());
                        return Err(e);
                    }
                }
            }
        }
        self.refresh_tailscale();
        let (cli, dns_name) = {
            let ts = lock(&self.tailscale);
            (tailscale::find_cli(), ts.dns_name.clone())
        };
        let mut ts_error = None;
        let mut url = None;
        match (cli, dns_name) {
            (Some(cli), Some(name)) => match tailscale::serve_on(&cli, port) {
                Ok(()) => url = Some(format!("https://{name}/m")),
                Err(e) => ts_error = Some(format!("Tailscale Serve: {e}")),
            },
            (Some(_), None) => {}
            (None, _) => {}
        }
        *lock(&self.url) = url;
        if let Some(e) = ts_error {
            lock(&self.tailscale).error = Some(e);
        }
        Ok(())
    }

    fn turn_off(&self) -> Result<(), String> {
        if let Some(handle) = lock(&self.server).take() {
            let _ = self.hub.send(HubMsg::Shutdown);
            handle.stop();
        }
        *lock(&self.pairing) = None;
        *lock(&self.url) = None;
        *lock(&self.error) = None;
        if let Some(cli) = tailscale::find_cli() {
            if let Err(e) = tailscale::serve_off(&cli) {
                lock(&self.tailscale).error = Some(format!("Tailscale Serve: {e}"));
            }
        }
        Ok(())
    }

    fn refresh_tailscale(&self) {
        let state = tailscale::state();
        *lock(&self.tailscale) = state;
    }

    /// Lock order, here and everywhere: store, pairing, url, server, error. Never re-entered.
    fn snapshot(&self) -> RemoteSnapshot {
        let store = lock(&self.store);
        let pairing = {
            let mut pairing = lock(&self.pairing);
            if pairing.as_ref().is_some_and(|p| p.expired(now_ms())) {
                *pairing = None;
            }
            pairing.as_ref().map(|p| self.pairing_public(p, store.port()))
        };
        RemoteSnapshot {
            on: store.enabled,
            port: store.port(),
            url: lock(&self.url).clone(),
            error: lock(&self.error).clone(),
            tailscale: lock(&self.tailscale).clone(),
            clients: self.clients.load(Ordering::SeqCst),
            devices: store.public(),
            pairing,
        }
    }

    /// Where the phone opens the pairing: the tailnet URL, or the local one while Tailscale is
    /// not publishing the server (only useful from a browser on this Mac).
    fn pairing_public(&self, code: &PairingCode, port: u16) -> Pairing {
        let base = lock(&self.url)
            .clone()
            .or_else(|| lock(&self.server).is_some().then(|| format!("http://127.0.0.1:{port}/m")));
        Pairing {
            code: code.display(),
            url: base.map(|b| format!("{b}#pair={}", code.code)),
            expires_at: code.expires_at,
        }
    }

    fn changed(&self) {
        self.host.events.emit(EVENT_REMOTE, &self.snapshot());
    }

    fn save(&self, store: &Store) {
        if let Some(path) = &self.path {
            if let Err(e) = store::write(path, store) {
                eprintln!("remote: writing {} failed: {e}", path.display());
            }
        }
    }

    // --- Called by the server ---------------------------------------------------------------

    pub(crate) fn taps(&self) -> &Taps {
        &self.taps
    }

    pub(crate) fn assets(&self) -> &dyn Assets {
        &*self.host.assets
    }

    pub(crate) fn runtime(&self) -> &tokio::runtime::Handle {
        &self.host.runtime
    }

    pub(crate) fn hub_subscribe(&self) -> broadcast::Receiver<HubMsg> {
        self.hub.subscribe()
    }

    pub(crate) fn agents(&self) -> &Agents {
        &self.agents
    }

    pub(crate) fn layout(&self) -> &Arc<Layout> {
        &self.layout
    }

    /// This Host, for `hello`.
    pub(crate) fn host_info(&self) -> HostInfo {
        HostInfo {
            name: hostname(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            home: std::env::var("HOME").ok().filter(|h| !h.is_empty()),
        }
    }

    /// The Hosts this one's linked Tabs point at, for `hello` and `hosts`.
    pub(crate) fn hosts(&self) -> Vec<LinkedHost> {
        lock(&self.hosts).clone()
    }

    /// Whether a page at `origin` may call the API routes from a browser: a page another Host
    /// on this tailnet serves (the phone's page, installed from the Mac, pairing with this
    /// Host). `https://<machine>.<this tailnet>.ts.net`, with this Host's own tailnet.
    pub(crate) fn tailnet_origin(&self, origin: &str) -> bool {
        let ours = lock(&self.tailscale).dns_name.clone();
        ours.is_some_and(|ours| same_tailnet(&ours, origin))
    }

    /// Where uploads go: `<data dir>/uploads`, created.
    pub(crate) fn upload_dir(&self) -> Result<PathBuf, String> {
        let dir = self.host.paths.data_dir()?.join(UPLOAD_DIR);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(dir)
    }

    /// Size Session `id`'s pty for a client: when that client is the only one showing it (no
    /// other client on the socket, `others` counts them, this one excluded, and no Terminal
    /// attached in process, the Mac webview's), or when it takes the size (`take`): its user is
    /// at it, and every other client follows the pty (`resized`; ADR 0007).
    pub(crate) fn resize(
        &self,
        id: SessionId,
        cols: u16,
        rows: u16,
        others: usize,
        take: bool,
    ) -> Result<(), String> {
        if !may_size(take, others, self.layout.is_attached(id)) {
            return Err(format!(
                "Session {id} is shown by another client; only a client that takes the size may size it"
            ));
        }
        if cols == 0 || rows == 0 {
            return Err("cols and rows must be at least 1".into());
        }
        self.sessions.resize(id, cols, rows)
    }

    /// A phone presents a pairing code: on success it is paired and gets its token.
    pub(crate) fn try_pair(
        &self,
        code: &str,
        name: &str,
        login: Option<String>,
    ) -> Result<(String, RemoteDevice), PairError> {
        let now = now_ms();
        let verdict = {
            let mut pairing = lock(&self.pairing);
            let Some(p) = pairing.as_mut() else {
                return Err(PairError::NoPairing);
            };
            let verdict = p.present(code, now);
            let left = auth::MAX_ATTEMPTS.saturating_sub(p.attempts);
            match verdict {
                Verdict::Ok | Verdict::Locked | Verdict::Expired => *pairing = None,
                Verdict::Wrong => {}
            }
            (verdict, left)
        };
        let result = match verdict {
            (Verdict::Ok, _) => {
                let mut store = lock(&self.store);
                let added = store.add(name, login, now);
                self.save(&store);
                Ok(added)
            }
            (Verdict::Wrong, left) => Err(PairError::Wrong { left }),
            (Verdict::Locked, _) => Err(PairError::Locked),
            (Verdict::Expired, _) => Err(PairError::Expired),
        };
        self.changed();
        result
    }

    /// The name of the phone `token` belongs to, or None for a stranger.
    pub(crate) fn verify_token(&self, token: &str) -> Option<String> {
        let mut store = lock(&self.store);
        let id = store.verify(token, now_ms())?;
        self.save(&store);
        store.devices.iter().find(|d| d.id == id).map(|d| d.name.clone())
    }

    pub(crate) fn client_connected(&self) {
        self.clients.fetch_add(1, Ordering::SeqCst);
        self.changed();
    }

    pub(crate) fn client_disconnected(&self) {
        self.clients.fetch_sub(1, Ordering::SeqCst);
        self.changed();
    }

    pub(crate) fn write_input(&self, id: SessionId, data: &str) -> Result<(), String> {
        self.sessions.write(id, data.as_bytes())
    }
}

fn read_store(path: &Path) -> Option<Store> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes)
        .inspect_err(|e| eprintln!("remote: {} unreadable ({e}); starting fresh", path.display()))
        .ok()
}

/// This machine's hostname, or `"host"` when it cannot be read.
fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: a plain libc call into a buffer of the stated size.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return "host".into();
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..end]).trim().to_owned();
    if name.is_empty() {
        "host".into()
    } else {
        name
    }
}

/// Whether `origin` is `https://<machine>.<tailnet>`, a machine on the tailnet of `dns_name`
/// (`<this machine>.<tailnet>`, as `bens-mac.tail1234.ts.net`): one label, then the same tailnet.
fn same_tailnet(dns_name: &str, origin: &str) -> bool {
    let Some((_, tailnet)) = dns_name.trim_end_matches('.').split_once('.') else {
        return false;
    };
    let Some(host) = origin.strip_prefix("https://") else {
        return false;
    };
    let Some((machine, rest)) = host.split_once('.') else {
        return false;
    };
    let label = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    tailnet.contains('.') && label(machine) && rest.eq_ignore_ascii_case(tailnet)
}

/// Whether a client may size a pty: it takes the size, or no other client shows the Session
/// (`others`: on the socket; `in_process`: the Mac webview's Terminal).
fn may_size(take: bool, others: usize, in_process: bool) -> bool {
    take || (others == 0 && !in_process)
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_sizes_a_pty_it_alone_shows_or_takes() {
        assert!(may_size(false, 0, false), "the only client showing it");
        assert!(!may_size(false, 1, false), "another client on the socket shows it");
        assert!(!may_size(false, 0, true), "the Mac webview shows it");
        assert!(may_size(true, 1, false), "taken from another client");
        assert!(may_size(true, 0, true), "taken from the Mac webview");
        assert!(may_size(true, 2, true));
    }

    #[test]
    fn a_page_from_another_machine_on_this_tailnet_may_call_the_api() {
        let ours = "bennet.tail1234.ts.net";
        assert!(same_tailnet(ours, "https://bens-mac.tail1234.ts.net"));
        assert!(same_tailnet("bennet.tail1234.ts.net.", "https://bens-mac.tail1234.ts.net"));
        assert!(same_tailnet(ours, "https://bennet.tail1234.ts.net"), "this Host's own page");
        assert!(same_tailnet(ours, "https://Bens-Mac.Tail1234.TS.net"));

        assert!(!same_tailnet(ours, "http://bens-mac.tail1234.ts.net"), "not https");
        assert!(!same_tailnet(ours, "https://bens-mac.tail9999.ts.net"), "another tailnet");
        assert!(!same_tailnet(ours, "https://tail1234.ts.net"), "no machine");
        assert!(!same_tailnet(ours, "https://a.b.tail1234.ts.net"), "two labels");
        assert!(!same_tailnet(ours, "https://evil.example/.tail1234.ts.net"));
        assert!(!same_tailnet(ours, "https://bens-mac.tail1234.ts.net:8443"), "a port");
        assert!(!same_tailnet(ours, "https://bens-mac.tail1234.ts.net.evil.example"));
        assert!(!same_tailnet("localhost", "https://bens-mac.localhost"), "no tailnet to match");
        assert!(!same_tailnet("bennet.net", "https://mac.net"), "a bare suffix is no tailnet");
    }
}
