//! What the Host knows about each Agent session beyond what the monitor probes, for Manager
//! (docs/architecture.md "Manager"):
//!
//! - **Status history**: every change of a Session's Agent status over the last few hours
//!   (`SessionInfo.history`), from which a client draws the Session's lane.
//! - **Hooks** (`hooks.rs`, `install.rs`, `server.rs`): a Claude Code started through
//!   sidebar-term's `claude` wrapper reports to a small server on 127.0.0.1. A question it asks
//!   (a permission, an AskUserQuestion) becomes `SessionInfo.pending`, and the hook is held
//!   open until a client answers it ([`Agents::answer`]) or lets it go ([`Agents::release`],
//!   when the user opens the Tab to answer there).
//! - **Agent events**: a feed of what every agent did (tools it used, questions asked, answers
//!   given; status changes of a screen-only agent), the last [`FEED_MAX`], on the
//!   `agent-event` event and over the Host protocol.
//!
//! - **Steps**: what a hooked agent's hooks say that the Journal counts (a prompt, an edit's
//!   lines, where its transcript is), to whoever watches them ([`Agents::watch_steps`]).
//!
//! The monitor calls [`Agents::decorate`] on every Session's facts each tick, before it compares
//! them with the last ones, so a new question or status change goes out like any other change.

pub mod hooks;
pub mod install;
mod server;

use crate::host::{Events, Host};
use crate::model::{
    AgentEvent, AgentEventKind, AgentKind, AgentStatus, Pending, SessionId, SessionInfo, StatusChange,
    EVENT_AGENT_EVENT,
};
use base64::Engine;
use rand::RngCore;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ffi::OsString;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;

/// How far back a Session's status history goes (Manager's widest zoom is 4 h).
pub const HISTORY_MS: u64 = 5 * 60 * 60 * 1000;
/// Changes kept per Session at most, however short they were.
const HISTORY_MAX: usize = 2000;
/// Agent events kept, every Session together.
pub const FEED_MAX: usize = 500;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Told of every agent event (Remote relays them to its clients).
pub type Watcher = Box<dyn Fn(&AgentEvent) + Send + Sync>;

/// Told of every step of a hooked agent, with its Session and the time (epoch ms).
pub type StepWatcher = Box<dyn Fn(SessionId, &hooks::Step, u64) + Send + Sync>;

#[derive(Clone)]
pub struct Agents {
    inner: Arc<Inner>,
}

struct Inner {
    events: Arc<dyn Events>,
    /// Every hook request must carry it (another user's process cannot answer for an agent).
    token: String,
    /// The hook endpoint's URL and the files the wrapper uses, once both are up.
    hooks: Mutex<Option<(String, install::Installed)>>,
    state: Mutex<State>,
    watchers: Mutex<Vec<Watcher>>,
    step_watchers: Mutex<Vec<StepWatcher>>,
    next_id: AtomicU64,
}

#[derive(Default)]
struct State {
    sessions: HashMap<SessionId, Tracked>,
    feed: VecDeque<AgentEvent>,
}

#[derive(Default)]
struct Tracked {
    history: Vec<StatusChange>,
    /// Its agent reported through its hooks (and has not ended since).
    hooked: bool,
    waiting: Option<Waiting>,
    /// The transcript its hooks last named.
    transcript: Option<String>,
}

/// A question an agent's hook is held open on.
struct Waiting {
    pending: Pending,
    /// The reply that gives each option.
    replies: Vec<Value>,
    /// Where the answer goes: `Some(reply)`, or `None` to let the agent ask in its Terminal.
    reply: oneshot::Sender<Option<Value>>,
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

impl Agents {
    /// Without hooks: history and screen-only events only. [`Agents::start`] adds them.
    pub fn new(events: Arc<dyn Events>) -> Self {
        Self {
            inner: Arc::new(Inner {
                events,
                token: new_token(),
                hooks: Mutex::new(None),
                state: Mutex::default(),
                watchers: Mutex::default(),
                step_watchers: Mutex::default(),
                next_id: AtomicU64::new(0),
            }),
        }
    }

    /// Write the wrapper and hook files under the Host's data dir and start the hook endpoint
    /// on 127.0.0.1 (a port of the system's choosing). Must run before any Session spawns, so
    /// each gets the environment ([`Agents::extend_env`]). Without a data dir, or when the port
    /// will not bind, agents simply run without hooks (said once, here).
    pub fn start(host: &Host) -> Self {
        let agents = Self::new(host.events.clone());
        let installed = host.paths.data_dir().and_then(|dir| install::install(&dir));
        let port = server::start(agents.clone(), &host.runtime);
        match (installed, port) {
            (Ok(installed), Ok(port)) => {
                *lock(&agents.inner.hooks) = Some((format!("http://127.0.0.1:{port}"), installed));
            }
            (Err(e), _) | (_, Err(e)) => eprintln!("agents: Claude Code hooks are off: {e}"),
        }
        agents
    }

    /// Add what a Session needs to reach the hooks to its environment (`session.rs` calls this
    /// for every spawn). Nothing while the hooks are off.
    pub fn extend_env(&self, env: &mut BTreeMap<OsString, OsString>) {
        let hooks = lock(&self.inner.hooks);
        let Some((url, installed)) = hooks.as_ref() else { return };
        let path = env.get(&OsString::from("PATH")).cloned();
        for (k, v) in install::session_env(installed, url, &self.inner.token, path.as_ref()) {
            env.insert(k, v);
        }
    }

    pub(crate) fn token_ok(&self, token: &str) -> bool {
        // Not secret-timing-safe; the token only keeps other users' processes out.
        !token.is_empty() && token == self.inner.token
    }

    /// Told of every agent event from now on.
    pub fn watch(&self, watcher: Watcher) {
        lock(&self.inner.watchers).push(watcher);
    }

    /// Told of every step of a hooked agent from now on.
    pub fn watch_steps(&self, watcher: StepWatcher) {
        lock(&self.inner.step_watchers).push(watcher);
    }

    fn step(&self, id: SessionId, step: hooks::Step, at: u64) {
        for w in lock(&self.inner.step_watchers).iter() {
            w(id, &step, at);
        }
    }

    /// The last [`FEED_MAX`] agent events, oldest first.
    pub fn recent(&self) -> Vec<AgentEvent> {
        lock(&self.inner.state).feed.iter().cloned().collect()
    }

    /// Add what the Host knows beyond the probe to a Session's fresh facts: the question it is
    /// waiting on (its status is then Needs input), whether it is hooked, and its status history
    /// with the status just derived. A screen-only agent's change of status is an event.
    pub fn decorate(&self, info: &mut SessionInfo) {
        self.decorate_at(info, now_ms());
    }

    fn decorate_at(&self, info: &mut SessionInfo, now: u64) {
        let mut fresh = Vec::new();
        {
            let mut state = lock(&self.inner.state);
            let t = state.sessions.entry(info.session_id).or_default();
            if info.agent != Some(AgentKind::Claude) {
                // Its agent is gone (or never was Claude Code): nothing can be waiting on it.
                t.hooked = false;
                if let Some(w) = t.waiting.take() {
                    let _ = w.reply.send(None);
                }
            }
            if let Some(w) = &t.waiting {
                info.status = Some(AgentStatus::NeedsInput);
                info.pending = Some(w.pending.clone());
            }
            info.hooked = t.hooked;

            let changed = match t.history.last() {
                None => info.status.is_some(),
                Some(last) => last.status != info.status,
            };
            if changed {
                t.history.push(StatusChange { status: info.status, at: now });
                if !t.hooked {
                    let text = match info.status {
                        Some(AgentStatus::Running) => Some((AgentEventKind::Command, "Working (screen only)")),
                        Some(AgentStatus::NeedsInput) => Some((AgentEventKind::Asked, "Waiting on you (screen only)")),
                        Some(AgentStatus::Done) => Some((AgentEventKind::Idle, "Idle at prompt (screen only)")),
                        None => None,
                    };
                    if let Some((kind, text)) = text {
                        fresh.push(AgentEvent { at: now, session_id: info.session_id, kind, text: text.to_owned() });
                    }
                }
            }
            // Old changes go, but the one in force at the window's start stays.
            let cutoff = now.saturating_sub(HISTORY_MS);
            let old = t.history.iter().take_while(|c| c.at < cutoff).count();
            let drop = old.saturating_sub(1).max(t.history.len().saturating_sub(HISTORY_MAX));
            if drop > 0 {
                t.history.drain(..drop);
            }
            info.history = t.history.clone();
        }
        for event in fresh {
            self.record(event);
        }
    }

    /// Session `id` ended: forget it, and let go of any hook held for it.
    pub fn forget(&self, id: SessionId) {
        if let Some(t) = lock(&self.inner.state).sessions.remove(&id) {
            if let Some(w) = t.waiting {
                let _ = w.reply.send(None);
            }
        }
    }

    /// Answer the question `pending_id` Session `id` is waiting on with option `option`.
    /// Refused when that question is no longer the one waiting (answered from elsewhere, or the
    /// agent moved on).
    pub fn answer(&self, id: SessionId, pending_id: u64, option: usize) -> Result<(), String> {
        let (label, session) = {
            let mut state = lock(&self.inner.state);
            let t = state.sessions.get_mut(&id).ok_or("That agent is gone")?;
            match &t.waiting {
                Some(w) if w.pending.id == pending_id => {}
                _ => return Err("That question is no longer waiting".into()),
            }
            let w = t.waiting.take().expect("checked above");
            let (Some(reply), Some(label)) = (w.replies.get(option).cloned(), w.pending.options.get(option).cloned()) else {
                t.waiting = Some(w);
                return Err(format!("No option {}", option + 1));
            };
            if w.reply.send(Some(reply)).is_err() {
                return Err("The agent stopped waiting".into());
            }
            (label, id)
        };
        self.record(AgentEvent {
            at: now_ms(),
            session_id: session,
            kind: AgentEventKind::Answered,
            text: format!("You answered “{label}”"),
        });
        Ok(())
    }

    /// Stop holding the question `pending_id`: the agent asks it in its Terminal instead.
    pub fn release(&self, id: SessionId, pending_id: u64) {
        let mut state = lock(&self.inner.state);
        if let Some(t) = state.sessions.get_mut(&id) {
            if t.waiting.as_ref().is_some_and(|w| w.pending.id == pending_id) {
                if let Some(w) = t.waiting.take() {
                    let _ = w.reply.send(None);
                }
            }
        }
    }

    fn record(&self, event: AgentEvent) {
        {
            let mut state = lock(&self.inner.state);
            if state.feed.len() >= FEED_MAX {
                state.feed.pop_front();
            }
            state.feed.push_back(event.clone());
        }
        self.inner.events.emit(EVENT_AGENT_EVENT, &event);
        for w in lock(&self.inner.watchers).iter() {
            w(&event);
        }
    }

    /// One hook from Session `id`'s agent: `event` is its name, `payload` what it sent. Resolves
    /// to the reply to print, once there is one (a question waits for its answer); `None` lets
    /// Claude Code carry on as it would without the hook.
    pub(crate) async fn hook(&self, id: SessionId, event: &str, payload: Value) -> Option<Value> {
        let transcript = {
            let mut state = lock(&self.inner.state);
            let t = state.sessions.entry(id).or_default();
            t.hooked = event != "SessionEnd";
            // A new prompt, the end of the turn or of the agent: nothing it asked is still waiting.
            if matches!(event, "UserPromptSubmit" | "Stop" | "SessionEnd") {
                if let Some(w) = t.waiting.take() {
                    let _ = w.reply.send(None);
                }
            }
            let named = hooks::transcript(&payload).filter(|p| t.transcript.as_deref() != Some(*p));
            named.map(|p| t.transcript.insert(p.to_owned()).clone())
        };
        if let Some(path) = transcript {
            self.step(id, hooks::Step::Transcript(path), now_ms());
        }
        let ask = match event {
            "PermissionRequest" => hooks::permission(&payload),
            "PreToolUse" => hooks::question(&payload),
            _ => {
                let now = now_ms();
                if let Some((kind, text)) = hooks::event(event, &payload) {
                    self.record(AgentEvent { at: now, session_id: id, kind, text });
                }
                if let Some(step) = hooks::step(event, &payload) {
                    self.step(id, step, now);
                }
                return None;
            }
        };
        self.wait(id, ask?).await
    }

    /// Hold the hook until `ask` is answered or let go.
    async fn wait(&self, id: SessionId, ask: hooks::Ask) -> Option<Value> {
        let pending_id = self.inner.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let now = now_ms();
        let (tx, rx) = oneshot::channel();
        {
            let mut state = lock(&self.inner.state);
            let t = state.sessions.entry(id).or_default();
            if let Some(old) = t.waiting.take() {
                let _ = old.reply.send(None);
            }
            t.waiting = Some(Waiting {
                pending: Pending {
                    id: pending_id,
                    kind: ask.kind,
                    text: ask.text,
                    detail: ask.detail,
                    options: ask.options,
                    since: now,
                },
                replies: ask.replies,
                reply: tx,
            });
        }
        self.record(AgentEvent { at: now, session_id: id, kind: AgentEventKind::Asked, text: ask.event });
        // Should the hook's request go away first (Claude Code gave up on it), the question goes too.
        let _clear = ClearOnDrop { agents: self.clone(), id, pending_id };
        rx.await.ok().flatten()
    }
}

/// Takes a question off its Session when the hook waiting on it ends, however it ends.
struct ClearOnDrop {
    agents: Agents,
    id: SessionId,
    pending_id: u64,
}

impl Drop for ClearOnDrop {
    fn drop(&mut self) {
        self.agents.release(self.id, self.pending_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::testing::Recorder;
    use serde_json::json;

    fn agents() -> (Agents, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        (Agents::new(rec.clone()), rec)
    }

    fn claude(id: SessionId, status: AgentStatus) -> SessionInfo {
        SessionInfo { agent: Some(AgentKind::Claude), status: Some(status), ..SessionInfo::empty(id) }
    }

    #[test]
    fn history_records_each_change_and_trims_to_the_window() {
        let (a, _) = agents();
        let mut info = claude(1, AgentStatus::Running);
        a.decorate_at(&mut info, 1000);
        a.decorate_at(&mut info, 2000);
        assert_eq!(info.history, [StatusChange { status: Some(AgentStatus::Running), at: 1000 }]);
        let mut info = claude(1, AgentStatus::Done);
        a.decorate_at(&mut info, 3000);
        assert_eq!(info.history.len(), 2);
        // Far later: the change in force at the window's start stays, older ones go.
        let later = 3000 + HISTORY_MS + 10;
        let mut info = claude(1, AgentStatus::Running);
        a.decorate_at(&mut info, later);
        assert_eq!(
            info.history,
            [
                StatusChange { status: Some(AgentStatus::Done), at: 3000 },
                StatusChange { status: Some(AgentStatus::Running), at: later },
            ]
        );
    }

    #[test]
    fn a_plain_session_has_no_history_until_an_agent_starts() {
        let (a, rec) = agents();
        let mut info = SessionInfo::empty(2);
        a.decorate_at(&mut info, 10);
        assert!(info.history.is_empty());
        let mut info = SessionInfo { agent: Some(AgentKind::Gemini), status: Some(AgentStatus::NeedsInput), ..SessionInfo::empty(2) };
        a.decorate_at(&mut info, 20);
        assert_eq!(info.history.len(), 1);
        let events = rec.named(EVENT_AGENT_EVENT);
        assert_eq!(events.len(), 1, "a screen-only agent's status change is an event");
        assert_eq!(events[0]["text"], "Waiting on you (screen only)");
    }

    #[tokio::test]
    async fn a_question_waits_for_its_answer() {
        let (a, rec) = agents();
        assert_eq!(a.hook(3, "SessionStart", json!({})).await, None);
        let payload = json!({ "tool_name": "Bash", "tool_input": { "command": "ls" } });
        let waiting = tokio::spawn({
            let a = a.clone();
            async move { a.hook(3, "PermissionRequest", payload).await }
        });
        // The question shows on the Session's facts, which read Needs input.
        let pending = loop {
            let mut info = claude(3, AgentStatus::Running);
            a.decorate_at(&mut info, 5);
            if let Some(p) = info.pending {
                assert_eq!(info.status, Some(AgentStatus::NeedsInput));
                assert!(info.hooked);
                break p;
            }
            tokio::task::yield_now().await;
        };
        assert_eq!(pending.options[0], "Yes");
        assert!(a.answer(3, pending.id + 1, 0).is_err(), "another question's id");
        assert!(a.answer(3, pending.id, 9).is_err(), "no such option");
        a.answer(3, pending.id, 0).unwrap();
        let reply = waiting.await.unwrap().unwrap();
        assert_eq!(reply["hookSpecificOutput"]["decision"]["behavior"], "allow");
        assert!(a.answer(3, pending.id, 0).is_err(), "answered once only");
        let texts: Vec<String> = rec.named(EVENT_AGENT_EVENT).iter().map(|e| e["text"].as_str().unwrap().to_owned()).collect();
        assert_eq!(texts, ["Asked to run ls", "You answered “Yes”"]);
        let mut info = claude(3, AgentStatus::Running);
        a.decorate_at(&mut info, 6);
        assert_eq!(info.pending, None);
    }

    #[tokio::test]
    async fn steps_are_said_and_a_transcript_once_until_it_changes() {
        let (a, _) = agents();
        let steps = Arc::new(Mutex::new(Vec::new()));
        let seen = steps.clone();
        a.watch_steps(Box::new(move |id, step, _| {
            assert_eq!(id, 3);
            lock(&seen).push(step.clone());
        }));
        a.hook(3, "SessionStart", json!({ "transcript_path": "/c/1.jsonl" })).await;
        a.hook(3, "UserPromptSubmit", json!({ "transcript_path": "/c/1.jsonl", "prompt": "hi" })).await;
        let write = json!({ "tool_name": "Write", "tool_input": { "file_path": "/r/a", "content": "1\n2" } });
        a.hook(3, "PostToolUse", write).await;
        a.hook(3, "Stop", json!({})).await;
        a.hook(3, "SessionStart", json!({ "transcript_path": "/c/2.jsonl" })).await;
        assert_eq!(
            *lock(&steps),
            [
                hooks::Step::Transcript("/c/1.jsonl".into()),
                hooks::Step::Prompt,
                hooks::Step::Edit { added: 2, removed: 0 },
                hooks::Step::Transcript("/c/2.jsonl".into()),
            ]
        );
    }

    #[tokio::test]
    async fn a_released_question_lets_the_agent_ask_in_its_terminal() {
        let (a, _) = agents();
        let payload = json!({ "tool_name": "Bash", "tool_input": { "command": "ls" } });
        let waiting = tokio::spawn({
            let a = a.clone();
            async move { a.hook(4, "PermissionRequest", payload).await }
        });
        let id = loop {
            let mut info = claude(4, AgentStatus::Done);
            a.decorate_at(&mut info, 1);
            if let Some(p) = info.pending {
                break p.id;
            }
            tokio::task::yield_now().await;
        };
        a.release(4, id);
        assert_eq!(waiting.await.unwrap(), None);
    }

    #[tokio::test]
    async fn the_agent_leaving_lets_go_of_its_question() {
        let (a, _) = agents();
        let payload = json!({ "tool_name": "Bash", "tool_input": { "command": "ls" } });
        let waiting = tokio::spawn({
            let a = a.clone();
            async move { a.hook(5, "PermissionRequest", payload).await }
        });
        loop {
            let mut info = claude(5, AgentStatus::Done);
            a.decorate_at(&mut info, 1);
            if info.pending.is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
        let mut shell = SessionInfo::empty(5);
        a.decorate_at(&mut shell, 2);
        assert_eq!(waiting.await.unwrap(), None);
        assert!(!shell.hooked);
        assert_eq!(shell.history.last().unwrap().status, None, "the lane ends");
    }

    #[test]
    fn the_feed_keeps_the_last_events() {
        let (a, _) = agents();
        for i in 0..(FEED_MAX + 5) {
            a.record(AgentEvent { at: i as u64, session_id: 1, kind: AgentEventKind::Read, text: String::new() });
        }
        let recent = a.recent();
        assert_eq!(recent.len(), FEED_MAX);
        assert_eq!(recent[0].at, 5);
    }
}
