//! The Remote server: axum on Tauri's tokio runtime, bound to 127.0.0.1 only.
//!
//! Routes:
//! - `GET /` redirects to `/m`, the phone's page; `/m` and everything under it serve the app's
//!   `index.html` (the SvelteKit SPA routes to `src/routes/m`); any other path is a built asset
//!   (`_app/...`, the manifest, the service worker, icons), read through Tauri's asset resolver
//!   (embedded in release builds; `../build` on disk in dev, so run `pnpm build` first).
//! - `POST /api/pair` `{code, name}`: pair a phone; returns its token.
//! - `GET /ws`: the phone's connection. Protocol (mirrored by `src/lib/mobile/protocol.ts`):
//!   the first text frame must be `{"t":"auth","token"}` within five seconds. Then, from the
//!   phone: `attach` / `detach` `{sessionId}`, `input` `{sessionId, data}`, `ping`. From the Mac:
//!   `hello` `{device, sidebar}`, `sidebar` `{sidebar}`, `attached` `{sessionId, cols, rows}`
//!   followed by a binary replay, `resized`, `exit` `{sessionId}`, `error` `{message}`, `pong`,
//!   and binary frames of output: a big-endian u32 Session id, then the bytes.
//!   Close codes: 4401 unknown token, 4408 no auth in time, 4429 fell too far behind (reconnect
//!   and replay), 1001 Remote turned off.

use super::tap::{Frame, Taps};
use super::{HubMsg, Inner, PairError};
use crate::model::SessionId;
use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream::{SplitSink, StreamExt};
use futures_util::SinkExt;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
/// WebSocket pings, which also check every attached Session is still attached.
const PING_EVERY: Duration = Duration::from_secs(5);

const CLOSE_UNAUTHORIZED: u16 = 4401;
const CLOSE_AUTH_TIMEOUT: u16 = 4408;
const CLOSE_FELL_BEHIND: u16 = 4429;
const CLOSE_GOING_AWAY: u16 = 1001;

/// The running server; `stop` ends it (connections close on the hub's `Shutdown`).
pub struct Handle {
    task: tauri::async_runtime::JoinHandle<()>,
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
    let router = Router::new()
        .route("/", get(root))
        .route("/api/pair", post(pair))
        .route("/ws", get(ws))
        .fallback(get(asset))
        .with_state(inner);
    let task = tauri::async_runtime::spawn(async move {
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
    // could set it too): the token is what admits a phone.
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

async fn ws(State(inner): State<Arc<Inner>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| connection(inner, socket))
}

#[derive(Deserialize)]
#[serde(tag = "t", rename_all = "lowercase", rename_all_fields = "camelCase")]
enum ClientMsg {
    Auth { token: String },
    Attach { session_id: SessionId },
    Detach { session_id: SessionId },
    Input { session_id: SessionId, data: String },
    Ping,
}

type Sink = SplitSink<WebSocket, Message>;

async fn send_json(sink: &mut Sink, value: serde_json::Value) -> bool {
    sink.send(Message::Text(Utf8Bytes::from(value.to_string())))
        .await
        .is_ok()
}

async fn send_output(sink: &mut Sink, id: SessionId, bytes: &[u8]) -> bool {
    let mut frame = Vec::with_capacity(4 + bytes.len());
    frame.extend_from_slice(&id.to_be_bytes());
    frame.extend_from_slice(bytes);
    sink.send(Message::Binary(Bytes::from(frame))).await.is_ok()
}

async fn close(sink: &mut Sink, code: u16, reason: &str) {
    let _ = sink
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: Utf8Bytes::from(reason),
        })))
        .await;
}

/// Counts the phone as connected for as long as it lives.
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

    let sidebar = inner.sidebar();
    if !send_json(
        &mut sink,
        json!({ "t": "hello", "device": device, "sidebar": sidebar.as_deref() }),
    )
    .await
    {
        return;
    }

    let mut hub = inner.hub_subscribe();
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
                    Ok(HubMsg::Sidebar(sidebar)) => {
                        send_json(&mut sink, json!({ "t": "sidebar", "sidebar": *sidebar })).await
                    }
                    Ok(HubMsg::Shutdown) | Err(RecvError::Closed) => {
                        close(&mut sink, CLOSE_GOING_AWAY, "remote off").await;
                        break;
                    }
                    // Fell behind on sidebar snapshots: only the latest matters.
                    Err(RecvError::Lagged(_)) => match inner.sidebar() {
                        Some(sidebar) => send_json(&mut sink, json!({ "t": "sidebar", "sidebar": *sidebar })).await,
                        None => true,
                    },
                };
                if !ok {
                    break;
                }
            }
            _ = ping.tick() => {
                // A subscription the taps dropped means this phone fell too far behind (it
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
    let Some(asset) = inner.app().asset_resolver().get(file.to_string()) else {
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
        asset.mime_type.as_str()
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
