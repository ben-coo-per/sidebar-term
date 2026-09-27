//! Remote: the Mac app serving its Sessions to a phone. While on, an HTTP + WebSocket server
//! listens on 127.0.0.1 (never on the LAN) and Tailscale Serve publishes it to the tailnet as
//! `https://<this Mac>.ts.net`, with a real certificate, so the phone's browser can install the
//! page as an app. A phone must pair once (a code shown in Settings) and then sends its token on
//! every connection. See docs/architecture.md "Remote".
//!
//! - `tap.rs`: each Session's recent output and attached phones, fed by `session.rs`.
//! - `auth.rs`: the store of paired phones (`remote.json`) and pairing codes.
//! - `tailscale.rs`: the Tailscale CLI (status, Serve on / off).
//! - `server.rs`: the axum routes: the page, pairing, the WebSocket.
//!
//! The sidebar the phone shows (Groups, Tabs, Titles, Agent status) is the Mac webview's: it
//! sends a snapshot through `remote_sidebar` whenever it changes, and Remote relays it, opaque,
//! to every phone (ADR 0001: Rust does not know what a Tab is).

mod auth;
mod server;
mod tailscale;
pub mod tap;

use crate::layout;
use crate::model::{Pairing, RemoteDevice, RemoteSnapshot, SessionId, TailscaleState, EVENT_REMOTE};
use crate::session::SessionManager;
use auth::{PairingCode, Store, Verdict};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;

pub use tap::Taps;

/// Remote (Tauri state). Everything lives in `Inner`, shared with the server's tasks.
pub struct Remote {
    inner: Arc<Inner>,
}

/// What the hub pushes to every connected phone.
#[derive(Clone)]
pub(crate) enum HubMsg {
    /// The Mac webview's latest sidebar snapshot.
    Sidebar(Arc<serde_json::Value>),
    /// Remote is turning off: every connection closes.
    Shutdown,
}

pub(crate) struct Inner {
    app: AppHandle,
    taps: Arc<Taps>,
    /// `remote.json`; `None` when the app data dir is unavailable (pairings then last one run).
    path: Option<PathBuf>,
    store: Mutex<Store>,
    pairing: Mutex<Option<PairingCode>>,
    sidebar: Mutex<Option<Arc<serde_json::Value>>>,
    hub: broadcast::Sender<HubMsg>,
    clients: AtomicU32,
    server: Mutex<Option<server::Handle>>,
    /// Why the server is not listening although Remote is on.
    error: Mutex<Option<String>>,
    tailscale: Mutex<TailscaleState>,
    /// `https://<dns name>/m` while Serve publishes the server.
    url: Mutex<Option<String>>,
}

/// Why a phone's pairing request was refused.
pub(crate) enum PairError {
    NoPairing,
    Wrong { left: u32 },
    Locked,
    Expired,
}

impl Remote {
    /// Load `remote.json` at `path`. The server is not started: see `start_if_enabled`.
    pub fn open(app: AppHandle, taps: Arc<Taps>, path: Option<PathBuf>) -> Self {
        let store = path.as_deref().and_then(read_store).unwrap_or_default();
        let (hub, _) = broadcast::channel(16);
        Self {
            inner: Arc::new(Inner {
                app,
                taps,
                path,
                store: Mutex::new(store),
                pairing: Mutex::new(None),
                sidebar: Mutex::new(None),
                hub,
                clients: AtomicU32::new(0),
                server: Mutex::new(None),
                error: Mutex::new(None),
                tailscale: Mutex::new(TailscaleState::default()),
                url: Mutex::new(None),
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

    /// The Mac webview's sidebar changed: relay it to every phone.
    pub fn publish_sidebar(&self, sidebar: serde_json::Value) {
        let value = Arc::new(sidebar);
        *lock(&self.inner.sidebar) = Some(value.clone());
        let _ = self.inner.hub.send(HubMsg::Sidebar(value));
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
        if let Err(e) = self.app.emit(EVENT_REMOTE, self.snapshot()) {
            eprintln!("remote: emit failed: {e}");
        }
    }

    fn save(&self, store: &Store) {
        if let Some(path) = &self.path {
            if let Err(e) = layout::write(path, store) {
                eprintln!("remote: writing {} failed: {e}", path.display());
            }
        }
    }

    // --- Called by the server ---------------------------------------------------------------

    pub(crate) fn taps(&self) -> &Taps {
        &self.taps
    }

    pub(crate) fn app(&self) -> &AppHandle {
        &self.app
    }

    pub(crate) fn hub_subscribe(&self) -> broadcast::Receiver<HubMsg> {
        self.hub.subscribe()
    }

    pub(crate) fn sidebar(&self) -> Option<Arc<serde_json::Value>> {
        lock(&self.sidebar).clone()
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
        self.app.state::<SessionManager>().write(id, data.as_bytes())
    }
}

fn read_store(path: &Path) -> Option<Store> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes)
        .inspect_err(|e| eprintln!("remote: {} unreadable ({e}); starting fresh", path.display()))
        .ok()
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
