//! Duration history: the last [`KEEP`] runs of each suite, keyed by repo, runner and command
//! signature, kept in `history.json` in the app data dir. Gives the ETA when no reporter streams
//! progress (docs/research/test-progress.md "ETA without progress").

use crate::layout;
use crate::model::SuiteOutcome;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Runs kept per suite.
pub const KEEP: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Passed,
    Failed,
}

impl From<SuiteOutcome> for Outcome {
    fn from(o: SuiteOutcome) -> Self {
        match o {
            SuiteOutcome::Passed => Outcome::Passed,
            SuiteOutcome::Failed => Outcome::Failed,
        }
    }
}

/// One recorded run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    /// Epoch ms.
    pub started_at: u64,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
}

/// Which suite a set of runs belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Key {
    /// The repo's `common_dir`, or the cwd outside a repo.
    pub repo: String,
    pub runner: String,
    /// `detect::runner::signature`.
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    #[serde(flatten)]
    key: Key,
    /// Oldest first.
    runs: Vec<Run>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct File {
    version: u32,
    suites: Vec<Entry>,
}

/// What the history says about a suite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Estimate {
    pub median_ms: u64,
    pub longest_ms: u64,
    pub runs: u32,
}

/// Median and longest of the given durations (`None` when empty). An even count takes the mean
/// of the two middle values.
pub fn estimate(durations: &[u64]) -> Option<Estimate> {
    if durations.is_empty() {
        return None;
    }
    let mut sorted = durations.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    let median_ms = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2
    };
    Some(Estimate {
        median_ms,
        longest_ms: sorted[n - 1],
        runs: n as u32,
    })
}

/// The history on disk. `None` path: kept for this run only.
pub struct History {
    path: Option<PathBuf>,
    file: File,
}

impl History {
    pub fn open(path: Option<PathBuf>) -> Self {
        let file = path.as_deref().and_then(read).unwrap_or_default();
        Self { path, file }
    }

    pub fn estimate(&self, key: &Key) -> Option<Estimate> {
        let entry = self.file.suites.iter().find(|e| &e.key == key)?;
        let durations: Vec<u64> = entry.runs.iter().map(|r| r.duration_ms).collect();
        estimate(&durations)
    }

    /// Append a run, keeping the last [`KEEP`], and save.
    pub fn record(&mut self, key: &Key, run: Run) {
        let entry = match self.file.suites.iter_mut().find(|e| &e.key == key) {
            Some(e) => e,
            None => {
                self.file.suites.push(Entry {
                    key: key.clone(),
                    runs: Vec::new(),
                });
                self.file.suites.last_mut().expect("just pushed")
            }
        };
        entry.runs.push(run);
        if entry.runs.len() > KEEP {
            let drop = entry.runs.len() - KEEP;
            entry.runs.drain(..drop);
        }
        self.file.version = 1;
        if let Some(path) = &self.path {
            if let Err(e) = layout::write(path, &self.file) {
                eprintln!("suite history: writing {} failed: {e}", path.display());
            }
        }
    }
}

fn read(path: &Path) -> Option<File> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes)
        .inspect_err(|e| eprintln!("suite history: {} unreadable ({e}); starting fresh", path.display()))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::testutil::TempDir;

    fn key(sig: &str) -> Key {
        Key {
            repo: "/r/.git".into(),
            runner: "pytest".into(),
            signature: sig.into(),
        }
    }

    fn run(duration_ms: u64) -> Run {
        Run {
            started_at: 1,
            duration_ms,
            outcome: None,
        }
    }

    #[test]
    fn median_and_longest() {
        assert_eq!(estimate(&[]), None);
        assert_eq!(estimate(&[30]), Some(Estimate { median_ms: 30, longest_ms: 30, runs: 1 }));
        assert_eq!(estimate(&[50, 10, 30]), Some(Estimate { median_ms: 30, longest_ms: 50, runs: 3 }));
        assert_eq!(estimate(&[10, 40, 20, 30]), Some(Estimate { median_ms: 25, longest_ms: 40, runs: 4 }));
    }

    #[test]
    fn keeps_the_last_ten_per_suite_and_survives_a_relaunch() {
        let dir = TempDir::new("history");
        let path = dir.path().join("history.json");
        let mut h = History::open(Some(path.clone()));
        assert_eq!(h.estimate(&key("pytest")), None);
        for i in 1..=12 {
            h.record(&key("pytest"), run(i * 1000));
        }
        h.record(&key("pytest tests/unit"), run(5));
        let est = h.estimate(&key("pytest")).unwrap();
        assert_eq!(est.runs, 10);
        assert_eq!(est.longest_ms, 12_000);
        assert_eq!(est.median_ms, 7_500, "runs 3..=12 remain");
        assert_eq!(h.estimate(&key("pytest tests/unit")).unwrap().runs, 1);

        let again = History::open(Some(path.clone()));
        assert_eq!(again.estimate(&key("pytest")), Some(est));
        let other = Key { repo: "/other/.git".into(), ..key("pytest") };
        assert_eq!(again.estimate(&other), None);

        std::fs::write(&path, "{ not json").unwrap();
        let fresh = History::open(Some(path));
        assert_eq!(fresh.estimate(&key("pytest")), None);
    }

    #[test]
    fn works_without_a_file() {
        let mut h = History::open(None);
        h.record(&key("x"), run(7));
        assert_eq!(h.estimate(&key("x")).unwrap().median_ms, 7);
    }
}
