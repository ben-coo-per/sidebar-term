//! Resume: what each Session is running, kept in `resume.json` in the app data dir so that after
//! the app closes with work still running (a crash, a quit, `pnpm app:install`) the next launch
//! can offer to start it again in the same Tabs: the Resume banner. See docs/architecture.md
//! "Resume"; `detect::resume` works out each entry.
//!
//! The file holds two lists of `ResumeEntry`, each keyed by the key the webview gave the Session
//! at spawn (its Tab id):
//! - `running`: this run's Sessions. A thread rewrites it whenever it changes (checked every
//!   second), and the app records it a last time as it exits, before killing the shells; nothing
//!   is written after that, so the dying shells do not empty it. A crash leaves the last check's.
//! - `leftover`: what earlier runs left running that the webview has neither resumed nor
//!   dismissed. At launch, and when the webview reloads, `running` moves here, each entry
//!   replacing an older one for the same key.

use crate::layout;
use crate::model::{ProbeTarget, ResumeEntry};
use serde::{Deserialize, Serialize};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

/// Time between checks of what each Session is running.
const TICK: Duration = Duration::from_secs(1);

/// Resume state (Tauri state), mirrored to `resume.json`.
pub struct Resume {
    /// `None` when the app data dir is unavailable: Resume then lasts one run.
    path: Option<PathBuf>,
    state: Mutex<State>,
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct State {
    running: Vec<ResumeEntry>,
    leftover: Vec<ResumeEntry>,
    /// Set once the app is exiting: `running` holds its final value.
    #[serde(skip)]
    frozen: bool,
}

impl State {
    /// The run is over: what it was running becomes leftover.
    fn end_run(&mut self) {
        for entry in std::mem::take(&mut self.running) {
            self.leftover.retain(|e| e.key != entry.key);
            self.leftover.push(entry);
        }
    }
}

impl Resume {
    /// Load the file at `path`; whatever the last run was running becomes leftover.
    pub fn open(path: Option<PathBuf>) -> Self {
        let mut state = path.as_deref().and_then(read).unwrap_or_default();
        state.end_run();
        let resume = Self {
            path,
            state: Mutex::new(state),
        };
        resume.save(&lock(&resume.state));
        resume
    }

    /// What each Session is running now. Written only when it changed, and never once frozen.
    pub fn record(&self, now: Vec<ResumeEntry>) {
        let mut s = lock(&self.state);
        if s.frozen || s.running == now {
            return;
        }
        s.running = now;
        self.save(&s);
    }

    /// The webview is reloading and its Sessions are about to be killed: what they run (`now`)
    /// becomes leftover, for the new page to offer.
    pub fn end_run(&self, now: Vec<ResumeEntry>) {
        let mut s = lock(&self.state);
        s.running = now;
        s.end_run();
        self.save(&s);
    }

    /// The app is exiting: record `now` a last time, then stop recording.
    pub fn finish(&self, now: Vec<ResumeEntry>) {
        let mut s = lock(&self.state);
        s.running = now;
        s.frozen = true;
        self.save(&s);
    }

    /// What earlier runs left running, not yet resumed or dismissed.
    pub fn leftover(&self) -> Vec<ResumeEntry> {
        lock(&self.state).leftover.clone()
    }

    /// Drop the leftover entries with these keys: resumed, dismissed, or their Tab is gone.
    pub fn forget(&self, keys: &[String]) {
        let mut s = lock(&self.state);
        let before = s.leftover.len();
        s.leftover.retain(|e| !keys.contains(&e.key));
        if s.leftover.len() != before {
            self.save(&s);
        }
    }

    fn save(&self, state: &State) {
        if let Some(path) = &self.path {
            if let Err(e) = layout::write(path, state) {
                eprintln!("resume: writing {} failed: {e}", path.display());
            }
        }
    }
}

/// The Resume entry of every keyed Session that is running something resumable.
pub fn entries(targets: &[(String, ProbeTarget)]) -> Vec<ResumeEntry> {
    targets
        .iter()
        .filter_map(|(key, target)| crate::detect::resume::entry(key, target))
        .collect()
}

/// Start the thread that runs `tick` every second (lib.rs: `Resume::record` of `entries`).
/// A panicking tick is skipped; the thread never exits.
pub fn spawn(tick: impl Fn() + Send + 'static) {
    let started = thread::Builder::new()
        .name("resume-recorder".into())
        .spawn(move || loop {
            if catch_unwind(AssertUnwindSafe(&tick)).is_err() {
                eprintln!("resume: recording panicked; skipping this tick");
            }
            thread::sleep(TICK);
        });
    if let Err(e) = started {
        eprintln!("resume: could not start thread: {e}");
    }
}

fn read(path: &Path) -> Option<State> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes)
        .inspect_err(|e| {
            eprintln!(
                "resume: {} unreadable ({e}); starting fresh",
                path.display()
            )
        })
        .ok()
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::testutil::TempDir;
    use crate::model::ResumeKind;

    fn e(key: &str, line: &str) -> ResumeEntry {
        ResumeEntry {
            key: key.into(),
            kind: ResumeKind::Command,
            line: line.into(),
            cwd: None,
        }
    }

    fn on_disk(path: &Path) -> State {
        read(path).expect("resume.json")
    }

    #[test]
    fn a_run_left_running_becomes_leftover_at_the_next_launch() {
        let dir = TempDir::new("resume-store");
        let path = dir.path().join("resume.json");

        let first = Resume::open(Some(path.clone()));
        assert!(first.leftover().is_empty());
        first.record(vec![e("a", "npm run dev"), e("b", "claude --resume x")]);
        assert_eq!(on_disk(&path).running.len(), 2);
        first.record(vec![e("a", "npm run dev")]);
        assert_eq!(on_disk(&path).running, [e("a", "npm run dev")]);
        drop(first); // a crash: nothing more is written

        let second = Resume::open(Some(path.clone()));
        assert_eq!(second.leftover(), [e("a", "npm run dev")]);
        assert!(on_disk(&path).running.is_empty());
    }

    #[test]
    fn exiting_records_a_last_time_then_ignores_the_dying_shells() {
        let dir = TempDir::new("resume-finish");
        let path = dir.path().join("resume.json");
        let r = Resume::open(Some(path.clone()));
        r.record(vec![e("a", "old")]);
        r.finish(vec![e("a", "uv run app.py")]);
        r.record(Vec::new());
        assert_eq!(on_disk(&path).running, [e("a", "uv run app.py")]);
        assert_eq!(
            Resume::open(Some(path)).leftover(),
            [e("a", "uv run app.py")]
        );
    }

    #[test]
    fn leftover_lasts_until_forgotten_and_newer_runs_replace_it_per_key() {
        let dir = TempDir::new("resume-leftover");
        let path = dir.path().join("resume.json");
        let r = Resume::open(Some(path.clone()));
        r.finish(vec![e("a", "one"), e("b", "two")]);

        // Launched, but quit again before acting on the banner: nothing is lost.
        let r = Resume::open(Some(path.clone()));
        r.finish(vec![e("b", "three")]);
        let r = Resume::open(Some(path.clone()));
        assert_eq!(r.leftover(), [e("a", "one"), e("b", "three")]);

        r.forget(&["a".into(), "gone".into()]);
        assert_eq!(r.leftover(), [e("b", "three")]);
        assert_eq!(on_disk(&path).leftover, [e("b", "three")]);
    }

    #[test]
    fn a_webview_reload_ends_the_run() {
        let r = Resume::open(None);
        r.record(vec![e("a", "npm run dev")]);
        r.end_run(vec![e("a", "npm run dev"), e("b", "make")]);
        assert_eq!(r.leftover(), [e("a", "npm run dev"), e("b", "make")]);
        r.record(Vec::new());
        assert_eq!(r.leftover().len(), 2);
    }

    #[test]
    fn a_corrupt_file_starts_fresh() {
        let dir = TempDir::new("resume-corrupt");
        let path = dir.path().join("resume.json");
        std::fs::write(&path, "{ nope").unwrap();
        assert!(Resume::open(Some(path.clone())).leftover().is_empty());
        assert_eq!(on_disk(&path), State::default());
    }
}
