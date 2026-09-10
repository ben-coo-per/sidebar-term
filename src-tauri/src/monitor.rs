//! Background poller: every tick, probe each live Session and emit `EVENT_SESSION_INFO`
//! for Sessions whose `SessionInfo` changed since the last emit. OWNER: detection agent.
//!
//! Polling is the v1 decision: macOS has no event for "foreground process group changed" or
//! "cwd changed" (docs/research/agent-detection.md, docs/research/cwd-git.md).

use crate::detect;
use crate::model::{ProbeTarget, SessionId, SessionInfo, EVENT_SESSION_INFO};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Time between ticks.
const TICK: Duration = Duration::from_millis(500);

/// Start the monitor thread. `targets` is called once per tick to get live Sessions.
/// Emit a `SessionInfo` for a Session the first time it is seen and whenever it changes.
/// Forget cached state for Sessions that disappear.
///
/// The thread never exits: a panic in `targets` skips the tick, a panic in one probe skips
/// that Session for the tick. (Only with `panic = "unwind"`; the release profile aborts on
/// panic, so the probe path is written not to panic at all.)
pub fn spawn<F>(app: AppHandle, targets: F)
where
    F: Fn() -> Vec<ProbeTarget> + Send + 'static,
{
    let started = thread::Builder::new()
        .name("session-monitor".into())
        .spawn(move || {
            let mut tracker = Tracker::default();
            loop {
                if let Ok(live) = catch_unwind(AssertUnwindSafe(&targets)) {
                    for info in tracker.tick(&live, detect::probe) {
                        if let Err(e) = app.emit(EVENT_SESSION_INFO, &info) {
                            eprintln!(
                                "session-monitor: emit for session {} failed: {e}",
                                info.session_id
                            );
                        }
                    }
                } else {
                    eprintln!("session-monitor: listing sessions panicked; skipping this tick");
                }
                thread::sleep(TICK);
            }
        });
    if let Err(e) = started {
        eprintln!("session-monitor: could not start thread: {e}");
    }
}

/// Last emitted `SessionInfo` per live Session.
#[derive(Default)]
struct Tracker {
    last: HashMap<SessionId, SessionInfo>,
}

impl Tracker {
    /// Probe every target and return the infos to emit: new Sessions and changed ones.
    /// Sessions absent from `targets` are forgotten, so a reused id would emit afresh.
    fn tick(
        &mut self,
        targets: &[ProbeTarget],
        probe: impl Fn(&ProbeTarget) -> SessionInfo,
    ) -> Vec<SessionInfo> {
        self.last
            .retain(|id, _| targets.iter().any(|t| t.session_id == *id));
        let mut changed = Vec::new();
        for t in targets {
            let Ok(info) = catch_unwind(AssertUnwindSafe(|| probe(t))) else {
                eprintln!(
                    "session-monitor: probe of session {} panicked; skipping it this tick",
                    t.session_id
                );
                continue;
            };
            if self.last.get(&t.session_id) != Some(&info) {
                self.last.insert(t.session_id, info.clone());
                changed.push(info);
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn t(id: SessionId) -> ProbeTarget {
        ProbeTarget {
            session_id: id,
            shell_pid: 100 + id as i32,
            fg_pgid: None,
        }
    }

    fn info(id: SessionId, fg: &str) -> SessionInfo {
        SessionInfo {
            foreground: Some(fg.into()),
            ..SessionInfo::empty(id)
        }
    }

    fn ids(v: &[SessionInfo]) -> Vec<SessionId> {
        v.iter().map(|i| i.session_id).collect()
    }

    #[test]
    fn emits_first_sight_and_changes_only() {
        let mut tr = Tracker::default();
        let fg = Cell::new("zsh");
        let probe = |p: &ProbeTarget| info(p.session_id, fg.get());

        assert_eq!(ids(&tr.tick(&[t(1), t(2)], probe)), [1, 2]);
        assert!(tr.tick(&[t(1), t(2)], probe).is_empty());

        fg.set("claude");
        let out = tr.tick(&[t(1), t(2)], probe);
        assert_eq!(ids(&out), [1, 2]);
        assert_eq!(out[0].foreground.as_deref(), Some("claude"));
        assert!(tr.tick(&[t(1), t(2)], probe).is_empty());

        // A new Session appears alongside unchanged ones.
        assert_eq!(ids(&tr.tick(&[t(1), t(2), t(3)], probe)), [3]);
    }

    #[test]
    fn forgets_sessions_that_disappear() {
        let mut tr = Tracker::default();
        let probe = |p: &ProbeTarget| info(p.session_id, "zsh");
        tr.tick(&[t(1), t(2)], probe);
        assert!(tr.tick(&[t(1)], probe).is_empty());
        assert_eq!(tr.last.len(), 1);
        // Seen again later: first sight again.
        assert_eq!(ids(&tr.tick(&[t(1), t(2)], probe)), [2]);
        assert!(tr.tick(&[], probe).is_empty());
        assert!(tr.last.is_empty());
    }

    #[test]
    fn a_panicking_probe_does_not_stop_the_others() {
        let mut tr = Tracker::default();
        let bad = Cell::new(true);
        let probe = |p: &ProbeTarget| {
            if p.session_id == 2 && bad.get() {
                panic!("probe blew up");
            }
            info(p.session_id, "zsh")
        };
        assert_eq!(ids(&tr.tick(&[t(1), t(2), t(3)], probe)), [1, 3]);
        bad.set(false);
        assert_eq!(ids(&tr.tick(&[t(1), t(2), t(3)], probe)), [2]);
    }

    #[test]
    fn real_probe_through_the_tracker() {
        // The tracker driven by the real `detect::probe` on this test process as the "shell".
        let mut tr = Tracker::default();
        let me = ProbeTarget {
            session_id: 9,
            shell_pid: std::process::id() as i32,
            fg_pgid: None,
        };
        let out = tr.tick(&[me], detect::probe);
        assert_eq!(ids(&out), [9]);
        assert!(out[0].cwd.is_some());
        assert!(tr.tick(&[me], detect::probe).is_empty());
    }
}
