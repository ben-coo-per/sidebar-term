//! The Remote server: axum on the Host's tokio runtime (`host::Host::runtime`: Tauri's in the
//! app, the daemon's own), bound to 127.0.0.1 only. It serves the Host protocol
//! (docs/architecture.md "Host protocol"; types in `src/lib/host/protocol.ts`).
//!
//! Routes:
//! - `GET /` redirects to `/m`, the phone's page; `/m` and everything under it serve the app's
//!   `index.html` (the SvelteKit SPA routes to `src/routes/m`); any other path is a built asset
//!   (`_app/...`, the manifest, the service worker, icons), read through `host::Assets` (the app:
//!   Tauri's asset resolver, embedded in release builds, `../build` on disk in dev, so run
//!   `pnpm build` first; the daemon: its `--web-root` directory).
//! - `POST /api/pair` `{code, name}`: pair a client; returns its token.
//! - `POST /api/upload`: multipart, `Authorization: Bearer <token>`; the first file part is
//!   written under `<data dir>/uploads/<stamp>/<name>` and `{path}` comes back, so a file
//!   dragged onto a remote Tab can be attached by path, as `drop.rs` does on the Mac.
//!   Both answer CORS preflights for the Mac app's webview, whose page is another origin
//!   (`tauri://localhost`; `http://localhost:1420` in dev); no other origin is allowed.
//! - `GET /ws`: a client's connection. The first text frame must be `{"t":"auth","token"}`
//!   within five seconds. Then, from the client: `attach` / `detach` `{sessionId}`, `input`
//!   `{sessionId, data}`, `ping`, and the commands, each with a client-chosen `id` answered by
//!   `ok {id, result?}` or `error {id, message}`: `resize {sessionId, cols, rows}` (only for
//!   the one client attached), `tab_new`, `tab_close`, `tab_rename`, `tab_move`,
//!   `tab_activate`, `group_new`, `group_rename`, `group_move`, `group_delete`,
//!   `group_set_collapsed` (the layout's, one to one). From the Host: `hello {host, device,
//!   layout, sessions}`, `layout {layout}` on change, `session {session}` on each change to a
//!   Session's facts, `activity {sessions}` while the Host samples, `attached {sessionId, cols,
//!   rows}` followed by a binary replay, `resized`, `exit {sessionId}`, `error {message}` (no
//!   id: the message could not be read, or `input` failed), `pong`, and binary frames of
//!   output: a big-endian u32 Session id, then the bytes.
//!   Close codes: 4401 unknown token, 4408 no auth in time, 4429 fell too far behind (reconnect
//!   and replay), 1001 Remote turned off.

use super::tap::{Frame, Taps};
use super::{HubMsg, Inner, PairError};
use crate::layout::TabNew;
use crate::model::SessionId;
use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream::{SplitSink, StreamExt};
use futures_util::SinkExt;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast::error::RecvError;
use tower_http::cors::CorsLayer;

const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
/// The origins the API routes answer CORS preflights for: the Mac app's webview page
/// (Tauri's custom scheme on macOS) and its dev server (`pnpm dev`, either spelling of the
/// loopback). A browser on the tailnet is not one.
const APP_ORIGINS: [&str; 3] = ["tauri://localhost", "http://localhost:1420", "http://127.0.0.1:1420"];
/// WebSocket pings, which also check every attached Session is still attached.
const PING_EVERY: Duration = Duration::from_secs(5);
/// The most an upload may be.
const UPLOAD_MAX: usize = 64 * 1024 * 1024;

const CLOSE_UNAUTHORIZED: u16 = 4401;
const CLOSE_AUTH_TIMEOUT: u16 = 4408;
const CLOSE_FELL_BEHIND: u16 = 4429;
const CLOSE_GOING_AWAY: u16 = 1001;

/// The running server; `stop` ends it (connections close on the hub's `Shutdown`).
pub struct Handle {
    task: tokio::task::JoinHandle<()>,
}

impl Handle {
    pub fn stop(self) {
        self.task.abort();
    }
}

/// Bind `127.0.0.1:port` now (so a busy port is an error here) and serve on the runtime.
pub fn start(inner: Arc<Inner>, port: u16) -> Result<Handle, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("port {port} on 127.0.0.1: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("listener: {e}"))?;
    let runtime = inner.runtime().clone();
    let api = Router::new()
        .route("/api/pair", post(pair))
        .route(
            "/api/upload",
            post(upload).layer(DefaultBodyLimit::max(UPLOAD_MAX)),
        )
        .layer(api_cors());
    let router = Router::new()
        .route("/", get(root))
        .merge(api)
        .route("/ws", get(ws))
        .fallback(get(asset))
        .with_state(inner);
    let task = runtime.spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("remote: listener: {e}");
                return;
            }
        };
        if let Err(e) = axum::serve(listener, router).await {
            eprintln!("remote: server ended: {e}");
        }
    });
    Ok(Handle { task })
}

/// CORS for the API routes, for the Mac app's webview only (`APP_ORIGINS`): a browser lets a
/// cross-origin `fetch` through only when the preflight names its origin. What admits a client
/// is unchanged: the pairing code, then the token.
fn api_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(APP_ORIGINS.map(HeaderValue::from_static))
        .allow_methods([Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
}

async fn root() -> Redirect {
    Redirect::temporary("/m")
}

#[derive(Deserialize)]
struct PairRequest {
    code: String,
    #[serde(default)]
    name: String,
}

async fn pair(
    State(inner): State<Arc<Inner>>,
    headers: HeaderMap,
    Json(req): Json<PairRequest>,
) -> Response {
    // Set by Tailscale Serve on requests it proxies; recorded, not trusted (a local process
    // could set it too): the token is what admits a client.
    let login = headers
        .get("tailscale-user-login")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let refused = |message: String| {
        (StatusCode::FORBIDDEN, Json(json!({ "error": message }))).into_response()
    };
    match inner.try_pair(&req.code, &req.name, login) {
        Ok((token, device)) => Json(json!({ "token": token, "device": device })).into_response(),
        Err(PairError::NoPairing) => {
            refused("No pairing in progress. Start one in Settings on the Mac.".into())
        }
        Err(PairError::Wrong { left }) => refused(format!(
            "Wrong code. {left} {} left.",
            if left == 1 { "try" } else { "tries" }
        )),
        Err(PairError::Locked) => {
            refused("Too many wrong codes. Start a new pairing on the Mac.".into())
        }
        Err(PairError::Expired) => {
            refused("That code has expired. Start a new pairing on the Mac.".into())
        }
    }
}

/// The token from `Authorization: Bearer <token>`.
fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

/// A file name safe to write under the upload dir: its final component, or `upload`.
fn upload_name(name: Option<&str>) -> String {
    name.map(Path::new)
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty() && n != "." && n != "..")
        .unwrap_or_else(|| "upload".into())
}

/// Where one upload goes: its own stamped directory, so names never collide.
fn upload_path(dir: &Path, name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    dir.join(nanos.to_string()).join(name)
}

async fn upload(
    State(inner): State<Arc<Inner>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    let Some(token) = bearer(&headers) else {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "no token" }))).into_response();
    };
    if inner.verify_token(token).is_none() {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "unknown token" }))).into_response();
    }
    let bad = |status: StatusCode, message: String| {
        (status, Json(json!({ "error": message }))).into_response()
    };
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => return bad(StatusCode::BAD_REQUEST, "no file in the upload".into()),
            Err(e) => return bad(StatusCode::BAD_REQUEST, format!("bad upload: {e}")),
        };
        let name = upload_name(field.file_name());
        let is_file = field.file_name().is_some() || field.name() == Some("file");
        if !is_file {
            continue;
        }
        let bytes = match field.bytes().await {
            Ok(b) => b,
            Err(e) => return bad(StatusCode::BAD_REQUEST, format!("bad upload: {e}")),
        };
        let dir = match inner.upload_dir() {
            Ok(d) => d,
            Err(e) => return bad(StatusCode::INTERNAL_SERVER_ERROR, e),
        };
        let path = upload_path(&dir, &name);
        let written = tokio::task::spawn_blocking({
            let path = path.clone();
            move || -> Result<(), String> {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::write(&path, &bytes).map_err(|e| e.to_string())
            }
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
        return match written {
            Ok(()) => Json(json!({ "path": path.to_string_lossy() })).into_response(),
            Err(e) => bad(StatusCode::INTERNAL_SERVER_ERROR, format!("writing the file: {e}")),
        };
    }
}

async fn ws(State(inner): State<Arc<Inner>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| connection(inner, socket))
}

/// Text frames a client sends. The commands carry a client-chosen `id` that the reply echoes.
#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "snake_case", rename_all_fields = "camelCase")]
enum ClientMsg {
    Auth { token: String },
    Attach { session_id: SessionId },
    Detach { session_id: SessionId },
    Input { session_id: SessionId, data: String },
    Ping,
    Resize { id: u64, session_id: SessionId, cols: u16, rows: u16 },
    TabNew {
        id: u64,
        group_id: Option<String>,
        after_tab_id: Option<String>,
        cwd: Option<String>,
        cols: Option<u16>,
        rows: Option<u16>,
    },
    TabClose { id: u64, tab_id: String },
    TabRename { id: u64, tab_id: String, title: String },
    TabMove { id: u64, tab_id: String, group_id: String, index: Option<usize> },
    TabActivate { id: u64, tab_id: String },
    GroupNew { id: u64, name: Option<String>, tab_id: Option<String> },
    GroupRename { id: u64, group_id: String, name: String },
    GroupMove { id: u64, group_id: String, index: usize },
    GroupDelete { id: u64, group_id: String },
    GroupSetCollapsed { id: u64, group_id: String, collapsed: bool },
}

/// A layout command's outcome: `Ok(result)` becomes `ok {id, result}`.
type Outcome = Result<serde_json::Value, String>;

/// Run a layout command off the runtime (it takes a lock and may spawn a pty).
async fn run_command(inner: &Arc<Inner>, msg: ClientMsg) -> Option<(u64, Outcome)> {
    let inner = inner.clone();
    let (id, job): (u64, Box<dyn FnOnce() -> Outcome + Send>) = match msg {
        ClientMsg::TabNew { id, group_id, after_tab_id, cwd, cols, rows } => (
            id,
            Box::new(move || {
                inner
                    .layout()
                    .tab_new(TabNew { group_id, after_tab_id, cwd, cols, rows })
                    .map(|tab| json!(tab))
            }),
        ),
        ClientMsg::TabClose { id, tab_id } => {
            (id, Box::new(move || inner.layout().tab_close(&tab_id).map(|()| json!(null))))
        }
        ClientMsg::TabRename { id, tab_id, title } => (
            id,
            Box::new(move || inner.layout().tab_rename(&tab_id, &title).map(|()| json!(null))),
        ),
        ClientMsg::TabMove { id, tab_id, group_id, index } => (
            id,
            Box::new(move || {
                inner
                    .layout()
                    .tab_move(&tab_id, &group_id, index)
                    .map(|()| json!(null))
            }),
        ),
        ClientMsg::TabActivate { id, tab_id } => (
            id,
            Box::new(move || inner.layout().tab_activate(&tab_id).map(|()| json!(null))),
        ),
        ClientMsg::GroupNew { id, name, tab_id } => (
            id,
            Box::new(move || {
                inner
                    .layout()
                    .group_new(name.as_deref(), tab_id.as_deref())
                    .map(|group| json!(group))
            }),
        ),
        ClientMsg::GroupRename { id, group_id, name } => (
            id,
            Box::new(move || inner.layout().group_rename(&group_id, &name).map(|()| json!(null))),
        ),
        ClientMsg::GroupMove { id, group_id, index } => (
            id,
            Box::new(move || inner.layout().group_move(&group_id, index).map(|()| json!(null))),
        ),
        ClientMsg::GroupDelete { id, group_id } => (
            id,
            Box::new(move || inner.layout().group_delete(&group_id).map(|()| json!(null))),
        ),
        ClientMsg::GroupSetCollapsed { id, group_id, collapsed } => (
            id,
            Box::new(move || {
                inner
                    .layout()
                    .group_set_collapsed(&group_id, collapsed)
                    .map(|()| json!(null))
            }),
        ),
        _ => return None,
    };
    let outcome = tokio::task::spawn_blocking(job)
        .await
        .unwrap_or_else(|e| Err(format!("the command panicked: {e}")));
    Some((id, outcome))
}

type Sink = SplitSink<WebSocket, Message>;

async fn send_json(sink: &mut Sink, value: serde_json::Value) -> bool {
    sink.send(Message::Text(Utf8Bytes::from(value.to_string())))
        .await
        .is_ok()
}

async fn send_reply(sink: &mut Sink, id: u64, outcome: Outcome) -> bool {
    let reply = match outcome {
        Ok(serde_json::Value::Null) => json!({ "t": "ok", "id": id }),
        Ok(result) => json!({ "t": "ok", "id": id, "result": result }),
        Err(message) => json!({ "t": "error", "id": id, "message": message }),
    };
    send_json(sink, reply).await
}

async fn send_output(sink: &mut Sink, id: SessionId, bytes: &[u8]) -> bool {
    let mut frame = Vec::with_capacity(4 + bytes.len());
    frame.extend_from_slice(&id.to_be_bytes());
    frame.extend_from_slice(bytes);
    sink.send(Message::Binary(Bytes::from(frame))).await.is_ok()
}

async fn send_layout(sink: &mut Sink, inner: &Inner) -> bool {
    send_json(sink, json!({ "t": "layout", "layout": inner.layout().snapshot() })).await
}

/// The facts of Session `id`, if it is still known (a Session gone since the notification is
/// nothing to tell: its Tab left with it in the layout).
async fn send_session(sink: &mut Sink, inner: &Inner, id: SessionId) -> bool {
    match inner.layout().fact(id) {
        Some(info) => send_json(sink, json!({ "t": "session", "session": info })).await,
        None => true,
    }
}

async fn close(sink: &mut Sink, code: u16, reason: &str) {
    let _ = sink
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: Utf8Bytes::from(reason),
        })))
        .await;
}

/// Counts the client as connected for as long as it lives.
struct Connected(Arc<Inner>);

impl Drop for Connected {
    fn drop(&mut self) {
        self.0.client_disconnected();
    }
}

async fn connection(inner: Arc<Inner>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();

    // Authenticate first, or go away.
    let first = tokio::time::timeout(AUTH_TIMEOUT, stream.next()).await;
    let token = match first {
        Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<ClientMsg>(&text) {
            Ok(ClientMsg::Auth { token }) => token,
            _ => {
                close(&mut sink, CLOSE_UNAUTHORIZED, "auth first").await;
                return;
            }
        },
        Ok(_) => {
            close(&mut sink, CLOSE_UNAUTHORIZED, "auth first").await;
            return;
        }
        Err(_) => {
            close(&mut sink, CLOSE_AUTH_TIMEOUT, "no auth").await;
            return;
        }
    };
    let Some(device) = inner.verify_token(&token) else {
        close(&mut sink, CLOSE_UNAUTHORIZED, "unknown token").await;
        return;
    };
    inner.client_connected();
    let _connected = Connected(inner.clone());

    // Subscribe before reading the state, so nothing changes unseen between the two; a change
    // queued before the read is re-sent, which a client takes as the same state.
    let mut hub = inner.hub_subscribe();
    let hello = json!({
        "t": "hello",
        "host": inner.host_info(),
        "device": device,
        "layout": inner.layout().snapshot(),
        "sessions": inner.layout().facts(),
    });
    if !send_json(&mut sink, hello).await {
        return;
    }

    let (tx, mut rx) = Taps::channel();
    let taps = inner.taps();
    let mut attached: HashMap<SessionId, u64> = HashMap::new();
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await; // the first tick is immediate

    loop {
        tokio::select! {
            msg = stream.next() => {
                let text = match msg {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(_)) => continue,
                };
                let parsed = match serde_json::from_str::<ClientMsg>(&text) {
                    Ok(m) => m,
                    Err(e) => {
                        if !send_json(&mut sink, json!({ "t": "error", "message": format!("bad message: {e}") })).await {
                            break;
                        }
                        continue;
                    }
                };
                match parsed {
                    ClientMsg::Auth { .. } => {}
                    ClientMsg::Ping => {
                        if !send_json(&mut sink, json!({ "t": "pong" })).await {
                            break;
                        }
                    }
                    ClientMsg::Attach { session_id } => {
                        if let Some(old) = attached.remove(&session_id) {
                            taps.detach(session_id, old);
                        }
                        match taps.attach(session_id, tx.clone()) {
                            Some(a) => {
                                attached.insert(session_id, a.sub);
                                let ok = send_json(&mut sink, json!({
                                    "t": "attached", "sessionId": session_id, "cols": a.cols, "rows": a.rows
                                })).await && send_output(&mut sink, session_id, &a.scrollback).await;
                                if !ok {
                                    break;
                                }
                            }
                            None => {
                                if !send_json(&mut sink, json!({ "t": "exit", "sessionId": session_id })).await {
                                    break;
                                }
                            }
                        }
                    }
                    ClientMsg::Detach { session_id } => {
                        if let Some(sub) = attached.remove(&session_id) {
                            taps.detach(session_id, sub);
                        }
                    }
                    ClientMsg::Input { session_id, data } => {
                        if let Err(e) = inner.write_input(session_id, &data) {
                            if !send_json(&mut sink, json!({ "t": "error", "message": e })).await {
                                break;
                            }
                        }
                    }
                    ClientMsg::Resize { id, session_id, cols, rows } => {
                        let outcome = if attached.contains_key(&session_id) {
                            let others = taps.subscribers(session_id).saturating_sub(1);
                            inner.resize(session_id, cols, rows, others).map(|()| json!(null))
                        } else {
                            Err(format!("not attached to Session {session_id}"))
                        };
                        if !send_reply(&mut sink, id, outcome).await {
                            break;
                        }
                    }
                    command => {
                        if let Some((id, outcome)) = run_command(&inner, command).await {
                            if !send_reply(&mut sink, id, outcome).await {
                                break;
                            }
                        }
                    }
                }
            }
            frame = rx.recv() => {
                let Some((id, frame)) = frame else { break };
                let ok = match frame {
                    Frame::Output(bytes) => send_output(&mut sink, id, &bytes).await,
                    Frame::Resized { cols, rows } => send_json(&mut sink, json!({
                        "t": "resized", "sessionId": id, "cols": cols, "rows": rows
                    })).await,
                    Frame::Exit => {
                        attached.remove(&id);
                        send_json(&mut sink, json!({ "t": "exit", "sessionId": id })).await
                    }
                };
                if !ok {
                    break;
                }
            }
            msg = hub.recv() => {
                let ok = match msg {
                    Ok(HubMsg::Layout) => send_layout(&mut sink, &inner).await,
                    Ok(HubMsg::Session(id)) => send_session(&mut sink, &inner, id).await,
                    Ok(HubMsg::Activity(sessions)) => {
                        send_json(&mut sink, json!({ "t": "activity", "sessions": *sessions })).await
                    }
                    Ok(HubMsg::Shutdown) | Err(RecvError::Closed) => {
                        close(&mut sink, CLOSE_GOING_AWAY, "remote off").await;
                        break;
                    }
                    // Fell behind: only the latest matters, so send the whole of it.
                    Err(RecvError::Lagged(_)) => {
                        let mut ok = send_layout(&mut sink, &inner).await;
                        for info in inner.layout().facts() {
                            ok = ok && send_json(&mut sink, json!({ "t": "session", "session": info })).await;
                        }
                        ok
                    }
                };
                if !ok {
                    break;
                }
            }
            _ = ping.tick() => {
                // A subscription the taps dropped means this client fell too far behind (it
                // reconnects and replays), unless the Session simply ended.
                let mut fell_behind = false;
                let mut ended = Vec::new();
                for (&id, &sub) in &attached {
                    if !taps.attached(id, sub) {
                        if taps.has(id) {
                            fell_behind = true;
                        } else {
                            ended.push(id);
                        }
                    }
                }
                if fell_behind {
                    close(&mut sink, CLOSE_FELL_BEHIND, "fell behind").await;
                    break;
                }
                let mut ok = true;
                for id in ended {
                    attached.remove(&id);
                    ok &= send_json(&mut sink, json!({ "t": "exit", "sessionId": id })).await;
                }
                if !ok || sink.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
            }
        }
    }

    for (id, sub) in attached {
        taps.detach(id, sub);
    }
}

/// The phone's page and the built assets.
async fn asset(State(inner): State<Arc<Inner>>, uri: Uri) -> Response {
    let path = uri.path();
    let is_page = path == "/m" || path.starts_with("/m/") || path == "/index.html";
    let file = if is_page { "/index.html" } else { path };
    let Some(asset) = inner.assets().asset(file) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let immutable = file.starts_with("/_app/immutable/");
    let cache = if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    // Tauri sniffs the type from the bytes and a few extensions; the manifest is neither.
    let mime = if file.ends_with(".webmanifest") {
        "application/manifest+json"
    } else {
        asset.mime.as_str()
    };
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, cache),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::X_FRAME_OPTIONS, "DENY"),
        ],
        asset.bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_messages_parse_with_their_ids() {
        let m: ClientMsg = serde_json::from_str(r#"{"t":"tab_new","id":7,"cwd":"/tmp"}"#).unwrap();
        assert!(matches!(m, ClientMsg::TabNew { id: 7, cwd: Some(ref c), group_id: None, .. } if c == "/tmp"));
        let m: ClientMsg =
            serde_json::from_str(r#"{"t":"tab_move","id":1,"tabId":"t","groupId":"g","index":2}"#).unwrap();
        assert!(matches!(m, ClientMsg::TabMove { id: 1, index: Some(2), .. }));
        let m: ClientMsg =
            serde_json::from_str(r#"{"t":"group_set_collapsed","id":2,"groupId":"g","collapsed":true}"#).unwrap();
        assert!(matches!(m, ClientMsg::GroupSetCollapsed { collapsed: true, .. }));
        let m: ClientMsg =
            serde_json::from_str(r#"{"t":"resize","id":3,"sessionId":4,"cols":100,"rows":30}"#).unwrap();
        assert!(matches!(m, ClientMsg::Resize { id: 3, session_id: 4, cols: 100, rows: 30 }));
        assert!(matches!(serde_json::from_str::<ClientMsg>(r#"{"t":"ping"}"#).unwrap(), ClientMsg::Ping));
        assert!(serde_json::from_str::<ClientMsg>(r#"{"t":"tab_close","tabId":"t"}"#).is_err(), "no id");
        assert!(serde_json::from_str::<ClientMsg>(r#"{"t":"nope"}"#).is_err());
    }

    #[test]
    fn upload_names_stay_inside_the_upload_dir() {
        assert_eq!(upload_name(Some("Screenshot.png")), "Screenshot.png");
        assert_eq!(upload_name(Some("../../etc/passwd")), "passwd");
        assert_eq!(upload_name(Some("dir/")), "dir");
        assert_eq!(upload_name(Some("")), "upload");
        assert_eq!(upload_name(Some("..")), "upload");
        assert_eq!(upload_name(None), "upload");
        let path = upload_path(Path::new("/data/uploads"), "a.png");
        assert!(path.starts_with("/data/uploads"));
        assert_eq!(path.file_name().unwrap(), "a.png");
        assert_eq!(path.components().count(), 5, "/data/uploads/<stamp>/a.png");
    }

    #[test]
    fn the_bearer_token_is_read_from_the_header() {
        let mut h = HeaderMap::new();
        assert_eq!(bearer(&h), None);
        h.insert(header::AUTHORIZATION, "Bearer abc".parse().unwrap());
        assert_eq!(bearer(&h), Some("abc"));
        h.insert(header::AUTHORIZATION, "Basic abc".parse().unwrap());
        assert_eq!(bearer(&h), None);
        h.insert(header::AUTHORIZATION, "Bearer ".parse().unwrap());
        assert_eq!(bearer(&h), None);
    }
}
