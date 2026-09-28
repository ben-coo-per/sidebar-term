//! The Journal: what every Agent session on this Host was doing, where and for how long, kept
//! on disk for Rewind (docs/architecture.md "Journal", ADR 0004). The status history in
//! `agents/` goes back a few hours and lives in memory; this is the part that stays.
//!
//! **Spans, not samples.** A row is written when something ends, never on a timer: one span per
//! stretch of one Agent status in one [`Context`] (the Tab, the agent, the repo, Worktree and
//! branch). Time per repo, per branch or per hour of the day is a sum over spans.
//!
//! **Files.** `journal/<year>-<month>.jsonl` in the Host's data dir, one JSON object per line,
//! appended. Months are UTC and only say which file a span is in: a span that crosses the end
//! of a month is written as two, so every span lies within its file's month. Two kinds of row:
//!
//! ```text
//! {"k":"ctx","id":1,"tab":"t3","agent":"claude","repo":"/r/.git","name":"r","br":"main"}
//! {"k":"span","c":1,"s":"running","a":1790000000000,"b":1790000042000}
//! ```
//!
//! A `ctx` row gives a Context a number, and the spans after it name it by that number, so a
//! repo's path is written once and not on every span. The number holds for the rows that
//! follow it, until a later `ctx` row gives the same number to something else (each run of the
//! Host starts again at 1), so a file is read from the top. A line that does not parse is
//! skipped: a crash can cut the last one short.
//!
//! **A crash.** A span is written when it ends, so the ones still open would go with the
//! process. Every [`TICK`] they are written to `journal/open.json` with the time; the next
//! launch ends them at that time and writes them as spans.

use crate::agents::now_ms;
use crate::model::{AgentKind, AgentStatus, SessionId, SessionInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
const DAY_MS: u64 = 86_400_000;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn is_false(b: &bool) -> bool {
    !*b
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
}

/// `open.json`.
#[derive(Serialize, Deserialize)]
struct OpenFile {
    /// When this was written: where the spans end, should the Host not live to end them.
    alive_at: u64,
    spans: Vec<OpenSpan>,
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
            }
            let _ = fs::remove_file(&path);
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

    /// Session `id` ended, and its span with it.
    pub fn end(&self, id: SessionId) {
        self.end_at(id, now_ms());
    }

    fn end_at(&self, id: SessionId, now: u64) {
        let mut state = lock(&self.inner.state);
        if !state.finished {
            self.close(&mut state, id, now);
        }
    }

    /// The Host is stopping: every open span ends now, and nothing is written after.
    pub fn finish(&self) {
        self.finish_at(now_ms());
    }

    fn finish_at(&self, now: u64) {
        let mut state = lock(&self.inner.state);
        let ids: Vec<SessionId> = state.open.keys().copied().collect();
        for id in ids {
            self.close(&mut state, id, now);
        }
        state.finished = true;
        if let Some(dir) = &self.inner.dir {
            let _ = fs::remove_file(dir.join(OPEN));
        }
    }

    /// Write the open spans to `open.json` with the time, for a launch after a crash.
    fn checkpoint(&self, now: u64) {
        let Some(dir) = &self.inner.dir else { return };
        let mut state = lock(&self.inner.state);
        if state.finished {
            return;
        }
        let path = dir.join(OPEN);
        if state.open.is_empty() {
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
                spans.extend(read_file(&dir.join(month.file())));
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
            let written = self.append(dir, month, open, start, stop);
            if written.is_err() {
                // What the file holds is unknown: give every Context its number again.
                self.month = None;
            }
            self.said(written);
            start = stop;
        }
    }

    fn append(&mut self, dir: &Path, month: Month, open: &Open, start: u64, end: u64) -> Result<(), String> {
        let path = dir.join(month.file());
        let mut lines = String::new();
        if self.month != Some(month) {
            self.month = Some(month);
            self.ids.clear();
            if ends_mid_line(&path) {
                lines.push('\n');
            }
        }
        let known = self.ids.get(&open.context).copied();
        let id = known.unwrap_or(self.ids.len() as u32 + 1);
        let mut rows = Vec::new();
        if known.is_none() {
            rows.push(Row::Ctx { id, context: open.context.clone() });
        }
        rows.push(Row::Span { c: id, s: open.status, a: start, b: end });
        for row in &rows {
            lines.push_str(&serde_json::to_string(row).map_err(|e| e.to_string())?);
            lines.push('\n');
        }
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        // One write for the Context and its span: O_APPEND keeps them together.
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(lines.as_bytes()))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        self.ids.insert(open.context.clone(), id);
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

/// The spans in one month's file, in order; none for a file that is not there.
fn read_file(path: &Path) -> Vec<Span> {
    let Ok(text) = fs::read_to_string(path) else { return Vec::new() };
    let mut contexts: HashMap<u32, Context> = HashMap::new();
    let mut spans = Vec::new();
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
            Err(_) => {}
        }
    }
    spans
}

/// Start the thread that writes the open spans down every [`TICK`]. A panicking tick is
/// skipped; the thread never exits.
pub fn spawn(journal: Journal) {
    let started = thread::Builder::new().name("journal".into()).spawn(move || loop {
        thread::sleep(TICK);
        if catch_unwind(AssertUnwindSafe(|| journal.checkpoint(now_ms()))).is_err() {
            eprintln!("journal: writing the open spans panicked; skipping this tick");
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
