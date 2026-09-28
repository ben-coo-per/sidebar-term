//! The progress file protocol: what the app's reporters and wrappers append, one JSON object
//! per line, to `$SIDEBAR_TERM_PROGRESS_DIR/<writer pid>.jsonl`, and how several files add up
//! to one Suite's numbers. Pure: no I/O here except [`Tail`], which only reads.
//!
//! ```text
//! {"t":1790365752402,"ev":"start","runner":"vitest","pid":91300}
//! {"t":1790365752867,"ev":"total","total":24}          once known (collection, onBegin, --list)
//! {"t":1790365753183,"ev":"case","status":"passed"}    one per test: passed | failed | skipped
//! {"t":1790365754191,"ev":"end","status":"failed","passed":23,"failed":1}
//! ```
//!
//! A runner may produce several files: one per test binary under `cargo test`, one per package
//! under `go test`, one per test process under nextest. Totals add up across files, and a file
//! that reports no cases but ends counts its total as done when it ends, so a multi-binary
//! `cargo test` moves binary by binary. Malformed lines are skipped, never fatal.

use crate::model::SuiteOutcome;
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// One parsed line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Start,
    Total(u32),
    Case(CaseStatus),
    End {
        status: Option<SuiteOutcome>,
        passed: Option<u32>,
        failed: Option<u32>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseStatus {
    Passed,
    Failed,
    Skipped,
}

/// Parse one line; `None` for anything that is not a well-formed protocol line.
pub fn parse_line(line: &str) -> Option<Line> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let count = |key: &str| v.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
    match v.get("ev")?.as_str()? {
        "start" => Some(Line::Start),
        "total" => Some(Line::Total(count("total")?)),
        "case" => Some(Line::Case(match v.get("status").and_then(Value::as_str)? {
            "passed" => CaseStatus::Passed,
            "failed" | "timedOut" | "timedout" | "interrupted" | "error" => CaseStatus::Failed,
            "skipped" | "pending" | "todo" | "xfailed" => CaseStatus::Skipped,
            _ => return None,
        })),
        "end" => Some(Line::End {
            status: match v.get("status").and_then(Value::as_str) {
                Some("passed") => Some(SuiteOutcome::Passed),
                Some("failed" | "timedout" | "interrupted") => Some(SuiteOutcome::Failed),
                _ => None,
            },
            passed: count("passed"),
            failed: count("failed"),
        }),
        _ => None,
    }
}

/// What one progress file has said so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileProgress {
    pub started: bool,
    pub total: Option<u32>,
    pub cases: u32,
    pub cases_failed: u32,
    pub end: Option<(Option<SuiteOutcome>, Option<u32>, Option<u32>)>,
}

impl FileProgress {
    pub fn apply(&mut self, line: Line) {
        match line {
            Line::Start => self.started = true,
            Line::Total(n) => self.total = Some(self.total.unwrap_or(0) + n),
            Line::Case(status) => {
                self.cases += 1;
                if status == CaseStatus::Failed {
                    self.cases_failed += 1;
                }
            }
            Line::End {
                status,
                passed,
                failed,
            } => self.end = Some((status, passed, failed)),
        }
    }

    /// Feed raw lines (malformed ones are skipped).
    pub fn apply_lines<'a>(&mut self, lines: impl IntoIterator<Item = &'a str>) {
        for line in lines {
            if let Some(l) = parse_line(line) {
                self.apply(l);
            }
        }
    }

    fn done(&self) -> u32 {
        if self.cases > 0 {
            return self.cases;
        }
        match self.end {
            Some((_, passed, failed)) if passed.is_some() || failed.is_some() => {
                passed.unwrap_or(0) + failed.unwrap_or(0)
            }
            Some(_) => self.total.unwrap_or(0),
            None => 0,
        }
    }

    fn failed(&self) -> u32 {
        let reported = self.end.and_then(|(_, _, f)| f).unwrap_or(0);
        self.cases_failed.max(reported)
    }
}

/// One Suite's numbers, summed over its files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// A reporter has spoken: the numbers are live.
    pub streaming: bool,
    pub done: u32,
    pub total: Option<u32>,
    pub failed: u32,
    /// Some file reported a failed end.
    pub any_failed: bool,
    /// Every file that started has ended (and there was at least one).
    pub all_ended: bool,
}

impl Progress {
    /// The outcome to report once the runner has exited.
    pub fn outcome_at_exit(&self) -> Option<SuiteOutcome> {
        if self.any_failed || self.failed > 0 {
            Some(SuiteOutcome::Failed)
        } else if self.all_ended {
            Some(SuiteOutcome::Passed)
        } else {
            None
        }
    }
}

pub fn merge<'a>(files: impl IntoIterator<Item = &'a FileProgress>) -> Progress {
    let mut p = Progress::default();
    let mut n = 0;
    let mut ended = 0;
    for f in files {
        if !f.started && f.total.is_none() && f.cases == 0 && f.end.is_none() {
            continue;
        }
        n += 1;
        p.streaming = true;
        p.done += f.done();
        p.failed += f.failed();
        if let Some(t) = f.total {
            p.total = Some(p.total.unwrap_or(0) + t);
        }
        if let Some((status, _, _)) = f.end {
            ended += 1;
            if status == Some(SuiteOutcome::Failed) {
                p.any_failed = true;
            }
        }
    }
    p.all_ended = n > 0 && ended == n;
    if let Some(t) = p.total {
        p.total = Some(t.max(p.done));
    }
    p
}

/// Reads the new lines of an append-only file across calls.
#[derive(Debug, Default)]
pub struct Tail {
    offset: u64,
    partial: Vec<u8>,
}

impl Tail {
    /// The complete lines appended since the last call. A trailing partial line waits for its
    /// newline. Unreadable files read as nothing.
    pub fn read_new(&mut self, path: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let Ok(mut f) = File::open(path) else {
            return out;
        };
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return out;
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() || buf.is_empty() {
            return out;
        }
        self.offset += buf.len() as u64;
        self.partial.extend_from_slice(&buf);
        while let Some(nl) = self.partial.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=nl).collect();
            out.push(String::from_utf8_lossy(&line[..nl]).into_owned());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_protocol_and_skips_garbage() {
        assert_eq!(parse_line(r#"{"t":1,"ev":"start","runner":"vitest","pid":9}"#), Some(Line::Start));
        assert_eq!(parse_line(r#"{"ev":"total","total":24}"#), Some(Line::Total(24)));
        assert_eq!(parse_line(r#"{"ev":"case","status":"passed"}"#), Some(Line::Case(CaseStatus::Passed)));
        assert_eq!(parse_line(r#"{"ev":"case","status":"timedOut","name":"x"}"#), Some(Line::Case(CaseStatus::Failed)));
        assert_eq!(parse_line(r#"{"ev":"case","status":"pending"}"#), Some(Line::Case(CaseStatus::Skipped)));
        assert_eq!(
            parse_line(r#"{"ev":"end","status":"failed","passed":23,"failed":1}"#),
            Some(Line::End { status: Some(SuiteOutcome::Failed), passed: Some(23), failed: Some(1) })
        );
        assert_eq!(parse_line(r#"{"ev":"end"}"#), Some(Line::End { status: None, passed: None, failed: None }));
        for bad in ["", "not json", "{}", r#"{"ev":"total"}"#, r#"{"ev":"case"}"#, r#"{"ev":"case","status":"meh"}"#, r#"{"ev":"total","total":-1}"#, r#"{"ev":"nope"}"#] {
            assert_eq!(parse_line(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn one_reporter_streams_cases() {
        let mut f = FileProgress::default();
        f.apply_lines([
            r#"{"ev":"start","runner":"pytest","pid":1}"#,
            r#"{"ev":"total","total":4}"#,
            "garbage",
            r#"{"ev":"case","status":"passed"}"#,
            r#"{"ev":"case","status":"failed"}"#,
        ]);
        let p = merge([&f]);
        assert_eq!(p, Progress { streaming: true, done: 2, total: Some(4), failed: 1, any_failed: false, all_ended: false });
        assert_eq!(p.outcome_at_exit(), Some(SuiteOutcome::Failed));

        f.apply_lines([r#"{"ev":"case","status":"skipped"}"#, r#"{"ev":"case","status":"passed"}"#, r#"{"ev":"end","status":"failed","passed":2,"failed":1}"#]);
        let p = merge([&f]);
        assert_eq!((p.done, p.failed, p.all_ended), (4, 1, true));
        assert_eq!(p.outcome_at_exit(), Some(SuiteOutcome::Failed));
    }

    #[test]
    fn cargo_binaries_add_up_and_count_as_done_when_they_end() {
        let mut lib = FileProgress::default();
        lib.apply_lines([r#"{"ev":"start","runner":"cargo","pid":1}"#, r#"{"ev":"total","total":12}"#]);
        let mut integration = FileProgress::default();
        integration.apply_lines([r#"{"ev":"start","runner":"cargo","pid":2}"#, r#"{"ev":"total","total":3}"#]);
        let p = merge([&lib, &integration]);
        assert_eq!((p.done, p.total, p.failed), (0, Some(15), 0));
        assert_eq!(p.outcome_at_exit(), None, "nothing ended: the runner's exit says nothing");

        lib.apply_lines([r#"{"ev":"end","status":"passed"}"#]);
        let p = merge([&lib, &integration]);
        assert_eq!((p.done, p.total), (12, Some(15)));
        assert!(!p.all_ended);

        integration.apply_lines([r#"{"ev":"end","status":"failed"}"#]);
        let p = merge([&lib, &integration]);
        assert_eq!((p.done, p.total, p.all_ended, p.any_failed), (15, Some(15), true, true));
        assert_eq!(p.outcome_at_exit(), Some(SuiteOutcome::Failed));

        let mut ok = FileProgress::default();
        ok.apply_lines([r#"{"ev":"total","total":2}"#, r#"{"ev":"end","status":"passed"}"#]);
        assert_eq!(merge([&ok]).outcome_at_exit(), Some(SuiteOutcome::Passed));
    }

    #[test]
    fn nextest_per_test_files_are_cases() {
        let mut list = FileProgress::default();
        list.apply_lines([r#"{"ev":"start","runner":"cargo","pid":1}"#, r#"{"ev":"total","total":3}"#]);
        let mut t1 = FileProgress::default();
        t1.apply_lines([r#"{"ev":"case","status":"passed"}"#]);
        let mut t2 = FileProgress::default();
        t2.apply_lines([r#"{"ev":"case","status":"failed"}"#]);
        let p = merge([&list, &t1, &t2]);
        assert_eq!((p.done, p.total, p.failed), (2, Some(3), 1));
        assert!(merge([&FileProgress::default()]) == Progress::default(), "an empty file is not a stream");
    }

    #[test]
    fn total_never_reads_below_done() {
        let mut f = FileProgress::default();
        f.apply_lines([r#"{"ev":"total","total":1}"#, r#"{"ev":"case","status":"passed"}"#, r#"{"ev":"case","status":"passed"}"#]);
        assert_eq!(merge([&f]).total, Some(2));
    }

    #[test]
    fn tail_reads_only_new_complete_lines() {
        let dir = crate::detect::testutil::TempDir::new("tail");
        let path = dir.path().join("1.jsonl");
        let mut tail = Tail::default();
        assert!(tail.read_new(&path).is_empty(), "missing file reads as nothing");
        std::fs::write(&path, "a\nb\npart").unwrap();
        assert_eq!(tail.read_new(&path), ["a", "b"]);
        assert!(tail.read_new(&path).is_empty());
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        use std::io::Write;
        f.write_all(b"ial\nc\n").unwrap();
        assert_eq!(tail.read_new(&path), ["partial", "c"]);
    }
}
