//! Idle close: a Host closes the Tabs whose Sessions have sat idle for a long time (the daemon,
//! `sidebar-termd --close-idle-after <hours>`), so week-old agents at their prompt stop holding
//! hundreds of MB each. Before a Tab goes, the daemon logs its Resume entry (`claude --resume
//! <id>` and where), so the conversation can still be picked up by hand.
//!
//! Idle means nothing is going on and nothing is waiting on anyone:
//! - an Agent session at its prompt (Agent status Done, no question pending), or
//! - a plain shell at its prompt (the shell holds the tty: nothing in the foreground),
//!
//! with no input from any client and no output for the whole time. A Session that is working,
//! waits on the user, or runs anything else in the foreground (`npm run dev`, an ssh hop, vim)
//! is never idle. The Host's active Tab and its last Tab are never closed. The clock starts when
//! the Host first sees the Session idle, so a restart gives every Session the full time again.

use crate::model::{AgentStatus, SessionId, SessionInfo};
use std::collections::HashMap;

/// Whether a Session's facts say it is idle (the module doc).
pub fn is_idle(info: &SessionInfo) -> bool {
    if info.pending.is_some() {
        return false;
    }
    match info.agent {
        Some(_) => info.status == Some(AgentStatus::Done),
        None => info.shell_is_foreground && info.status.is_none(),
    }
}

/// One Tab with a live Session, as the closer sees it.
pub struct Candidate<'a> {
    pub tab_id: &'a str,
    pub info: &'a SessionInfo,
    /// The Session's last input or output, epoch ms.
    pub last_io: u64,
}

/// When each Session was first seen idle, across ticks.
#[derive(Default)]
pub struct IdleCloser {
    since: HashMap<SessionId, u64>,
}

impl IdleCloser {
    /// The Tabs to close now: idle for at least `after_ms` (counted from when they were first
    /// seen idle or their last I/O, whichever is later), never `active`, and never so many that
    /// no Tab would be left (`tab_count` is every Tab of the Host).
    pub fn tick(
        &mut self,
        now: u64,
        after_ms: u64,
        candidates: &[Candidate],
        active: Option<&str>,
        tab_count: usize,
    ) -> Vec<String> {
        self.since
            .retain(|id, _| candidates.iter().any(|c| c.info.session_id == *id));
        let mut close = Vec::new();
        for c in candidates {
            let id = c.info.session_id;
            if !is_idle(c.info) {
                self.since.remove(&id);
                continue;
            }
            let since = *self.since.entry(id).or_insert(now);
            let idle_from = since.max(c.last_io);
            if now.saturating_sub(idle_from) >= after_ms && Some(c.tab_id) != active {
                close.push(c.tab_id.to_owned());
            }
        }
        // Keep one Tab whatever happens: a Host with none would only make a new one.
        let keep = tab_count.saturating_sub(1);
        close.truncate(keep);
        close
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentKind, Pending, PendingKind};

    const H: u64 = 3_600_000;

    fn shell(id: SessionId) -> SessionInfo {
        SessionInfo::empty(id)
    }

    fn agent(id: SessionId, status: Option<AgentStatus>) -> SessionInfo {
        SessionInfo {
            agent: Some(AgentKind::Claude),
            shell_is_foreground: false,
            foreground: Some("claude".into()),
            status,
            ..SessionInfo::empty(id)
        }
    }

    #[test]
    fn idle_is_at_a_prompt_with_nothing_waiting() {
        assert!(is_idle(&shell(1)));
        assert!(is_idle(&agent(1, Some(AgentStatus::Done))));
        assert!(!is_idle(&agent(1, Some(AgentStatus::Running))));
        assert!(!is_idle(&agent(1, Some(AgentStatus::NeedsInput))));
        let dev_server = SessionInfo {
            shell_is_foreground: false,
            foreground: Some("node".into()),
            ..SessionInfo::empty(1)
        };
        assert!(!is_idle(&dev_server));
        let asking = SessionInfo {
            pending: Some(Pending {
                id: 1,
                kind: PendingKind::Permission,
                text: String::new(),
                detail: Vec::new(),
                options: Vec::new(),
                since: 0,
            }),
            ..agent(1, Some(AgentStatus::Done))
        };
        assert!(!is_idle(&asking));
    }

    #[test]
    fn a_tab_closes_after_the_whole_time_idle_and_never_the_active_or_last_one() {
        let mut closer = IdleCloser::default();
        let (a, b, c) = (shell(1), agent(2, Some(AgentStatus::Done)), agent(3, Some(AgentStatus::Running)));
        let tabs = |io: u64| {
            [
                Candidate { tab_id: "a", info: &a, last_io: io },
                Candidate { tab_id: "b", info: &b, last_io: io },
                Candidate { tab_id: "c", info: &c, last_io: io },
            ]
        };
        // First seen idle at 0: the clock starts then, whatever the last I/O was.
        assert!(closer.tick(0, 24 * H, &tabs(0), None, 3).is_empty());
        assert!(closer.tick(23 * H, 24 * H, &tabs(0), None, 3).is_empty());
        assert_eq!(closer.tick(24 * H, 24 * H, &tabs(0), None, 3), ["a", "b"]);
        // Output an hour ago restarts the count; the active Tab is spared.
        assert_eq!(closer.tick(25 * H, 24 * H, &tabs(0), Some("a"), 3), ["b"]);
        assert!(closer.tick(25 * H, 24 * H, &tabs(24 * H), None, 3).is_empty());
        // Never the last Tab.
        assert_eq!(closer.tick(26 * H, 24 * H, &tabs(0), None, 2), ["a"]);
        assert!(closer.tick(26 * H, 24 * H, &tabs(0), None, 1).is_empty());
    }

    #[test]
    fn busy_again_starts_the_clock_over() {
        let mut closer = IdleCloser::default();
        let idle = agent(1, Some(AgentStatus::Done));
        let busy = agent(1, Some(AgentStatus::Running));
        let one = |info| [Candidate { tab_id: "a", info, last_io: 0 }];
        closer.tick(0, 10, &one(&idle), None, 2);
        closer.tick(5, 10, &one(&busy), None, 2);
        assert!(closer.tick(12, 10, &one(&idle), None, 2).is_empty(), "idle again only since 12");
        assert_eq!(closer.tick(22, 10, &one(&idle), None, 2), ["a"]);
        // A Session that is gone is forgotten.
        closer.tick(23, 10, &[], None, 2);
        assert!(closer.since.is_empty());
    }
}
