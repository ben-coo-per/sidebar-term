//! The Journal: how much agent time each repo and branch on this Host had, hour by hour, kept
//! on disk for Rewind (docs/architecture.md "Journal", ADR 0004). The status history in
//! `agents/` goes back a few hours and lives in memory; this is the part that stays.
//!
//! **A tally an hour.** Nothing is written as it happens. For each hour and each [`Context`]
//! (the agent, the repo and branch) the Journal keeps one [`Tally`]: seconds agents worked,
//! seconds they waited on the user, prompts given, lines added and removed, and what Claude
//! Code says it used, by model. When the hour is over the tally is one row. So the Journal
//! grows with the hours agents were at work and the repos they were in, never with how much
//! they did or how often their status changed. No words are kept: not a prompt, not a path
//! but the repo's.
//!
//! **Files.** `journal/<year>-<month>.jsonl` in the Host's data dir, one JSON object per line,
//! appended; a row is in the file of the month its hour began in (UTC). Two kinds of row:
//!
//! ```text
//! {"k":"ctx","id":1,"agent":"claude","repo":"/r/.git","name":"r","br":"main"}
//! {"k":"hour","h":497376,"c":1,"run":1800,"wait":240,"turns":3,"add":120,"del":30,"m":{"claude-opus-5-5":{"out":9100,"usd":1420000}}}
//! ```
//!
//! A `ctx` row gives a Context a number, and the rows after it name it by that number, so a
//! repo's path is written once. The number holds for the rows that follow it, until a later
//! `ctx` row gives the same number to something else (each run of the Host starts again at 1),
//! so a file is read from the top. `h` is the hour, counted from the epoch. An hour and a
//! Context may have several rows (the Host stopped and started within the hour); they add up.
//! A line that does not parse is skipped: a crash can cut the last one short.
//!
//! **Cost.** Claude Code keeps running totals of the tokens it used and what they cost, per
//! model, and writes them into the conversation's transcript now and then (a `cost-state`
//! line: when it exits, at some idle moments; not every turn). The hooks name the transcript;
//! every [`TICK`] the Journal reads what each one has gained, and what the totals moved by
//! goes into the tally of the hour the file was written in. The totals it has counted are
//! kept in `journal/meters.json`, so what Claude Code writes as the Host kills it is counted by
//! the next launch. The messages in a transcript carry token counts too; they are not used,
//! because they add up to well under the totals.
//!
//! **A crash.** The tallies not yet written would go with the process. Every [`TICK`] they
//! are written to `journal/open.json`; the next launch writes them as rows.

use crate::agents::hooks::Step;
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

/// Time between two ticks: what a crash can lose of each tally, and how soon a transcript's
/// totals are read.
const TICK: Duration = Duration::from_secs(60);
/// The tallies not yet written, for the next launch (see the module's words on a crash).
const OPEN: &str = "open.json";
/// The transcripts looked at for Claude Code's totals, and the totals counted so far.
const METERS: &str = "meters.json";
/// How much of a transcript's end is read for the totals when it is first looked at. After
/// that, what it gained since is read.
const TAIL: u64 = 1024 * 1024;
/// The most of a transcript read at once; of more than that, the end.
const READ_MAX: u64 = 8 * 1024 * 1024;
const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 24 * HOUR_MS;
/// A transcript whose totals have not moved for this long, in no live Session, is let go.
const METER_IDLE_MS: u64 = 30 * DAY_MS;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero<T: Default + PartialEq>(n: &T) -> bool {
    *n == T::default()
}

/// Where an agent ran and which one: what agent time is counted under.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Context {
    pub agent: AgentKind,
    /// The repo's identity, `GitInfo.common_dir`: the same for every Worktree of one repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// The repo's display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    /// The Context of `info`'s agent; `None` when it has no agent.
    fn of(info: &SessionInfo) -> Option<Self> {
        let agent = info.agent?;
        // Over a remote hop, cwd and git describe this Host, not the far side.
        let git = info.git.as_ref().filter(|_| !info.remote);
        Some(Self {
            agent,
            repo: git.map(|g| g.common_dir.clone()),
            name: git.map(|g| g.repo_name.clone()),
            br: git.and_then(|g| g.branch.clone()),
            cwd: match git {
                Some(_) => None,
                None => info.cwd.clone().filter(|_| !info.remote),
            },
            remote: info.remote,
        })
    }
}

/// What one model was used for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spend {
    /// Output tokens.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub out: u64,
    /// Millionths of a dollar, as Claude Code worked it out.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub usd: u64,
}

impl Spend {
    /// What `self` has over `base`, nothing where it has less.
    fn over(&self, base: &Spend) -> Spend {
        Spend { out: self.out.saturating_sub(base.out), usd: self.usd.saturating_sub(base.usd) }
    }

    fn add(&mut self, more: &Spend) {
        self.out += more.out;
        self.usd += more.usd;
    }
}

/// What was used by model, under the model's id (`claude-opus-5-5`).
pub type Models = BTreeMap<String, Spend>;

/// One hour in one Context, counted. What is zero is not written.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    /// Seconds agents worked (Agent status Running). Two at once for an hour are 7200.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub run: u64,
    /// Seconds agents waited on the user (Needs input).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wait: u64,
    /// Prompts the user gave hooked agents.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub turns: u32,
    /// Lines hooked agents' edits added.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub add: u32,
    /// Lines hooked agents' edits removed.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub del: u32,
    /// What Claude Code says it used, by model, counted in the hour it wrote its totals.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub m: Models,
}

impl Tally {
    fn add(&mut self, more: &Tally) {
        self.run += more.run;
        self.wait += more.wait;
        self.turns += more.turns;
        self.add += more.add;
        self.del += more.del;
        for (model, spend) in &more.m {
            self.m.entry(model.clone()).or_default().add(spend);
        }
    }
}

/// One hour in one Context, as [`Journal::read`] gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hour {
    /// When the hour began, epoch ms.
    pub start: u64,
    pub context: Context,
    pub tally: Tally,
}

/// A tally being kept, its times in ms.
#[derive(Default)]
struct Counting {
    run_ms: u64,
    wait_ms: u64,
    rest: Tally,
}

impl Counting {
    fn tally(&self) -> Tally {
        let secs = |ms: u64| (ms + 500) / 1000;
        Tally { run: secs(self.run_ms), wait: secs(self.wait_ms), ..self.rest.clone() }
    }
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
    Hour {
        h: u64,
        c: u32,
        #[serde(flatten)]
        tally: Tally,
    },
}

/// `open.json`.
#[derive(Serialize, Deserialize)]
struct OpenFile {
    hours: Vec<OpenHour>,
}

#[derive(Serialize, Deserialize)]
struct OpenHour {
    h: u64,
    #[serde(flatten)]
    context: Context,
    #[serde(flatten)]
    tally: Tally,
}

/// An agent at work or waiting, its time counted up to `since`.
struct Open {
    context: Context,
    status: AgentStatus,
    since: u64,
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
    /// The agents at work or waiting.
    open: HashMap<SessionId, Open>,
    /// The Context each live Session's agent was last seen in.
    contexts: HashMap<SessionId, Context>,
    /// The tallies not yet written, by hour and Context.
    hours: HashMap<(u64, Context), Counting>,
    /// `hours` has changed since `open.json` was written.
    changed: bool,
    /// `open.json` holds tallies (so it has to go once none is left).
    checkpointed: bool,
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
    /// The Host is stopping: its dying Sessions are not counted.
    finished: bool,
    /// A write failed and was said so; said again only after one that worked.
    failing: bool,
}

impl Journal {
    /// The Journal in `dir`, made on the first write. Tallies the last run left unwritten (it
    /// crashed, or was killed) are written now.
    pub fn open(dir: Option<PathBuf>) -> Self {
        let journal = Self { inner: Arc::new(Inner { dir, state: Mutex::default() }) };
        if let Some(dir) = &journal.inner.dir {
            let mut state = lock(&journal.inner.state);
            let path = dir.join(OPEN);
            if let Some(left) = fs::read(&path).ok().and_then(|b| serde_json::from_slice::<OpenFile>(&b).ok()) {
                let mut hours = left.hours;
                hours.sort_by_key(|hour| hour.h);
                for hour in hours {
                    state.write(dir, hour.h, &hour.context, hour.tally);
                }
            }
            let _ = fs::remove_file(&path);
            let meters = fs::read(dir.join(METERS)).ok().and_then(|b| serde_json::from_slice::<MetersFile>(&b).ok());
            state.watched = meters.unwrap_or_default().files;
        }
        journal
    }

    /// The monitor's word on Sessions that changed: an agent that is working or waiting has
    /// its time counted from now, under the Context it is in, and one that was has it counted
    /// up to now. An agent idle at its prompt is not counted.
    pub fn observe(&self, infos: &[SessionInfo]) {
        self.observe_at(infos, now_ms());
    }

    fn observe_at(&self, infos: &[SessionInfo], now: u64) {
        let mut state = lock(&self.inner.state);
        if state.finished {
            return;
        }
        for info in infos {
            let id = info.session_id;
            let context = Context::of(info);
            if let Some(context) = &context {
                state.contexts.insert(id, context.clone());
            }
            let now_in = context.zip(info.status).filter(|(_, status)| *status != AgentStatus::Done);
            let same = match (state.open.get(&id), &now_in) {
                (Some(open), Some((context, status))) => open.context == *context && open.status == *status,
                (None, None) => true,
                _ => false,
            };
            if same {
                continue;
            }
            if let Some(open) = state.open.remove(&id) {
                state.spend(&open, now);
            }
            if let Some((context, status)) = now_in {
                state.open.insert(id, Open { context, status, since: now });
            }
        }
    }

    /// A step of Session `id`'s hooked agent, at `at` (`Agents::watch_steps`): a prompt and an
    /// edit's lines are counted in the hour and the Context they came in, and a transcript is
    /// watched from now on. A step of a Session whose Context is not known is not counted.
    pub fn step(&self, id: SessionId, step: &Step, at: u64) {
        let mut state = lock(&self.inner.state);
        if state.finished {
            return;
        }
        let context = state.contexts.get(&id).cloned();
        match (step, context) {
            (Step::Transcript(path), context) => {
                state.transcripts.insert(id, path.clone());
                if self.inner.dir.is_some() && !state.watched.contains_key(path) {
                    // What it holds already is not this Journal's to count: it was used before
                    // the file was looked at.
                    let len = fs::metadata(path).map_or(0, |m| m.len());
                    let seen = totals_in(Path::new(path), len.saturating_sub(TAIL)).unwrap_or_default();
                    let watched = Watched { ctx: context, len, at, start: seen.start, totals: seen.models };
                    state.watched.insert(path.clone(), watched);
                    state.unsaved = true;
                }
            }
            (Step::Prompt, Some(context)) => state.counting(at, &context).rest.turns += 1,
            (Step::Edit { added, removed }, Some(context)) => {
                let tally = &mut state.counting(at, &context).rest;
                tally.add += added;
                tally.del += removed;
            }
            (_, None) => {}
        }
    }

    /// Session `id` ended: its agent's time is counted up to now.
    pub fn end(&self, id: SessionId) {
        self.end_at(id, now_ms());
    }

    fn end_at(&self, id: SessionId, now: u64) {
        let mut state = lock(&self.inner.state);
        if let Some(open) = state.open.remove(&id) {
            if !state.finished {
                state.spend(&open, now);
            }
        }
        state.contexts.remove(&id);
        // Its transcript stays watched: Claude Code writes its totals as it goes.
        state.transcripts.remove(&id);
    }

    /// The Host is stopping: every tally is written, this hour's too, and nothing is counted
    /// after.
    pub fn finish(&self) {
        self.finish_at(now_ms());
    }

    fn finish_at(&self, now: u64) {
        self.measure(now);
        let mut state = lock(&self.inner.state);
        if state.finished {
            return;
        }
        state.settle(now);
        state.open.clear();
        state.flush(self.inner.dir.as_deref(), None);
        state.finished = true;
        if let Some(dir) = &self.inner.dir {
            let _ = fs::remove_file(dir.join(OPEN));
        }
    }

    /// What is done every [`TICK`]: read the transcripts, count the time of the agents at work
    /// up to now, write the tallies of the hours that are over, and keep the rest in
    /// `open.json`.
    fn tick(&self, now: u64) {
        self.measure(now);
        let mut state = lock(&self.inner.state);
        if state.finished {
            return;
        }
        state.settle(now);
        state.flush(self.inner.dir.as_deref(), Some(now / HOUR_MS));
        let Some(dir) = &self.inner.dir else { return };
        let path = dir.join(OPEN);
        if state.hours.is_empty() {
            if state.checkpointed {
                let _ = fs::remove_file(&path);
                state.checkpointed = false;
            }
        } else if state.changed {
            let hours = state.hours.iter().map(|((h, context), counting)| OpenHour {
                h: *h,
                context: context.clone(),
                tally: counting.tally(),
            });
            let file = OpenFile { hours: hours.collect() };
            let written = fs::create_dir_all(dir)
                .map_err(|e| e.to_string())
                .and_then(|_| crate::store::write(&path, &file));
            state.changed = written.is_err();
            state.checkpointed = true;
            state.said(written.map_err(|e| format!("{OPEN}: {e}")));
        }
    }

    /// Look at every watched transcript that changed, and count what Claude Code's totals
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
            state.count(&path, len, written, totals);
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

    /// Every hour that began in `from..to` (epoch ms) in which something was counted, one
    /// [`Hour`] per hour and Context, the hours in order. What is not yet written is in it, up
    /// to now.
    pub fn read(&self, from: u64, to: u64) -> Vec<Hour> {
        self.read_at(from, to, now_ms())
    }

    fn read_at(&self, from: u64, to: u64, now: u64) -> Vec<Hour> {
        if from >= to {
            return Vec::new();
        }
        let mut state = lock(&self.inner.state);
        if !state.finished {
            state.settle(now);
        }
        let mut rows = Vec::new();
        if let Some(dir) = &self.inner.dir {
            let last = Month::of(to - 1);
            let mut month = Month::of(from);
            while month <= last {
                rows.extend(read_file(&dir.join(month.file())));
                month = month.next();
            }
        }
        rows.extend(state.hours.iter().map(|((h, context), counting)| (*h, context.clone(), counting.tally())));
        drop(state);
        let mut hours: Vec<Hour> = Vec::new();
        let mut at: HashMap<(u64, Context), usize> = HashMap::new();
        for (h, context, tally) in rows {
            let start = h * HOUR_MS;
            if !(from..to).contains(&start) || tally == Tally::default() {
                continue;
            }
            match at.get(&(h, context.clone())) {
                Some(i) => hours[*i].tally.add(&tally),
                None => {
                    at.insert((h, context.clone()), hours.len());
                    hours.push(Hour { start, context, tally });
                }
            }
        }
        hours.sort_by_key(|hour| hour.start);
        hours
    }
}

impl State {
    /// The tally of the hour `at` is in, in `context`.
    fn counting(&mut self, at: u64, context: &Context) -> &mut Counting {
        self.changed = true;
        self.hours.entry((at / HOUR_MS, context.clone())).or_default()
    }

    /// Count `open`'s time up to `now`, in each hour it was in.
    fn spend(&mut self, open: &Open, now: u64) {
        let mut start = open.since;
        while start < now {
            let stop = now.min((start / HOUR_MS + 1) * HOUR_MS);
            let counting = self.counting(start, &open.context);
            match open.status {
                AgentStatus::Running => counting.run_ms += stop - start,
                AgentStatus::NeedsInput => counting.wait_ms += stop - start,
                AgentStatus::Done => {}
            }
            start = stop;
        }
    }

    /// Count the time of every agent at work or waiting up to `now`.
    fn settle(&mut self, now: u64) {
        let mut open = std::mem::take(&mut self.open);
        for o in open.values_mut() {
            self.spend(o, now);
            o.since = o.since.max(now);
        }
        self.open = open;
    }

    /// Write the tallies of the hours before `before` (of every hour, for `None`) and let go
    /// of them. With no `dir` they are let go of all the same.
    fn flush(&mut self, dir: Option<&Path>, before: Option<u64>) {
        let mut over: Vec<(u64, Context)> =
            self.hours.keys().filter(|(h, _)| before.is_none_or(|b| *h < b)).cloned().collect();
        over.sort_by_key(|(h, _)| *h);
        for key in over {
            let Some(counting) = self.hours.remove(&key) else { continue };
            self.changed = true;
            if let Some(dir) = dir {
                self.write(dir, key.0, &key.1, counting.tally());
            }
        }
    }

    /// Append hour `h`'s tally in `context` to its month's file, unless it is of nothing.
    fn write(&mut self, dir: &Path, h: u64, context: &Context, tally: Tally) {
        if tally == Tally::default() {
            return;
        }
        let written = self.append(dir, Month::of(h * HOUR_MS), context, |c| Row::Hour { h, c, tally });
        if written.is_err() {
            // What the file holds is unknown: give every Context its number again.
            self.month = None;
        }
        self.said(written);
    }

    /// Append the row `row` makes of `context`'s number to `month`'s file, after the `ctx` row
    /// that gives it the number if the file has none from this run.
    fn append(&mut self, dir: &Path, month: Month, context: &Context, row: impl FnOnce(u32) -> Row) -> Result<(), String> {
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

    /// The totals `path` now holds are `totals`, written at `at`: count what they have over
    /// what was counted, in the hour `at` is in, under the transcript's Context.
    fn count(&mut self, path: &str, len: u64, at: u64, totals: Option<Totals>) {
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
            .map(|(model, t)| (model.clone(), t.over(base.get(model).unwrap_or(&Spend::default()))))
            .filter(|(_, t)| *t != Spend::default())
            .collect();
        let context = w.ctx.clone();
        w.start = totals.start;
        w.totals = totals.models;
        self.unsaved = true;
        if over.is_empty() {
            return;
        }
        w.at = at.max(w.at);
        if let Some(context) = context {
            let tally = &mut self.counting(at, &context).rest;
            for (model, spend) in &over {
                tally.m.entry(model.clone()).or_default().add(spend);
            }
        }
    }

    /// What of `totals` was counted already: the most this Journal read from the same run of
    /// Claude Code, in any transcript (`/clear` starts another, and the totals carry on);
    /// failing that what this transcript last held, if the totals are those and more (a
    /// resume, carrying them on under another run); else nothing.
    fn counted(&self, path: &str, totals: &Totals) -> Models {
        let sum = |m: &Models| m.values().map(|t| t.out + t.usd).sum::<u64>();
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
                now.out >= t.out && now.usd >= t.usd
            })
        };
        match self.watched.get(path) {
            Some(w) if has_more(&w.totals) => w.totals.clone(),
            _ => Models::new(),
        }
    }

    /// Say a failed write once, not on every row after it.
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
    let models = models
        .iter()
        .map(|(model, u)| {
            let out = u.get("outputTokens").and_then(serde_json::Value::as_u64).unwrap_or(0);
            let usd = u.get("costUSD").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
            (model.clone(), Spend { out, usd: (usd.max(0.0) * 1e6).round() as u64 })
        })
        .collect();
    let start = v.get("startTime").and_then(serde_json::Value::as_u64).unwrap_or(0);
    Some(Totals { start, models })
}

/// The tallies in one month's file, in order; none for a file that is not there.
fn read_file(path: &Path) -> Vec<(u64, Context, Tally)> {
    let Ok(text) = fs::read_to_string(path) else { return Vec::new() };
    let mut contexts: HashMap<u32, Context> = HashMap::new();
    let mut rows = Vec::new();
    for line in text.lines() {
        match serde_json::from_str::<Row>(line) {
            Ok(Row::Ctx { id, context }) => {
                contexts.insert(id, context);
            }
            Ok(Row::Hour { h, c, tally }) => {
                if let Some(context) = contexts.get(&c) {
                    rows.push((h, context.clone(), tally));
                }
            }
            Err(_) => {}
        }
    }
    rows
}

/// Start the thread that ticks every [`TICK`]. A panicking tick is skipped; the thread never
/// exits.
pub fn spawn(journal: Journal) {
    let started = thread::Builder::new().name("journal".into()).spawn(move || loop {
        thread::sleep(TICK);
        if catch_unwind(AssertUnwindSafe(|| journal.tick(now_ms()))).is_err() {
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
    #[cfg(test)]
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
    /// The hour that begins at `DAY`.
    const H: u64 = DAY / HOUR_MS;
    const MIN: u64 = 60_000;
    const SEPTEMBER: &str = "2026-09.jsonl";
    const OPUS: &str = "claude-opus-5-5";

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

    fn lines(tmp: &TempDir, file: &str) -> Vec<String> {
        let text = fs::read_to_string(tmp.path().join("journal").join(file)).unwrap_or_default();
        text.lines().map(str::to_owned).collect()
    }

    /// Each hour read as (hours after `DAY`, branch, seconds worked, seconds waited).
    fn times(hours: &[Hour]) -> Vec<(u64, &str, u64, u64)> {
        hours
            .iter()
            .map(|h| ((h.start - DAY) / HOUR_MS, h.context.br.as_deref().unwrap_or("-"), h.tally.run, h.tally.wait))
            .collect()
    }

    fn day(j: &Journal, now: u64) -> Vec<Hour> {
        j.read_at(DAY, DAY + DAY_MS, now)
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
    fn an_hour_in_a_context_is_one_row_however_much_happened() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        // Two agents on one branch, one's status changing a hundred times.
        for i in 0..100 {
            let at = DAY + i * 30_000;
            let status = if i % 2 == 0 { AgentStatus::Running } else { AgentStatus::NeedsInput };
            j.observe_at(&[agent(1, status, "main"), agent(2, AgentStatus::Running, "main")], at);
            j.step(1, &Step::Prompt, at);
            j.step(1, &Step::Edit { added: 3, removed: 1 }, at);
        }
        j.observe_at(&[agent(1, AgentStatus::Done, "main"), agent(2, AgentStatus::Done, "main")], DAY + 50 * MIN);
        j.tick(DAY + 55 * MIN);
        assert!(lines(&tmp, SEPTEMBER).is_empty(), "nothing until the hour is over");
        j.tick(DAY + 61 * MIN);
        assert_eq!(
            lines(&tmp, SEPTEMBER),
            [
                r#"{"k":"ctx","id":1,"agent":"claude","repo":"/r/.git","name":"r","br":"main"}"#.to_owned(),
                format!(r#"{{"k":"hour","h":{H},"c":1,"run":4500,"wait":1500,"turns":100,"add":300,"del":100}}"#),
            ],
            "25 min and 50 min of work, 25 min of waiting"
        );
        assert!(!tmp.path().join("journal").join(OPEN).exists(), "nothing is left to keep");
        let hours = day(&j, DAY + 62 * MIN);
        assert_eq!(times(&hours), [(0, "main", 4500, 1500)]);
        assert_eq!(hours[0].tally.turns, 100);
        assert_eq!(hours[0].context.repo.as_deref(), Some("/r/.git"));
    }

    #[test]
    fn an_hour_row_stays_small() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], DAY);
        j.step(1, &Step::Prompt, DAY);
        j.step(1, &Step::Edit { added: 1200, removed: 800 }, DAY);
        j.observe_at(&[agent(1, AgentStatus::NeedsInput, "main")], DAY + 40 * MIN);
        j.finish_at(DAY + 59 * MIN);
        let rows = lines(&tmp, SEPTEMBER);
        assert!(rows[1].len() <= 96, "{} bytes: {}", rows[1].len(), rows[1]);
    }

    #[test]
    fn time_is_counted_in_the_hour_it_was_in() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let october = Month::of(DAY).next().start();
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], october - 90 * MIN);
        j.observe_at(&[agent(1, AgentStatus::Done, "main")], october + 20 * MIN);
        j.tick(october + 61 * MIN);
        assert_eq!(lines(&tmp, SEPTEMBER).len(), 3, "the Context and two hours");
        assert_eq!(lines(&tmp, "2026-10.jsonl").len(), 2, "each file names its own Contexts");
        let hours = j.read_at(october - 2 * HOUR_MS, october + HOUR_MS, october + 62 * MIN);
        let runs: Vec<(u64, u64)> = hours.iter().map(|h| (h.start, h.tally.run)).collect();
        assert_eq!(runs, [(october - 2 * HOUR_MS, 1800), (october - HOUR_MS, 3600), (october, 1200)]);
        let one = j.read_at(october - HOUR_MS, october, october + 62 * MIN);
        assert_eq!(one.len(), 1, "the hours that began in what was asked for");
    }

    #[test]
    fn an_idle_agent_and_a_shell_are_not_counted() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let shell = SessionInfo { cwd: Some("/r".into()), ..SessionInfo::empty(1) };
        j.observe_at(&[shell.clone(), agent(2, AgentStatus::Done, "main")], DAY);
        j.step(1, &Step::Prompt, DAY + MIN);
        // An agent leaves: its Session is a shell again.
        j.observe_at(&[agent(3, AgentStatus::Running, "main")], DAY);
        j.observe_at(&[SessionInfo { session_id: 3, ..shell }], DAY + 5 * MIN);
        // A Session ends with its agent at work.
        j.observe_at(&[agent(4, AgentStatus::Running, "main")], DAY);
        j.end_at(4, DAY + 2 * MIN);
        j.end_at(2, DAY + 30 * MIN);
        assert_eq!(times(&day(&j, DAY + 40 * MIN)), [(0, "main", 7 * 60, 0)]);
    }

    #[test]
    fn another_branch_is_another_tally() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], DAY);
        j.observe_at(&[agent(1, AgentStatus::Running, "fix")], DAY + 10 * MIN);
        j.step(1, &Step::Prompt, DAY + 11 * MIN);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], DAY + 30 * MIN);
        j.end_at(1, DAY + 35 * MIN);
        let mut hours = day(&j, DAY + 40 * MIN);
        hours.sort_by_key(|h| h.context.br.clone());
        assert_eq!(times(&hours), [(0, "fix", 1200, 0), (0, "main", 900, 0)]);
        assert_eq!((hours[0].tally.turns, hours[1].tally.turns), (1, 0), "a prompt is counted where it was given");
    }

    #[test]
    fn outside_a_repo_the_cwd_stands_for_it_and_a_remote_hop_has_neither() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let mut info = agent(1, AgentStatus::Running, "main");
        info.git = None;
        j.observe_at(&[info], DAY);
        j.end_at(1, DAY + MIN);
        let mut far = agent(2, AgentStatus::Running, "main");
        far.remote = true;
        j.observe_at(&[far], DAY + HOUR_MS);
        j.end_at(2, DAY + HOUR_MS + MIN);
        j.finish_at(DAY + 2 * HOUR_MS);
        let rows = lines(&tmp, SEPTEMBER);
        assert_eq!(rows[0], r#"{"k":"ctx","id":1,"agent":"claude","cwd":"/r/src"}"#);
        assert_eq!(rows[2], r#"{"k":"ctx","id":2,"agent":"claude","remote":true}"#);
    }

    #[test]
    fn the_next_launch_writes_what_a_crash_left() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], DAY);
        j.step(1, &Step::Prompt, DAY + MIN);
        j.tick(DAY + 10 * MIN);
        assert!(tmp.path().join("journal").join(OPEN).exists());
        j.observe_at(&[agent(1, AgentStatus::NeedsInput, "main")], DAY + 10 * MIN + 30_000);
        drop(j); // the crash: what came after the tick is lost

        let j = journal(&tmp);
        assert!(!tmp.path().join("journal").join(OPEN).exists());
        assert_eq!(times(&day(&j, DAY + 20 * MIN)), [(0, "main", 600, 0)]);
        // The same hour again in this run: the rows add up, and the file still reads though
        // this run numbers its Contexts from 1 again.
        j.observe_at(&[agent(1, AgentStatus::Running, "fix"), agent(2, AgentStatus::Running, "main")], DAY + 20 * MIN);
        j.finish_at(DAY + 25 * MIN);
        let mut hours = day(&j, DAY + 30 * MIN);
        hours.sort_by_key(|h| h.context.br.clone());
        assert_eq!(times(&hours), [(0, "fix", 300, 0), (0, "main", 900, 0)]);
        assert_eq!(hours[1].tally.turns, 1);
    }

    #[test]
    fn stopping_writes_this_hour_too_and_counts_no_more() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], DAY);
        j.tick(DAY + MIN);
        j.finish_at(DAY + 2 * MIN);
        assert!(!tmp.path().join("journal").join(OPEN).exists());
        assert_eq!(lines(&tmp, SEPTEMBER).len(), 2);
        // The Sessions die as the Host kills them: not an agent's doing.
        j.observe_at(&[agent(1, AgentStatus::NeedsInput, "main")], DAY + 3 * MIN);
        j.step(1, &Step::Prompt, DAY + 3 * MIN);
        j.end_at(1, DAY + 4 * MIN);
        j.tick(DAY + 5 * MIN);
        j.finish_at(DAY + 6 * MIN);
        let hours = day(&j, DAY + 7 * MIN);
        assert_eq!(times(&hours), [(0, "main", 120, 0)]);
        assert_eq!(hours[0].tally.turns, 0);
        assert_eq!(lines(&tmp, SEPTEMBER).len(), 2);
    }

    #[test]
    fn a_line_cut_short_costs_only_itself() {
        let tmp = TempDir::new("journal");
        let dir = tmp.path().join("journal");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(SEPTEMBER), r#"{"k":"hour","h":4973"#).unwrap();
        let j = journal(&tmp);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], DAY);
        j.finish_at(DAY + MIN);
        assert_eq!(times(&day(&j, DAY + 2 * MIN)), [(0, "main", 60, 0)]);
    }

    #[test]
    fn without_a_data_dir_nothing_is_kept_and_nothing_breaks() {
        let j = Journal::open(None);
        j.observe_at(&[agent(1, AgentStatus::Running, "main")], DAY);
        j.step(1, &Step::Transcript("/nowhere/1.jsonl".into()), DAY);
        j.tick(DAY + MIN);
        assert_eq!(times(&day(&j, DAY + 2 * MIN)), [(0, "main", 120, 0)], "this hour is still known");
        j.tick(DAY + HOUR_MS + MIN);
        assert_eq!(times(&day(&j, DAY + HOUR_MS + MIN)), [(1, "main", 60, 0)], "an hour that is over is let go");
        j.finish_at(DAY + 2 * HOUR_MS);
        assert!(day(&j, DAY + 2 * HOUR_MS).is_empty());
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

    const TALK: &str = "{\"type\":\"assistant\",\"message\":{\"usage\":{\"output_tokens\":5}}}\n";

    /// A transcript in `tmp`, watched for Session 1, an agent on `main`.
    fn watched(tmp: &TempDir, j: &Journal, name: &str, at: u64) -> PathBuf {
        let file = tmp.path().join(name);
        j.observe_at(&[agent(1, AgentStatus::Done, "main")], at);
        j.step(1, &Step::Transcript(file.to_string_lossy().into_owned()), at);
        file
    }

    /// What Opus was used for in each hour read: (hours after `DAY`, output tokens, usd).
    fn spent(j: &Journal, now: u64) -> Vec<(u64, u64, u64)> {
        let hours = day(j, now);
        hours.iter().filter_map(|h| Some(((h.start - DAY) / HOUR_MS, h.tally.m.get(OPUS)?.out, h.tally.m[OPUS].usd))).collect()
    }

    #[test]
    fn what_the_totals_moved_by_is_counted_in_the_hour_they_were_written() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let file = watched(&tmp, &j, "1.jsonl", DAY);
        add(&file, TALK);
        j.tick(DAY + MIN);
        assert!(day(&j, DAY + MIN).is_empty(), "no totals yet, and an idle agent");
        add(&file, &cost_state(7, &[(OPUS, 1000, 0.5), ("claude-haiku-4-5", 20, 0.001)]));
        j.tick(DAY + 2 * MIN);
        add(&file, &cost_state(7, &[(OPUS, 1400, 0.7), ("claude-haiku-4-5", 20, 0.001)]));
        j.tick(DAY + 3 * MIN);
        j.tick(DAY + 4 * MIN);
        add(&file, &cost_state(7, &[(OPUS, 1500, 0.75), ("claude-haiku-4-5", 20, 0.001)]));
        j.tick(DAY + HOUR_MS + MIN);
        assert_eq!(spent(&j, DAY + HOUR_MS + MIN), [(0, 1400, 700_000), (1, 100, 50_000)]);
        let rows = lines(&tmp, SEPTEMBER);
        assert_eq!(
            rows[1],
            format!(r#"{{"k":"hour","h":{H},"c":1,"m":{{"claude-haiku-4-5":{{"out":20,"usd":1000}},"claude-opus-5-5":{{"out":1400,"usd":700000}}}}}}"#)
        );
        assert_eq!(rows.len(), 2, "the second hour is not over");
    }

    #[test]
    fn totals_are_found_however_much_was_written_after_them() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let file = watched(&tmp, &j, "1.jsonl", DAY);
        add(&file, TALK);
        j.tick(DAY + MIN);
        // Between two looks: the totals, then more than the first look would read.
        add(&file, &cost_state(7, &[(OPUS, 1000, 0.5)]));
        add(&file, &TALK.repeat(2 * TAIL as usize / TALK.len()));
        j.tick(DAY + 2 * MIN);
        assert_eq!(spent(&j, DAY + 3 * MIN), [(0, 1000, 500_000)]);
    }

    #[test]
    fn what_a_transcript_held_before_it_was_looked_at_is_not_counted() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        add(&tmp.path().join("1.jsonl"), &cost_state(7, &[(OPUS, 1000, 0.5)]));
        let file = watched(&tmp, &j, "1.jsonl", DAY);
        j.tick(DAY + MIN);
        assert!(spent(&j, DAY + MIN).is_empty());
        add(&file, &cost_state(7, &[(OPUS, 1300, 0.6)]));
        j.tick(DAY + 2 * MIN);
        assert_eq!(spent(&j, DAY + 2 * MIN), [(0, 300, 100_000)]);
    }

    #[test]
    fn totals_carry_on_over_a_clear_and_a_resume_and_start_again_in_a_new_run() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let one = watched(&tmp, &j, "1.jsonl", DAY);
        add(&one, &cost_state(7, &[(OPUS, 1000, 0.5)]));
        j.tick(DAY + MIN);
        // `/clear`: another transcript, the same run, its totals carrying on.
        let two = watched(&tmp, &j, "2.jsonl", DAY + HOUR_MS);
        add(&two, &cost_state(7, &[(OPUS, 1200, 0.6)]));
        j.tick(DAY + HOUR_MS + MIN);
        // A resume: another run, carrying the totals on.
        add(&two, &cost_state(8, &[(OPUS, 1250, 0.7)]));
        j.tick(DAY + 2 * HOUR_MS + MIN);
        // Another run that counts from nothing, twice read.
        add(&two, &cost_state(9, &[(OPUS, 40, 0.02)]));
        j.tick(DAY + 3 * HOUR_MS + MIN);
        add(&two, &cost_state(9, &[(OPUS, 90, 0.03)]));
        j.tick(DAY + 4 * HOUR_MS + MIN);
        assert_eq!(
            spent(&j, DAY + 4 * HOUR_MS + MIN),
            [(0, 1000, 500_000), (1, 200, 100_000), (2, 50, 100_000), (3, 40, 20_000), (4, 50, 10_000)]
        );
    }

    #[test]
    fn totals_written_while_the_host_was_down_are_counted_by_the_next_launch() {
        let tmp = TempDir::new("journal");
        let j = journal(&tmp);
        let file = watched(&tmp, &j, "1.jsonl", DAY);
        add(&file, &cost_state(7, &[(OPUS, 1000, 0.5)]));
        j.tick(DAY + MIN);
        j.finish_at(DAY + 2 * MIN);
        // Claude Code writes its totals as the Host kills it.
        add(&file, &cost_state(7, &[(OPUS, 1100, 0.55)]));

        let j = journal(&tmp);
        j.tick(DAY + HOUR_MS + MIN);
        let hours = day(&j, DAY + HOUR_MS + MIN);
        assert_eq!(spent(&j, DAY + HOUR_MS + MIN), [(0, 1000, 500_000), (1, 100, 50_000)]);
        assert_eq!(hours[1].context.br.as_deref(), Some("main"), "under the Context it was last in");
        // Long after, in no Session: let go.
        j.tick(DAY + HOUR_MS + MIN + METER_IDLE_MS + 1);
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
}
