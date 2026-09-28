//! The hook endpoint: axum on the Host's tokio runtime, on 127.0.0.1 at a port the system picks,
//! always on (unlike Remote, which is the user's switch). `POST /hook/{event}` takes one Claude
//! Code hook, as `install.rs`'s hook script sends it: the payload as the body, the Session's id
//! in `x-sidebar-term-session` and the Host's token in `x-sidebar-term-token`. It answers with
//! the reply to print (JSON), or an empty body for none; a question's request stays open until
//! it is answered or let go (`Agents::hook`).

use super::Agents;
use crate::model::SessionId;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

/// A `PostToolUse` carries the tool's whole input and output: a big file written, say.
const BODY_MAX: usize = 32 * 1024 * 1024;

/// Bind 127.0.0.1 now and serve on `runtime`; the port bound.
pub fn start(agents: Agents, runtime: &tokio::runtime::Handle) -> Result<u16, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("hook endpoint: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| format!("hook endpoint: {e}"))?;
    let port = listener.local_addr().map_err(|e| format!("hook endpoint: {e}"))?.port();
    let router = Router::new()
        .route("/hook/{event}", post(hook))
        .layer(DefaultBodyLimit::max(BODY_MAX))
        .with_state(agents);
    runtime.spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("agents: hook endpoint: {e}");
                return;
            }
        };
        if let Err(e) = axum::serve(listener, router).await {
            eprintln!("agents: hook endpoint ended: {e}");
        }
    });
    Ok(port)
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

async fn hook(State(agents): State<Agents>, Path(event): Path<String>, headers: HeaderMap, body: Bytes) -> Response {
    if !agents.token_ok(header(&headers, "x-sidebar-term-token").unwrap_or("")) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(session) = header(&headers, "x-sidebar-term-session").and_then(|s| s.trim().parse::<SessionId>().ok()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let payload = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    match agents.hook(session, &event, payload).await {
        Some(reply) => Json(reply).into_response(),
        None => StatusCode::OK.into_response(),
    }
}

#[cfg(test)]
mod tests {
    //! The whole path: the hook script, through `curl`, to the endpoint, answered by the Host.

    use super::super::{install, Agents};
    use super::start;
    use crate::host::testing::Recorder;
    use crate::model::{AgentKind, AgentStatus, SessionInfo};
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn a_hook_is_held_until_answered_and_prints_the_reply() {
        if Command::new("curl").arg("--version").output().is_err() {
            eprintln!("no curl; skipped");
            return;
        }
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let agents = Agents::new(Arc::new(Recorder::default()));
        let port = start(agents.clone(), runtime.handle()).unwrap();
        let data = std::env::temp_dir().join(format!("sidebar-term-endpoint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let installed = install::install(&data).unwrap();
        let mut env = std::collections::BTreeMap::new();
        *agents.inner.hooks.lock().unwrap() = Some((format!("http://127.0.0.1:{port}"), installed));
        agents.extend_env(&mut env);

        let mut hook = Command::new(data.join(install::DIR).join("hook"))
            .arg("PermissionRequest")
            .env_clear()
            .envs(&env)
            .env("PATH", "/usr/bin:/bin")
            .env(crate::session::SESSION_ID_VAR, "7")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let payload = r#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        hook.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        let pending = loop {
            let mut info = SessionInfo { agent: Some(AgentKind::Claude), status: Some(AgentStatus::Running), ..SessionInfo::empty(7) };
            agents.decorate(&mut info);
            if let Some(p) = info.pending {
                break p;
            }
            assert!(Instant::now() < deadline, "the question never arrived");
            std::thread::sleep(Duration::from_millis(20));
        };
        agents.answer(7, pending.id, 0).unwrap();
        let out = hook.wait_with_output().unwrap();
        let reply: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(reply["hookSpecificOutput"]["decision"]["behavior"], "allow");

        // A wrong token is turned away, and the hook prints nothing.
        let out = Command::new(data.join(install::DIR).join("hook"))
            .arg("Stop")
            .env_clear()
            .envs(&env)
            .env("PATH", "/usr/bin:/bin")
            .env(install::ENV_TOKEN, "wrong")
            .env(crate::session::SESSION_ID_VAR, "7")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success() && out.stdout.is_empty());
        let _ = std::fs::remove_dir_all(&data);
    }
}
