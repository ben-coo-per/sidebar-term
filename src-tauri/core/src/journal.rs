//! The Journal: what every Agent session on this Host was doing, where and for how long, kept
//! on disk for Rewind (docs/architecture.md "Journal", ADR 0004). The status history in
//! `agents/` goes back a few hours and lives in memory; this is the part that stays.
//!
//! **Spans, not samples.** A row is written when something ends, never on a timer: one span per
//! stretch of one Agent status in one [`Context`] (the Tab, the agent, the repo, Worktree and
//! branch). Time per repo, per branch or per hour of the day is a sum over spans.
//!
//! **Turns.** A hooked agent's hooks say what it does (`agents/hooks.rs`, [`Step`]); from the
//! user's prompt to the agent's stop is a turn, and a turn is one row of counts ([`Tally`]):
//! tools used by kind, lines added and removed, the programs its commands ran, questions asked
//! and how long they waited. No row per tool used, and no words: not the prompt, not a path.
//! A screen-only agent has its spans only.
//!
//! **Cost.** Claude Code keeps running totals of the tokens it used and what they cost, per
//! model, and writes them into the conversation's transcript now and then (a `cost-state`
//! line: when it exits, at some idle moments; not every turn). The hooks name the transcript;
//! every [`TICK`] the Journal reads what each one has gained, and when the totals
//! moved, writes what they moved by as a `cost` row ([`Cost`]), from when they were last known
//! to when the file was written. The totals it has counted are kept in `journal/meters.json`,
//! so what Claude Code writes as the Host kills it is counted by the next launch. The
//! messages in a transcript carry token counts too; they are not used, because they add up to
//! well under the totals.
//!
//! **Files.** `journal/<year>-<month>.jsonl` in the Host's data dir, one JSON object per line,
//! appended. Months are UTC and only say which file a span is in: a span that crosses the end
//! of a month is written as two, so every span lies within its file's month. A turn is written
//! whole, in the month it began in, and a cost in the month it was read in. Four kinds of row:
//!
//! ```text
//! {"k":"ctx","id":1,"tab":"t3","agent":"claude","repo":"/r/.git","name":"r","br":"main"}
//! {"k":"span","c":1,"s":"running","a":1790000000000,"b":1790000042000}
//! {"k":"turn","c":1,"a":1790000000000,"b":1790000042000,"len":38,"tools":{"edit":2,"run":1},"add":9,"del":2,"files":1,"cmds":{"cargo":1}}
//! {"k":"cost","c":1,"a":1790000000000,"b":1790000600000,"m":{"claude-opus-5-5":{"in":120,"out":9100,"cr":810000,"cw":5200,"usd":1420000}}}
//! ```
//!
//! A `ctx` row gives a Context a number, and the rows after it name it by that number, so a
//! repo's path is written once and not on every span. The number holds for the rows that
//! follow it, until a later `ctx` row gives the same number to something else (each run of the
//! Host starts again at 1), so a file is read from the top. A line that does not parse is
//! skipped: a crash can cut the last one short.
//!
//! **A crash.** A span is written when it ends, so the ones still open would go with the
//! process, and the turns with them. Every [`TICK`] they are written to `journal/open.json`
//! with the time; the next launch ends them at that time and writes them as rows.

use crate::agents::hooks::{Step, ToolKind, ToolUse};
use crate::agents::now_ms;
use crate::model::{AgentKind, AgentStatus, SessionId, SessionInfo};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

/// Time between two writes of the open spans: what a crash can lose of each.
const TICK: Duration = Duration::from_secs(30);
/// The spans still open, for the next launch to end (see the module's words on a crash).
const OPEN: &str = "open.json";
/// The transcripts looked at for Claude Code's totals, and the totals counted so far.
const METERS: &str = "meters.json";
/// How much of a transcript's end is read for the totals when it is first looked at. After
/// that, what it gained since is read.
const TAIL: u64 = 1024 * 1024;
/// The most of a transcript read at once; of more than that, the end.
const READ_MAX: u64 = 8 * 1024 * 1024;
/// A transcript whose totals have not moved for this long, in no live Session, is let go.
const METER_IDLE_MS: u64 = 30 * DAY_MS;
const DAY_MS: u64 = 86_400_000;
/// Programs a turn counts its commands under at most; commands of any other are not counted
/// by program.
const PROGRAMS_MAX: usize = 12;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero<T: Default + PartialEq>(n: &T) -> bool {
    *n == T::default()
}

/// Where an agent ran and which one: what a span's time is counted under.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Context {
    /// The Tab's id in this Host's layout.
    pub tab: String,
    pub agent: AgentKind,
    /// The repo's identity, `GitInfo.common_dir`: the same for every Worktree of one repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// The repo's display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The Worktree's name; `None` in the main Worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wt: Option<String>,
    /// The branch; `None` when HEAD is detached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub br: Option<String>,
    /// The cwd, for a Session that is not in a repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// The agent runs over a remote hop (ssh, docker exec): where it is, this Host cannot say.
    #[serde(default, skip_serializing_if = "is_false")]
    pub remote: bool,
}

impl Context {
    /// The Context of `info`'s agent in Tab `tab`; `None` when it has no agent.
    fn of(info: &SessionInfo, tab: String) -> Option<Self> {
        let agent = info.agent?;
        // Over a remote hop, cwd and git describe this Host, not the far side.
        let git = info.git.as_ref().filter(|_| !info.remote);
        Some(Self {
            tab,
            agent,
            repo: git.map(|g| g.common_dir.clone()),
            name: git.map(|g| g.repo_name.clone()),
            wt: git.and_then(|g| g.worktree_name.clone()),
            br: git.and_then(|g| g.branch.clone()),
            cwd: match git {
                Some(_) => None,
                None => info.cwd.clone().filter(|_| !info.remote),
            },
            remote: info.remote,
        })
    }
}

/// One stretch of one Agent status in one Context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub context: Context,
    pub status: AgentStatus,
    /// Epoch ms.
    pub start: u64,
    /// Epoch ms, after `start`.
    pub end: u64,
}

/// What a hooked agent did in one turn, counted. What is zero is not written.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    /// Characters of the prompt.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub len: u32,
    /// Tools used, by kind, the failed uses included.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<ToolKind, u32>,
    /// Uses of a tool that failed or were interrupted.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fail: u32,
    /// Lines its edits added.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub add: u32,
    /// Lines its edits removed.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub del: u32,
    /// Files it changed.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub files: u32,
    /// Commands run, by program (`cargo`, `git`): [`PROGRAMS_MAX`] programs at most.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cmds: BTreeMap<String, u32>,
    /// Questions it asked the user, permissions included.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub asked: u32,
    /// How long its questions waited, ms in all: from each question to the agent's next step.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub waited: u64,
}

/// One turn of a hooked agent: from the user's prompt to the agent's stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub context: Context,
    /// Epoch ms.
    pub start: u64,
    /// Epoch ms. A turn cut short with no stop (Esc) ends at the next prompt.
    pub end: u64,
    pub tally: Tally,
}

/// Tokens used on one model, and what they cost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    /// Input tokens that were not read from the cache.
    #[serde(rename = "in", default, skip_serializing_if = "is_zero")]
    pub input: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub out: u64,
    /// Thinking tokens.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub th: u64,
    /// Input tokens read from the cache.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cr: u64,
    /// Input tokens written to the cache.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cw: u64,
    /// Millionths of a dollar, as Claude Code worked it out.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub usd: u64,
}

impl Tokens {
    fn fields(&self) -> [u64; 6] {
        [self.input, self.out, self.th, self.cr, self.cw, self.usd]
    }

    /// What `self` has over `base`, nothing where it has less.
    fn over(&self, base: &Tokens) -> Tokens {
        Tokens {
            input: self.input.saturating_sub(base.input),
            out: self.out.saturating_sub(base.out),
            th: self.th.saturating_sub(base.th),
            cr: self.cr.saturating_sub(base.cr),
            cw: self.cw.saturating_sub(base.cw),
            usd: self.usd.saturating_sub(base.usd),
        }
    }
}

/// Tokens by model, under the model's id (`claude-opus-5-5`).
pub type Models = BTreeMap<String, Tokens>;

/// What an agent used between two readings of its totals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cost {
    pub context: Context,
    /// When the totals were last known before, epoch ms.
    pub start: u64,
    /// When the totals were written, epoch ms.
    pub end: u64,
    pub models: Models,
}

/// Claude Code's totals as a transcript holds them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Totals {
    /// Its `startTime`: the run of Claude Code that counted them.
    start: u64,
    models: Models,
}

/// A transcript the Journal reads the totals from, in `meters.json` under its path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Watched {
    /// The Context its cost is counted under: its Session's, when last known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ctx: Option<Context>,
    /// The file's length when it was last read.
    #[serde(default)]
    len: u64,
    /// When its totals last moved, or when it was first looked at, epoch ms.
    at: u64,
    /// The totals last read from it, and the run that counted them.
    #[serde(default)]
    start: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    totals: Models,
}

/// `meters.json`.
#[derive(Default, Serialize, Deserialize)]
struct MetersFile {
    files: BTreeMap<String, Watched>,
}

/// A line of a month's file.
#[derive(Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "lowercase")]
enum Row {
    Ctx {
        id: u32,
        #[serde(flatten)]
        context: Context,
    },
    Span {
        c: u32,
        s: AgentStatus,
        a: u64,
        b: u64,
    },
    Turn {
        c: u32,
        a: u64,
        b: u64,
        #[serde(flatten)]
        tally: Tally,
    },
    Cost {
        c: u32,
        a: u64,
        b: u64,
        m: Models,
    },
}

/// `open.json`.
#[derive(Serialize, Deserialize)]
struct OpenFile {
    /// When this was written: where the spans end, should the Host not live to end them.
    alive_at: u64,
    spans: Vec<OpenSpan>,
    #[serde(default)]
    turns: Vec<OpenTurnRow>,
}

#[derive(Serialize, Deserialize)]
struct OpenTurnRow {
    #[serde(flatten)]
    context: Context,
    a: u64,
    #[serde(flatten)]
    tally: Tally,
}

#[derive(Serialize, Deserialize)]
struct OpenSpan {
    #[serde(flatten)]
    context: Context,
    s: AgentStatus,
    a: u64,
}

/// A span that has not ended.
struct Open {
    context: Context,
    status: AgentStatus,
    since: u64,
}

/// A turn that has not ended.
struct OpenTurn {
    /// Where it began, for a turn that ends with its Session's Context no longer known.
    context: Option<Context>,
    start: u64,
    tally: Tally,
    /// The files it changed so far.
    edited: HashSet<String>,
    /// When the question it waits on was asked.
    asked_at: Option<u64>,
    /// When its agent was last seen to leave, with no step since: where the turn ends if no
    /// stop comes (the agent was killed).
    left: Option<u64>,
}

impl OpenTurn {
    /// Whatever the agent does next, it waits on its question no longer.
    fn settle(&mut self, now: u64) {
        if let Some(asked) = self.asked_at.take() {
            self.tally.waited += now.saturating_sub(asked);
        }
    }

    fn count(&mut self, used: &ToolUse) {
        let t = &mut self.tally;
        *t.tools.entry(used.kind).or_default() += 1;
        t.fail += u32::from(used.failed);
        t.add += used.added;
        t.del += used.removed;
        if let Some(file) = &used.file {
            self.edited.insert(file.clone());
            t.files = self.edited.len() as u32;
        }
        if let Some(program) = &used.program {
            if t.cmds.len() < PROGRAMS_MAX || t.cmds.contains_key(program) {
                *t.cmds.entry(program.clone()).or_default() += 1;
            }
        }
    }
}

#[derive(Clone)]
pub struct Journal {
    inner: Arc<Inner>,
}

struct Inner {
    /// `<data dir>/journal`; `None` when the Host has no data dir (then nothing is kept).
    dir: Option<PathBuf>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    open: HashMap<SessionId, Open>,
    turns: HashMap<SessionId, OpenTurn>,
    /// The Context each live Session's agent was last seen in.
    contexts: HashMap<SessionId, Context>,
    /// The transcript of each live Session's hooked agent.
    transcripts: HashMap<SessionId, String>,
    /// Every transcript looked at, by path.
    watched: BTreeMap<String, Watched>,
    /// `watched` has changed since `meters.json` was written.
    unsaved: bool,
    /// The month whose file `ids` was written to.
    month: Option<Month>,
    /// The number of each Context the file was given in this run.
    ids: HashMap<Context, u32>,
    /// `open.json` holds spans (so it has to go once none is open).
    checkpointed: bool,
    /// The Host is stopping: its dying Sessions are not written down.
    finished: bool,
    /// A write failed and was said so; said again only after one that worked.
    failing: bool,
}

impl Journal {
    /// The Journal in `dir`, made on the first write. Spans the last run left open (it crashed,
    /// or was killed) are ended at the last time it was known alive.
    pub fn open(dir: Option<PathBuf>) -> Self {
        let journal = Self { inner: Arc::new(Inner { dir, state: Mutex::default() }) };
        if let Some(dir) = &journal.inner.dir {
            let path = dir.join(OPEN);
            if let Some(left) = fs::read(&path).ok().and_then(|b| serde_json::from_slice::<OpenFile>(&b).ok()) {
                let mut state = lock(&journal.inner.state);
                for span in left.spans {
                    let open = Open { context: span.context, status: span.s, since: span.a };
                    state.write(dir, &open, left.alive_at);
                }
                for turn in left.turns {
                    state.write_turn(dir, &turn.context, turn.a, left.alive_at.max(turn.a), turn.tally);
                }
            }
            let _ = fs::remove_file(&path);
            let meters = fs::read(dir.join(METERS)).ok().and_then(|b| serde_json::from_slice::<MetersFile>(&b).ok());
            lock(&journal.inner.state).watched = meters.unwrap_or_default().files;
        }
        journal
    }

    /// The monitor's word on Sessions that changed. A Session whose Agent status or Context is
    /// no longer its open span's ends that span and starts the next; `tab_of` gives a Session's
    /// Tab id, and a Session without a Tab is not kept.
    pub fn observe(&self, infos: &[SessionInfo], tab_of: impl Fn(SessionId) -> Option<String>) {
        self.observe_at(infos, tab_of, now_ms());
    }

    fn observe_at(&self, infos: &[SessionInfo], tab_of: impl Fn(SessionId) -> Option<String>, now: u64) {
        // Before the lock: `tab_of` takes the layout's.
        let seen: Vec<(SessionId, Option<(Context, AgentStatus)>)> = infos
            .iter()
            .map(|info| {
                let now_in = info
                    .status
                    .and_then(|status| Some((Context::of(info, tab_of(info.session_id)?)?, status)));
                (info.session_id, now_in)
            })
            .collect();
        let mut state = lock(&self.inner.state);
        if state.finished {
            return;
        }
        for (id, now_in) in seen {
            match &now_in {
                Some((context, _)) => {
                    state.contexts.insert(id, context.clone());
                }
                None => {
                    if let Some(turn) = state.turns.get_mut(&id) {
                        turn.left.get_or_insert(now);
                    }
                }
            }
            let same = match (state.open.get(&id), &now_in) {
                (Some(open), Some((context, status))) => open.context == *context && open.status == *status,
                (None, None) => true,
                _ => false,
            };
            if same {
                continue;
            }
            self.close(&mut state, id, now);
            if let Some((context, status)) = now_in {
                state.open.insert(id, Open { context, status, since: now });
            }
        }
    }

    /// A step of the turn of Session `id`'s hooked agent, at `at` (`Agents::watch_steps`). A
    /// prompt begins a turn (and ends one cut short before it), a stop ends it, and what comes
    /// between is counted. Steps outside a turn are not kept: a tool that finishes after the
    /// stop, the rest of a turn this Host started in the middle of.
    pub fn step(&self, id: SessionId, step: &Step, at: u64) {
        let mut state = lock(&self.inner.state);
        if state.finished {
            return;
        }
        match step {
            Step::Prompt { len } => {
                self.close_turn(&mut state, id, at);
                let turn = OpenTurn {
                    context: state.contexts.get(&id).cloned(),
                    start: at,
                    tally: Tally { len: *len, ..Tally::default() },
                    edited: HashSet::new(),
                    asked_at: None,
                    left: None,
                };
                state.turns.insert(id, turn);
            }
            Step::Stop => {
                if let Some(turn) = state.turns.get_mut(&id) {
                    turn.left = None;
                }
                self.close_turn(&mut state, id, at);
            }
            Step::Transcript(path) => {
                state.transcripts.insert(id, path.clone());
                if !state.watched.contains_key(path) {
                    // What it holds already is not this Journal's to count: it was used before
                    // the file was looked at.
                    let len = fs::metadata(path).map_or(0, |m| m.len());
                    let seen = totals_in(Path::new(path), len.saturating_sub(TAIL)).unwrap_or_default();
                    let ctx = state.contexts.get(&id).cloned();
                    let watched = Watched { ctx, len, at, start: seen.start, totals: seen.models };
                    state.watched.insert(path.clone(), watched);
                    state.unsaved = true;
                }
            }
            Step::Tool(_) | Step::Asked | Step::Answered => {
                let Some(turn) = state.turns.get_mut(&id) else { return };
                turn.left = None;
                turn.settle(at);
                match step {
                    Step::Tool(used) => turn.count(used),
                    Step::Asked => {
                        turn.tally.asked += 1;
                        turn.asked_at = Some(at);
                    }
                    _ => {}
                }
            }
        }
    }

    /// Session `id` ended, and its span and its turn with it.
    pub fn end(&self, id: SessionId) {
        self.end_at(id, now_ms());
    }

    fn end_at(&self, id: SessionId, now: u64) {
        let mut state = lock(&self.inner.state);
        if !state.finished {
            self.close(&mut state, id, now);
            self.close_turn(&mut state, id, now);
        }
        state.contexts.remove(&id);
        // Its transcript stays watched: Claude Code writes its totals as it goes.
        state.transcripts.remove(&id);
    }

    /// The Host is stopping: every open span and turn ends now, and nothing is written after.
    pub fn finish(&self) {
        self.finish_at(now_ms());
    }

    fn finish_at(&self, now: u64) {
        self.measure(now);
        let mut state = lock(&self.inner.state);
        let ids: Vec<SessionId> = state.open.keys().copied().collect();
        for id in ids {
            self.close(&mut state, id, now);
        }
        let ids: Vec<SessionId> = state.turns.keys().copied().collect();
        for id in ids {
            self.close_turn(&mut state, id, now);
        }
        state.finished = true;
        if let Some(dir) = &self.inner.dir {
            let _ = fs::remove_file(dir.join(OPEN));
        }
    }

    /// Write the open spans and turns to `open.json` with the time, for a launch after a crash.
    fn checkpoint(&self, now: u64) {
        let Some(dir) = &self.inner.dir else { return };
        let mut state = lock(&self.inner.state);
        if state.finished {
            return;
        }
        let path = dir.join(OPEN);
        let turns: Vec<OpenTurnRow> = state
            .turns
            .iter()
            .filter_map(|(id, turn)| {
                let context = state.contexts.get(id).or(turn.context.as_ref())?.clone();
                let mut tally = turn.tally.clone();
                if let Some(asked) = turn.asked_at {
                    tally.waited += now.saturating_sub(asked);
                }
                Some(OpenTurnRow { context, a: turn.start, tally })
            })
            .collect();
        if state.open.is_empty() && turns.is_empty() {
            if state.checkpointed {
                let _ = fs::remove_file(&path);
                state.checkpointed = false;
            }
            return;
        }
        let file = OpenFile {
            alive_at: now,
            spans: state
                .open
                .values()
                .map(|o| OpenSpan { context: o.context.clone(), s: o.status, a: o.since })
                .collect(),
            turns,
        };
        let written = fs::create_dir_all(dir)
            .map_err(|e| e.to_string())
            .and_then(|_| crate::store::write(&path, &file));
        state.said(written.map_err(|e| format!("{OPEN}: {e}")));
        state.checkpointed = true;
    }

    /// Every span that overlaps `from..to` (epoch ms), cut to it, the ones still open included
    /// (they end now). In the order they were written, the open ones last.
    pub fn read(&self, from: u64, to: u64) -> Vec<Span> {
        self.read_at(from, to, now_ms())
    }

    fn read_at(&self, from: u64, to: u64, now: u64) -> Vec<Span> {
        let mut spans = Vec::new();
        if from >= to {
            return spans;
        }
        let state = lock(&self.inner.state);
        if let Some(dir) = &self.inner.dir {
            let last = Month::of(to - 1);
            let mut month = Month::of(from);
            while month <= last {
                spans.extend(read_file(&dir.join(month.file())).0);
                month = month.next();
            }
        }
        spans.extend(state.open.values().map(|o| Span {
            context: o.context.clone(),
            status: o.status,
            start: o.since,
            end: now,
        }));
        drop(state);
        spans.retain_mut(|s| {
            s.start = s.start.max(from);
            s.end = s.end.min(to);
            s.start < s.end
        });
        spans
    }

    /// Look at every watched transcript that changed, and write what Claude Code's totals
    /// moved by. The files are read outside the lock.
    fn measure(&self, now: u64) {
        let Some(dir) = &self.inner.dir else { return };
        let looks: Vec<(String, u64)> = {
            let mut state = lock(&self.inner.state);
            if state.finished {
                return;
            }
            let state = &mut *state;
            for (id, path) in &state.transcripts {
                if let (Some(context), Some(w)) = (state.contexts.get(id), state.watched.get_mut(path)) {
                    if w.ctx.as_ref() != Some(context) {
                        w.ctx = Some(context.clone());
                        state.unsaved = true;
                    }
                }
            }
            state.watched.iter().map(|(path, w)| (path.clone(), w.len)).collect()
        };
        let read: Vec<(String, u64, u64, Option<Totals>)> = looks
            .into_iter()
            .filter_map(|(path, len)| {
                let meta = fs::metadata(&path).ok()?;
                if meta.len() == len {
                    return None;
                }
                let written = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
                let written = written.map_or(now, |d| (d.as_millis() as u64).min(now));
                // A file that is shorter than it was is another file: all of it is new.
                let from = if meta.len() > len { len } else { 0 };
                let totals = totals_in(Path::new(&path), from);
                Some((path, meta.len(), written, totals))
            })
            .collect();
        let mut state = lock(&self.inner.state);
        if state.finished {
            return;
        }
        for (path, len, written, totals) in read {
            state.count(dir, &path, len, written, totals);
        }
        let live: HashSet<&String> = state.transcripts.values().collect();
        let idle: Vec<String> = state
            .watched
            .iter()
            .filter(|(path, w)| now.saturating_sub(w.at) > METER_IDLE_MS && !live.contains(path))
            .map(|(path, _)| path.clone())
            .collect();
        for path in idle {
            state.watched.remove(&path);
            state.unsaved = true;
        }
        if state.unsaved {
            let file = MetersFile { files: state.watched.clone() };
            let written = fs::create_dir_all(dir)
                .map_err(|e| e.to_string())
                .and_then(|_| crate::store::write(&dir.join(METERS), &file));
            state.unsaved = written.is_err();
            state.said(written.map_err(|e| format!("{METERS}: {e}")));
        }
    }

    /// Every cost read in `from..to` (epoch ms), whole, in the order they were written.
    pub fn costs(&self, from: u64, to: u64) -> Vec<Cost> {
        let mut costs = Vec::new();
        if from >= to {
            return costs;
        }
        let _state = lock(&self.inner.state);
        if let Some(dir) = &self.inner.dir {
            let last = Month::of(to - 1);
            let mut month = Month::of(from);
            while month <= last {
                costs.extend(read_file(&dir.join(month.file())).2);
                month = month.next();
            }
        }
        costs.retain(|c| (from..to).contains(&c.end));
        costs
    }

    /// Every turn that began in `from..to` (epoch ms), whole, the ones still going included
    /// (they end now). In the order they were written, the ones still going last.
    pub fn turns(&self, from: u64, to: u64) -> Vec<Turn> {
        self.turns_at(from, to, now_ms())
    }

    fn turns_at(&self, from: u64, to: u64, now: u64) -> Vec<Turn> {
        let mut turns = Vec::new();
        if from >= to {
            return turns;
        }
        let state = lock(&self.inner.state);
        if let Some(dir) = &self.inner.dir {
            let last = Month::of(to - 1);
            let mut month = Month::of(from);
            while month <= last {
                turns.extend(read_file(&dir.join(month.file())).1);
                month = month.next();
            }
        }
        turns.extend(state.turns.iter().filter_map(|(id, turn)| {
            let context = state.contexts.get(id).or(turn.context.as_ref())?.clone();
            let mut turn = Turn { context, start: turn.start, end: now.max(turn.start), tally: turn.tally.clone() };
            turn.tally.waited += state.turns[id].asked_at.map_or(0, |asked| now.saturating_sub(asked));
            Some(turn)
        }));
        drop(state);
        turns.retain(|t| (from..to).contains(&t.start));
        turns
    }

    /// End Session `id`'s turn, if it is in one: at `now`, or when its agent left if it has
    /// (and said nothing since). Written under the Context the Session is in now, the one it
    /// began in failing that; a turn with neither is not kept.
    fn close_turn(&self, state: &mut State, id: SessionId, now: u64) {
        let Some(mut turn) = state.turns.remove(&id) else { return };
        let end = turn.left.unwrap_or(now).max(turn.start);
        turn.settle(end);
        let context = state.contexts.get(&id).cloned().or(turn.context);
        if let (Some(context), Some(dir)) = (context, &self.inner.dir) {
            state.write_turn(dir, &context, turn.start, end, turn.tally);
        }
    }

    fn close(&self, state: &mut State, id: SessionId, now: u64) {
        let Some(open) = state.open.remove(&id) else { return };
        if let Some(dir) = &self.inner.dir {
            state.write(dir, &open, now);
        }
    }
}

impl State {
    /// Write `open`, ended at `end`, as one span per month it was open in.
    fn write(&mut self, dir: &Path, open: &Open, end: u64) {
        let mut start = open.since;
        while start < end {
            let month = Month::of(start);
            let stop = end.min(month.next().start());
            let row = |c| Row::Span { c, s: open.status, a: start, b: stop };
            let written = self.append(dir, month, &open.context, row);
            self.said(written);
            start = stop;
        }
    }

    /// The totals `path` now holds are `totals`, written at `at`: write what they have over
    /// what was counted, under the transcript's Context, and count them.
    fn count(&mut self, dir: &Path, path: &str, len: u64, at: u64, totals: Option<Totals>) {
        let base = totals.as_ref().map(|t| self.counted(path, t));
        let Some(w) = self.watched.get_mut(path) else { return };
        w.len = len;
        let (Some(totals), Some(base)) = (totals, base) else { return };
        if totals.start == w.start && totals.models == w.totals {
            return;
        }
        let over: Models = totals
            .models
            .iter()
            .map(|(model, t)| (model.clone(), t.over(base.get(model).unwrap_or(&Tokens::default()))))
            .filter(|(_, t)| *t != Tokens::default())
            .collect();
        let (from, context) = (w.at, w.ctx.clone());
        w.start = totals.start;
        w.totals = totals.models;
        self.unsaved = true;
        if over.is_empty() {
            return;
        }
        let at = at.max(from);
        w.at = at;
        if let Some(context) = context {
            let row = |c| Row::Cost { c, a: from, b: at, m: over };
            let written = self.append(dir, Month::of(at), &context, row);
            self.said(written);
        }
    }

    /// What of `totals` was counted already: the most this Journal read from the same run of
    /// Claude Code, in any transcript (`/clear` starts another, and the totals carry on);
    /// failing that what this transcript last held, if the totals are those and more (a
    /// resume, carrying them on under another run); else nothing.
    fn counted(&self, path: &str, totals: &Totals) -> Models {
        let sum = |m: &Models| m.values().flat_map(|t| t.fields()).sum::<u64>();
        let same_run = self
            .watched
            .values()
            .filter(|w| totals.start != 0 && w.start == totals.start)
            .max_by_key(|w| sum(&w.totals));
        if let Some(w) = same_run {
            return w.totals.clone();
        }
        let has_more = |held: &Models| {
            held.iter().all(|(model, t)| {
                let now = totals.models.get(model).copied().unwrap_or_default();
                now.fields().iter().zip(t.fields()).all(|(now, held)| *now >= held)
            })
        };
        match self.watched.get(path) {
            Some(w) if has_more(&w.totals) => w.totals.clone(),
            _ => Models::new(),
        }
    }

    /// Write a turn, whole, in the month it began in.
    fn write_turn(&mut self, dir: &Path, context: &Context, start: u64, end: u64, tally: Tally) {
        let row = |c| Row::Turn { c, a: start, b: end, tally };
        let written = self.append(dir, Month::of(start), context, row);
        self.said(written);
    }

    /// Append the row `row` makes of `context`'s number to `month`'s file, after the `ctx` row
    /// that gives it the number if the file has none from this run.
    fn append(&mut self, dir: &Path, month: Month, context: &Context, row: impl FnOnce(u32) -> Row) -> Result<(), String> {
        let written = self.append_lines(dir, month, context, row);
        if written.is_err() {
            // What the file holds is unknown: give every Context its number again.
            self.month = None;
        }
        written
    }

    fn append_lines(&mut self, dir: &Path, month: Month, context: &Context, row: impl FnOnce(u32) -> Row) -> Result<(), String> {
        let path = dir.join(month.file());
        let mut lines = String::new();
        if self.month != Some(month) {
            self.month = Some(month);
            self.ids.clear();
            if ends_mid_line(&path) {
                lines.push('\n');
            }
        }
        let known = self.ids.get(context).copied();
        let id = known.unwrap_or(self.ids.len() as u32 + 1);
        let mut rows = Vec::new();
        if known.is_none() {
            rows.push(Row::Ctx { id, context: context.clone() });
        }
        rows.push(row(id));
        for row in &rows {
            lines.push_str(&serde_json::to_string(row).map_err(|e| e.to_string())?);
            lines.push('\n');
        }
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        // One write for the Context and its row: O_APPEND keeps them together.
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(lines.as_bytes()))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        self.ids.insert(context.clone(), id);
        Ok(())
    }

    /// Say a failed write once, not on every span after it.
    fn said(&mut self, written: Result<(), String>) {
        match written {
            Ok(()) => self.failing = false,
            Err(e) if !self.failing => {
                self.failing = true;
                eprintln!("journal: not written ({e}); saying so once");
            }
            Err(_) => {}
        }
    }
}

/// Whether the file at `path` ends without a newline (a crash cut its last line short).
fn ends_mid_line(path: &Path) -> bool {
    let last = || -> std::io::Result<u8> {
        let mut f = fs::File::open(path)?;
        f.seek(SeekFrom::End(-1))?;
        let mut byte = [0u8];
        f.read_exact(&mut byte)?;
        Ok(byte[0])
    };
    last().is_ok_and(|b| b != b'\n')
}

/// Claude Code's totals in the transcript at `path`: its last `cost-state` line, looked for
/// from byte `from` on (in the last [`READ_MAX`] bytes, if that is more). `None` when there is
/// none there. A line cut by where the reading starts is not one.
fn totals_in(path: &Path, from: u64) -> Option<Totals> {
    let mut f = fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(from.max(len.saturating_sub(READ_MAX)))).ok()?;
    let mut tail = Vec::new();
    f.read_to_end(&mut tail).ok()?;
    let text = String::from_utf8_lossy(&tail);
    text.lines().rev().filter(|l| l.contains("cost-state")).find_map(totals_of)
}

/// The totals a `cost-state` line holds; `None` for any other line.
fn totals_of(line: &str) -> Option<Totals> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "cost-state" {
        return None;
    }
    let models = v.get("modelUsage")?.as_object()?;
    let n = |u: &serde_json::Value, key: &str| u.get(key).and_then(serde_json::Value::as_u64).unwrap_or(0);
    let models = models
        .iter()
        .map(|(model, u)| {
            let usd = u.get("costUSD").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
            let tokens = Tokens {
                input: n(u, "inputTokens"),
                out: n(u, "outputTokens"),
                th: n(u, "thinkingTokens"),
                cr: n(u, "cacheReadInputTokens"),
                cw: n(u, "cacheCreationInputTokens"),
                usd: (usd.max(0.0) * 1e6).round() as u64,
            };
            (model.clone(), tokens)
        })
        .collect();
    Some(Totals { start: n(&v, "startTime"), models })
}

/// The spans, the turns and the costs in one month's file, in order; none for a file that is
/// not there.
fn read_file(path: &Path) -> (Vec<Span>, Vec<Turn>, Vec<Cost>) {
    let (mut spans, mut turns, mut costs) = (Vec::new(), Vec::new(), Vec::new());
    let Ok(text) = fs::read_to_string(path) else { return (spans, turns, costs) };
    let mut contexts: HashMap<u32, Context> = HashMap::new();
    for line in text.lines() {
        match serde_json::from_str::<Row>(line) {
            Ok(Row::Ctx { id, context }) => {
                contexts.insert(id, context);
            }
            Ok(Row::Span { c, s, a, b }) => {
                if let Some(context) = contexts.get(&c) {
                    spans.push(Span { context: context.clone(), status: s, start: a, end: b });
                }
            }
            Ok(Row::Turn { c, a, b, tally }) => {
                if let Some(context) = contexts.get(&c) {
                    turns.push(Turn { context: context.clone(), start: a, end: b, tally });
                }
            }
            Ok(Row::Cost { c, a, b, m }) => {
                if let Some(context) = contexts.get(&c) {
                    costs.push(Cost { context: context.clone(), start: a, end: b, models: m });
                }
            }
            Err(_) => {}
        }
    }
    (spans, turns, costs)
}

/// Start the thread that, every [`TICK`], writes the open spans and turns down and looks at the
/// transcripts. A panicking tick is skipped; the thread never exits.
pub fn spawn(journal: Journal) {
    let started = thread::Builder::new().name("journal".into()).spawn(move || loop {
        thread::sleep(TICK);
        let tick = || {
            journal.checkpoint(now_ms());
            journal.measure(now_ms());
        };
        if catch_unwind(AssertUnwindSafe(tick)).is_err() {
            eprintln!("journal: a tick panicked; skipping it");
        }
    });
    if let Err(e) = started {
        eprintln!("journal: could not start thread: {e}");
    }
}

/// A month of the UTC calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Month {
    year: i64,
    /// 1 to 12.
    month: u32,
}

impl Month {
    /// The month epoch ms `ms` is in. (Days to a date: Howard Hinnant's `civil_from_days`.)
    fn of(ms: u64) -> Self {
        let z = (ms / DAY_MS) as i64 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = yoe + era * 400 + i64::from(month <= 2);
        Self { year, month }
    }

    fn next(self) -> Self {
        match self.month {
            12 => Self { year: self.year + 1, month: 1 },
            m => Self { year: self.year, month: m + 1 },
        }
    }

    /// Epoch ms of its first instant. (A date to days: Hinnant's `days_from_civil`.)
    fn start(self) -> u64 {
        let y = self.year - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = i64::from(if self.month > 2 { self.month - 3 } else { self.month + 9 });
        let doy = (153 * mp + 2) / 5;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        days.max(0) as u64 * DAY_MS
    }

    fn file(self) -> String {
        format!("{:04}-{:02}.jsonl", self.year, self.month)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::testutil::TempDir;
    use crate::model::GitInfo;

    /// 2026-09-28 00:00 UTC.
    const DAY: u64 = 1_790_553_600_000;
    const SEPTEMBER: &str = "2026-09.jsonl";

    fn journal(tmp: &TempDir) -> Journal {
        Journal::open(Some(tmp.path().join("journal")))
    }

    fn agent(id: SessionId, status: AgentStatus, branch: &str) -> SessionInfo {
        SessionInfo {
            agent: Some(AgentKind::Claude),
            status: Some(status),
            cwd: Some("/r/src".into()),
            git: Some(GitInfo {
                repo_name: "r".into(),
                common_dir: "/r/.git".into(),
                worktree_root: "/r".into(),
                worktree_name: None,
                branch: Some(branch.into()),
                head_short: None,
            }),
            ..SessionInfo::empty(id)
        }
    }

    fn tab(id: SessionId) -> Option<String> {
        Some(format!("t{id}"))
    }

    fn lines(tmp: &TempDir, file: &str) -> Vec<String> {
        let text = fs::read_to_string(tmp.path().join("journal").join(file)).unwrap_or_default();
        text.lines().map(str::to_owned).collect()
    }

    fn shape(spans: &[Span]) -> Vec<(&str, AgentStatus, u64, u64)> {
        spans
            .iter()
            .map(|s| (s.context.br.as_deref().unwrap_or("-"), s.status, s.start - DAY, s.end - DAY))
            .collect()
    }

    #[test]
    fn the_months_are_the_calendars() {
        assert_eq!(Month::of(0), Month { year: 1970, month: 1 });
        assert_eq!(Month::of(DAY), Month { year: 2026, month: 9 });
        assert_eq!(Month::of(DAY).file(), SEPTEMBER);
        let october = Month::of(DAY).next();
        assert_eq!(october.start(), DAY + 3 * DAY_MS, "28, 29 and 30 September");
        assert_eq!(Month::of(october.start() - 1), Month { year: 2026, month: 9 });
        assert_eq!(Month::of(october.start()), october);
        assert_eq!(Month { year: 2026, month: 12 }.next(), Month { year: 2027, month: 1 });
        // A leap day is in February.
        let march = Month { year: 2028, month: 3 };
        assert_eq!(Month::of(march.start() - 1), Month { year: 2028, month: 2 });
        assert_eq!(march.start() - Month { year: 2028, month: 2 }.start(), 29 * DAY_MS);
        assert_eq!(Month { year: 1970, month: 1 }.start(), 0);
    }

    #[test]
    fn a_span_is_written_when_its_status_ends() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY + 1000);
        assert!(lines(&tmp, SEPTEMBER).is_empty(), "nothing until it ends");
        // The same facts again (another field changed): the span goes on.
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY + 2000);
        j.observe_at(&[agent(1, AgentStatus::NeedsInput, "main")], tab, DAY + 5000);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY + 9000);
        assert_eq!(
            lines(&tmp, SEPTEMBER),
            [
                r#"{"k":"ctx","id":1,"tab":"t1","agent":"claude","repo":"/r/.git","name":"r","br":"main"}"#.to_owned(),
                format!(r#"{{"k":"span","c":1,"s":"running","a":{},"b":{}}}"#, DAY + 1000, DAY + 5000),
                format!(r#"{{"k":"span","c":1,"s":"needs-input","a":{},"b":{}}}"#, DAY + 5000, DAY + 9000),
            ],
            "the Context once, then its spans"
        );
        let spans = j.read_at(DAY, DAY + DAY_MS, DAY + 10_000);
        assert_eq!(
            shape(&spans),
            [
                ("main", AgentStatus::Running, 1000, 5000),
                ("main", AgentStatus::NeedsInput, 5000, 9000),
                ("main", AgentStatus::Running, 9000, 10_000),
            ],
            "the open span too, up to now"
        );
        assert_eq!(spans[0].context.tab, "t1");
        assert_eq!(spans[0].context.repo.as_deref(), Some("/r/.git"));
    }

    #[test]
    fn a_span_row_stays_small() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::NeedsInput, "main")], tab, DAY);
        j.end_at(1, DAY + 1);
        let rows = lines(&tmp, SEPTEMBER);
        assert!(rows[1].len() <= 72, "{} bytes: {}", rows[1].len(), rows[1]);
    }

    #[test]
    fn another_branch_is_another_context() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY);
        j.observe_at(&[agent(1, AgentStatus::Running, "fix")], tab, DAY + 100);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY + 300);
        j.end_at(1, DAY + 600);
        assert_eq!(
            shape(&j.read_at(DAY, DAY + DAY_MS, DAY + 700)),
            [
                ("main", AgentStatus::Running, 0, 100),
                ("fix", AgentStatus::Running, 100, 300),
                ("main", AgentStatus::Running, 300, 600),
            ]
        );
        let contexts = lines(&tmp, SEPTEMBER).iter().filter(|l| l.contains(r#""k":"ctx""#)).count();
        assert_eq!(contexts, 2, "main is written once");
    }

    #[test]
    fn only_an_agent_in_a_tab_is_kept() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let shell = SessionInfo { cwd: Some("/r".into()), ..SessionInfo::empty(1) };
        j.observe_at(&[shell.clone()], tab, DAY);
        j.observe_at(&[agent(2, AgentStatus::Running, "main")], |_| None, DAY);
        j.observe_at(&[agent(3, AgentStatus::Running, "main")], tab, DAY);
        // The agent leaves: its Session is a shell again.
        j.observe_at(&[SessionInfo { session_id: 3, ..shell }], tab, DAY + 50);
        j.end_at(1, DAY + 60);
        j.end_at(2, DAY + 60);
        let spans = j.read_at(DAY, DAY + DAY_MS, DAY + 100);
        assert_eq!(shape(&spans), [("main", AgentStatus::Running, 0, 50)]);
        assert_eq!(spans[0].context.tab, "t3");
    }

    #[test]
    fn outside_a_repo_the_cwd_stands_for_it_and_a_remote_hop_has_neither() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let mut info = agent(1, AgentStatus::Done, "main");
        info.git = None;
        j.observe_at(&[info], tab, DAY);
        let mut far = agent(2, AgentStatus::Done, "main");
        far.remote = true;
        j.observe_at(&[far], tab, DAY);
        j.end_at(1, DAY + 10);
        j.end_at(2, DAY + 20);
        let rows = lines(&tmp, SEPTEMBER);
        assert_eq!(rows[0], r#"{"k":"ctx","id":1,"tab":"t1","agent":"claude","cwd":"/r/src"}"#);
        assert_eq!(rows[2], r#"{"k":"ctx","id":2,"tab":"t2","agent":"claude","remote":true}"#);
    }

    #[test]
    fn a_span_over_the_end_of_a_month_is_two() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let october = Month::of(DAY).next().start();
        j.observe_at(&[agent(1, AgentStatus::Done, "main")], tab, october - 500);
        j.end_at(1, october + 700);
        assert_eq!(lines(&tmp, SEPTEMBER).len(), 2);
        assert_eq!(lines(&tmp, "2026-10.jsonl").len(), 2, "each file names its own Contexts");
        let both = j.read_at(october - DAY_MS, october + DAY_MS, october + 1000);
        assert_eq!(
            both.iter().map(|s| (s.start, s.end)).collect::<Vec<_>>(),
            [(october - 500, october), (october, october + 700)]
        );
        let cut = j.read_at(october - 100, october + 100, october + 1000);
        assert_eq!(
            cut.iter().map(|s| (s.start, s.end)).collect::<Vec<_>>(),
            [(october - 100, october), (october, october + 100)],
            "cut to what was asked for"
        );
    }

    #[test]
    fn the_next_launch_ends_what_a_crash_left_open() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY);
        j.observe_at(&[agent(1, AgentStatus::Done, "main")], tab, DAY + 1000);
        j.checkpoint(DAY + 4000);
        drop(j); // the crash: no `finish`

        let j = journal(&tmp);
        assert!(!tmp.path().join("journal").join(OPEN).exists());
        assert_eq!(
            shape(&j.read_at(DAY, DAY + DAY_MS, DAY + 9000)),
            [("main", AgentStatus::Running, 0, 1000), ("main", AgentStatus::Done, 1000, 4000)],
            "ended when the Host was last known alive"
        );
        // This run numbers its Contexts from 1 again, and the file still reads.
        j.observe_at(&[agent(1, AgentStatus::Running, "fix")], tab, DAY + 5000);
        j.end_at(1, DAY + 6000);
        let spans = j.read_at(DAY, DAY + DAY_MS, DAY + 9000);
        assert_eq!(shape(&spans)[2], ("fix", AgentStatus::Running, 5000, 6000));
        assert_eq!(shape(&spans)[1].0, "main", "what was written before keeps its Context");
    }

    #[test]
    fn open_json_goes_once_nothing_is_open() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let open = tmp.path().join("journal").join(OPEN);
        j.checkpoint(DAY);
        assert!(!open.exists(), "nothing open, nothing written");
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY);
        j.checkpoint(DAY + 10);
        assert!(open.exists());
        j.end_at(1, DAY + 20);
        j.checkpoint(DAY + 30);
        assert!(!open.exists());
    }

    #[test]
    fn stopping_ends_every_span_and_writes_no_more() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Running, "main"), agent(2, AgentStatus::Done, "main")], tab, DAY);
        j.checkpoint(DAY + 10);
        j.finish_at(DAY + 100);
        assert!(!tmp.path().join("journal").join(OPEN).exists());
        // The Sessions die as the Host kills them: not an agent's doing.
        j.observe_at(&[agent(1, AgentStatus::Done, "main")], tab, DAY + 200);
        j.end_at(1, DAY + 300);
        j.checkpoint(DAY + 400);
        let mut spans = shape(&j.read_at(DAY, DAY + DAY_MS, DAY + 500))
            .into_iter()
            .map(|(_, status, start, end)| (status, start, end))
            .collect::<Vec<_>>();
        spans.sort_by_key(|s| s.0 == AgentStatus::Done);
        assert_eq!(spans, [(AgentStatus::Running, 0, 100), (AgentStatus::Done, 0, 100)]);
    }

    #[test]
    fn a_line_cut_short_costs_only_itself() {
        let tmp = TempDir::new("journal");
        let dir = tmp.path().join("journal");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(SEPTEMBER), r#"{"k":"span","c":1,"s":"runn"#).unwrap();
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY);
        j.end_at(1, DAY + 10);
        assert_eq!(shape(&j.read_at(DAY, DAY + DAY_MS, DAY + 20)), [("main", AgentStatus::Running, 0, 10)]);
    }

    fn used(kind: ToolKind) -> ToolUse {
        ToolUse { kind, failed: false, added: 0, removed: 0, file: None, program: None }
    }

    fn edit(file: &str, added: u32, removed: u32) -> Step {
        Step::Tool(ToolUse { added, removed, file: Some(file.into()), ..used(ToolKind::Edit) })
    }

    fn run(program: &str) -> Step {
        Step::Tool(ToolUse { program: Some(program.into()), ..used(ToolKind::Run) })
    }

    #[test]
    fn a_turn_is_one_row_of_counts() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Done, "main")], tab, DAY);
        // Before any prompt: not a turn's.
        j.step(1, &run("ls"), DAY + 10);
        j.step(1, &Step::Prompt { len: 38 }, DAY + 1000);
        j.step(1, &Step::Tool(used(ToolKind::Read)), DAY + 1100);
        j.step(1, &edit("/r/a.rs", 5, 2), DAY + 1200);
        j.step(1, &edit("/r/a.rs", 4, 0), DAY + 1300);
        j.step(1, &run("cargo"), DAY + 1400);
        j.step(1, &Step::Tool(ToolUse { failed: true, ..used(ToolKind::Run) }), DAY + 1500);
        assert!(lines(&tmp, SEPTEMBER).is_empty(), "nothing until it ends");
        j.step(1, &Step::Stop, DAY + 9000);
        // After the stop: not a turn's either.
        j.step(1, &run("make"), DAY + 9500);
        j.step(1, &Step::Stop, DAY + 9600);
        assert_eq!(
            lines(&tmp, SEPTEMBER),
            [
                r#"{"k":"ctx","id":1,"tab":"t1","agent":"claude","repo":"/r/.git","name":"r","br":"main"}"#.to_owned(),
                format!(
                    r#"{{"k":"turn","c":1,"a":{},"b":{},"len":38,"tools":{{"edit":2,"read":1,"run":2}},"fail":1,"add":9,"del":2,"files":1,"cmds":{{"cargo":1}}}}"#,
                    DAY + 1000,
                    DAY + 9000
                ),
            ]
        );
        let turns = j.turns_at(DAY, DAY + DAY_MS, DAY + 10_000);
        assert_eq!(turns.len(), 1);
        assert_eq!((turns[0].start, turns[0].end), (DAY + 1000, DAY + 9000));
        assert_eq!(turns[0].context.br.as_deref(), Some("main"));
        assert_eq!(turns[0].tally.tools[&ToolKind::Edit], 2);
        assert!(j.read_at(DAY, DAY + DAY_MS, DAY + 10_000).len() == 1, "the span is read as before");
    }

    #[test]
    fn a_question_waits_until_the_agents_next_step() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY);
        j.step(1, &Step::Prompt { len: 1 }, DAY);
        j.step(1, &Step::Asked, DAY + 1000);
        j.step(1, &Step::Answered, DAY + 4000);
        j.step(1, &run("ls"), DAY + 4100);
        // Answered in its Terminal: the tool it asked for is its next step.
        j.step(1, &Step::Asked, DAY + 5000);
        j.step(1, &run("ls"), DAY + 7000);
        // Still waiting when the turn is read, and when it stops.
        j.step(1, &Step::Asked, DAY + 8000);
        let going = j.turns_at(DAY, DAY + DAY_MS, DAY + 8500);
        assert_eq!((going[0].tally.asked, going[0].tally.waited, going[0].end), (3, 5500, DAY + 8500));
        j.step(1, &Step::Stop, DAY + 9000);
        let turns = j.turns_at(DAY, DAY + DAY_MS, DAY + 9500);
        assert_eq!((turns[0].tally.asked, turns[0].tally.waited), (3, 3000 + 2000 + 1000));
    }

    #[test]
    fn a_turn_with_no_stop_ends_at_the_next_prompt_or_when_its_agent_left() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let shell = |id| SessionInfo { cwd: Some("/r".into()), ..SessionInfo::empty(id) };
        j.observe_at(&[agent(1, AgentStatus::Running, "main"), agent(2, AgentStatus::Running, "main")], tab, DAY);
        // Esc, then another prompt.
        j.step(1, &Step::Prompt { len: 1 }, DAY + 100);
        j.step(1, &Step::Prompt { len: 2 }, DAY + 500);
        // Its agent is lost sight of, and says more: it had not left.
        j.observe_at(&[shell(1)], tab, DAY + 600);
        j.step(1, &run("ls"), DAY + 700);
        j.observe_at(&[agent(1, AgentStatus::Running, "fix")], tab, DAY + 800);
        j.step(1, &Step::Stop, DAY + 900);
        // Killed: no stop, no more steps, and the Session goes on as a shell.
        j.step(2, &Step::Prompt { len: 3 }, DAY + 100);
        j.observe_at(&[shell(2)], tab, DAY + 300);
        j.observe_at(&[shell(2)], tab, DAY + 400);
        j.end_at(2, DAY + 5000);
        let turns = j.turns_at(DAY, DAY + DAY_MS, DAY + 6000);
        let shape: Vec<_> = turns.iter().map(|t| (t.tally.len, t.start - DAY, t.end - DAY, t.context.br.as_deref())).collect();
        assert_eq!(
            shape,
            [(1, 100, 500, Some("main")), (2, 500, 900, Some("fix")), (3, 100, 300, Some("main"))],
            "under the Context it ended in"
        );
    }

    #[test]
    fn a_turn_counts_a_dozen_programs_at_most() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY);
        j.step(1, &Step::Prompt { len: 1 }, DAY);
        for i in 0..PROGRAMS_MAX + 3 {
            j.step(1, &run(&format!("p{i:02}")), DAY + 1);
        }
        j.step(1, &run("p00"), DAY + 2);
        j.step(1, &Step::Stop, DAY + 3);
        let tally = &j.turns_at(DAY, DAY + DAY_MS, DAY + 4)[0].tally;
        assert_eq!(tally.cmds.len(), PROGRAMS_MAX);
        assert_eq!(tally.cmds["p00"], 2);
        assert_eq!(tally.tools[&ToolKind::Run] as usize, PROGRAMS_MAX + 4, "every command is still a tool used");
    }

    #[test]
    fn a_turn_is_in_the_month_it_began_in_and_survives_a_crash() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let october = Month::of(DAY).next().start();
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, october - 500);
        j.step(1, &Step::Prompt { len: 7 }, october - 400);
        j.step(1, &edit("/r/a.rs", 1, 0), october - 300);
        j.step(1, &Step::Asked, october + 100);
        j.checkpoint(october + 300);
        drop(j); // the crash

        let j = journal(&tmp);
        assert!(j.turns_at(october, october + DAY_MS, october + 900).is_empty());
        let turns = j.turns_at(october - DAY_MS, october, october + 900);
        assert_eq!(turns.len(), 1);
        assert_eq!((turns[0].start, turns[0].end), (october - 400, october + 300));
        assert_eq!(
            turns[0].tally,
            Tally { len: 7, tools: BTreeMap::from([(ToolKind::Edit, 1)]), add: 1, files: 1, asked: 1, waited: 200, ..Tally::default() }
        );
        assert_eq!(j.read_at(october - DAY_MS, october + DAY_MS, october + 900).len(), 2, "its span, in two");
    }

    #[test]
    fn stopping_ends_the_turns_too() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY);
        j.step(1, &Step::Prompt { len: 1 }, DAY + 10);
        j.finish_at(DAY + 100);
        j.step(1, &run("ls"), DAY + 200);
        j.step(1, &Step::Prompt { len: 2 }, DAY + 300);
        let turns = j.turns_at(DAY, DAY + DAY_MS, DAY + 400);
        assert_eq!(turns.iter().map(|t| (t.tally.len, t.end - DAY)).collect::<Vec<_>>(), [(1, 100)]);
    }

    /// A transcript line with Claude Code's totals, in the shape of 2.1.283's.
    fn cost_state(start: u64, models: &[(&str, u64, f64)]) -> String {
        let usage: serde_json::Map<String, serde_json::Value> = models
            .iter()
            .map(|(model, out, usd)| {
                let u = serde_json::json!({
                    "inputTokens": 10, "outputTokens": out, "thinkingTokens": 0,
                    "cacheReadInputTokens": out * 100, "cacheCreationInputTokens": 0,
                    "webSearchRequests": 0, "costUSD": usd,
                });
                (model.to_string(), u)
            })
            .collect();
        let line = serde_json::json!({
            "type": "cost-state", "sessionId": "s", "startTime": start, "totalCostUSD": 0.0,
            "hasUnknownModelCost": false, "modelUsage": usage,
        });
        format!("{line}\n")
    }

    fn add(path: &Path, lines: &str) {
        let mut f = fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        f.write_all(lines.as_bytes()).unwrap();
    }

    const OPUS: &str = "claude-opus-5-5";
    const TALK: &str = "{\"type\":\"assistant\",\"message\":{\"usage\":{\"output_tokens\":5}}}\n";

    fn outs(costs: &[Cost]) -> Vec<(u64, u64, u64, u64)> {
        costs.iter().map(|c| (c.start - DAY, c.end - DAY, c.models[OPUS].out, c.models[OPUS].usd)).collect()
    }

    #[test]
    fn a_cost_is_what_the_totals_moved_by() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let file = tmp.path().join("1.jsonl");
        let path = file.to_string_lossy().into_owned();
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY);
        j.step(1, &Step::Transcript(path.clone()), DAY + 100);
        j.measure(DAY + 200);
        add(&file, TALK);
        j.measure(DAY + 300);
        assert!(j.costs(DAY, DAY + DAY_MS).is_empty(), "no totals yet");
        add(&file, &cost_state(7, &[(OPUS, 1000, 0.5), ("claude-haiku-4-5", 20, 0.001)]));
        j.measure(DAY + 1000);
        add(&file, TALK);
        j.measure(DAY + 2000);
        add(&file, &cost_state(7, &[(OPUS, 1500, 0.75), ("claude-haiku-4-5", 20, 0.001)]));
        j.measure(DAY + 3000);
        j.measure(DAY + 4000);
        let costs = j.costs(DAY, DAY + DAY_MS);
        assert_eq!(outs(&costs), [(100, 1000, 1000, 500_000), (1000, 3000, 500, 250_000)]);
        assert_eq!(costs[0].models["claude-haiku-4-5"], Tokens { input: 10, out: 20, cr: 2000, usd: 1000, ..Tokens::default() });
        assert_eq!(costs[1].models.len(), 1, "a model that used nothing more is not written");
        assert_eq!(costs[1].models[OPUS], Tokens { out: 500, cr: 50_000, usd: 250_000, ..Tokens::default() });
        assert_eq!(costs[0].context.br.as_deref(), Some("main"));
        let row = lines(&tmp, SEPTEMBER).into_iter().find(|l| l.contains(r#""k":"cost""#)).unwrap();
        assert!(row.starts_with(&format!(r#"{{"k":"cost","c":1,"a":{},"b":{},"m":{{"#, DAY + 100, DAY + 1000)), "{row}");
    }

    #[test]
    fn totals_are_found_however_much_was_written_after_them() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let file = tmp.path().join("1.jsonl");
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY);
        j.step(1, &Step::Transcript(file.to_string_lossy().into_owned()), DAY);
        add(&file, TALK);
        j.measure(DAY + 100);
        // Between two looks: the totals, then more than the first look would read.
        add(&file, &cost_state(7, &[(OPUS, 1000, 0.5)]));
        add(&file, &TALK.repeat(2 * TAIL as usize / TALK.len()));
        j.measure(DAY + 200);
        assert_eq!(outs(&j.costs(DAY, DAY + DAY_MS)), [(0, 200, 1000, 500_000)]);
    }

    #[test]
    fn what_a_transcript_held_before_it_was_looked_at_is_not_counted() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let file = tmp.path().join("1.jsonl");
        add(&file, &cost_state(7, &[(OPUS, 1000, 0.5)]));
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY);
        j.step(1, &Step::Transcript(file.to_string_lossy().into_owned()), DAY + 100);
        j.measure(DAY + 200);
        assert!(j.costs(DAY, DAY + DAY_MS).is_empty());
        add(&file, &cost_state(7, &[(OPUS, 1300, 0.6)]));
        j.measure(DAY + 300);
        assert_eq!(outs(&j.costs(DAY, DAY + DAY_MS)), [(100, 300, 300, 100_000)]);
    }

    #[test]
    fn totals_carry_on_over_a_clear_and_a_resume_and_start_again_in_a_new_run() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let (one, two) = (tmp.path().join("1.jsonl"), tmp.path().join("2.jsonl"));
        let path = |f: &PathBuf| f.to_string_lossy().into_owned();
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY);
        j.step(1, &Step::Transcript(path(&one)), DAY);
        add(&one, &cost_state(7, &[(OPUS, 1000, 0.5)]));
        j.measure(DAY + 100);
        // `/clear`: another transcript, the same run, its totals carrying on.
        j.step(1, &Step::Transcript(path(&two)), DAY + 200);
        add(&two, &cost_state(7, &[(OPUS, 1200, 0.6)]));
        j.measure(DAY + 300);
        // A resume: another run, carrying the totals on.
        add(&two, &cost_state(8, &[(OPUS, 1250, 0.7)]));
        j.measure(DAY + 400);
        // Another run that counts from nothing.
        add(&two, &cost_state(9, &[(OPUS, 40, 0.02)]));
        j.measure(DAY + 500);
        add(&two, &cost_state(9, &[(OPUS, 90, 0.03)]));
        j.measure(DAY + 600);
        assert_eq!(
            outs(&j.costs(DAY, DAY + DAY_MS)),
            [(0, 100, 1000, 500_000), (200, 300, 200, 100_000), (300, 400, 50, 100_000), (400, 500, 40, 20_000), (500, 600, 50, 10_000)]
        );
    }

    #[test]
    fn totals_written_while_the_host_was_down_are_counted_by_the_next_launch() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let file = tmp.path().join("1.jsonl");
        j.observe_at(&[agent(1, AgentStatus::Running, "fix")], tab, DAY);
        j.step(1, &Step::Transcript(file.to_string_lossy().into_owned()), DAY);
        add(&file, &cost_state(7, &[(OPUS, 1000, 0.5)]));
        j.measure(DAY + 100);
        j.finish_at(DAY + 200);
        // Claude Code writes its totals as the Host kills it.
        add(&file, &cost_state(7, &[(OPUS, 1100, 0.55)]));

        let j = journal(&tmp);
        j.measure(DAY + 9000);
        let costs = j.costs(DAY, DAY + DAY_MS);
        assert_eq!(outs(&costs), [(0, 100, 1000, 500_000), (100, 9000, 100, 50_000)]);
        assert_eq!(costs[1].context.br.as_deref(), Some("fix"), "under the Context it was last in");
        // Long after, in no Session: let go.
        j.measure(DAY + 9000 + METER_IDLE_MS + 1);
        assert!(lock(&j.inner.state).watched.is_empty());
    }

    /// `JOURNAL_TRANSCRIPT=<a transcript> cargo test -p sidebar-term-core real_transcript -- --ignored --nocapture`
    #[test]
    #[ignore = "reads a real Claude Code transcript named by JOURNAL_TRANSCRIPT"]
    fn real_transcript() {
        let path = std::env::var("JOURNAL_TRANSCRIPT").expect("JOURNAL_TRANSCRIPT");
        let len = fs::metadata(&path).unwrap().len();
        let totals = totals_in(Path::new(&path), len.saturating_sub(TAIL)).expect("no cost-state in its last MiB");
        assert!(totals.start > 0);
        for (model, t) in &totals.models {
            println!("{model}: {t:?}");
        }
        assert!(totals.models.values().any(|t| t.out > 0 && t.usd > 0));
    }

    #[test]
    fn without_a_data_dir_nothing_is_kept_and_nothing_breaks() {
        let j = Journal::open(None);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], tab, DAY);
        j.checkpoint(DAY + 5);
        assert_eq!(j.read_at(DAY, DAY + DAY_MS, DAY + 10).len(), 1, "the open span is still known");
        j.end_at(1, DAY + 10);
        assert!(j.read_at(DAY, DAY + DAY_MS, DAY + 20).is_empty());
        j.finish_at(DAY + 30);
    }
}
