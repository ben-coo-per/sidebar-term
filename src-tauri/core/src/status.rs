//! Agent status: Running / Needs input / Done for an Agent session, derived on the Host from
//! the Session's facts (`SessionInfo.agent`) and what the Host read in its output
//! (`remote/tap.rs`: the OSC 0 / 2 title, BELs, when output last arrived). The rules are the
//! table in docs/architecture.md "Agent status" (sources: docs/research/agent-detection.md);
//! they were `computeAgentStatus` in `src/lib/agentStatus.ts` until #28 moved them here, so a
//! headless Host and the Mac app give one answer. Pure: the clock is passed in.

use crate::model::{AgentKind, AgentStatus, SessionInfo};
use crate::remote::tap::Marks;
use std::time::{Duration, Instant};

/// How long Claude Code counts as Running after the last output. Only a fallback, for when its
/// title carries no state prefix (`CLAUDE_CODE_DISABLE_TERMINAL_TITLE`, tmux, older versions).
pub const CLAUDE_RUNNING_WINDOW: Duration = Duration::from_millis(3000);

/// Claude Code's title prefix while busy: alternates ◐ / ◑ (frozen on one frame when unfocused).
const CLAUDE_BUSY_PREFIXES: [char; 2] = ['◐', '◑'];
/// Claude Code's title prefix while idle or waiting on a prompt.
const CLAUDE_IDLE_PREFIX: char = '✳';

/// A braille spinner frame (U+2800..U+28FF), per Codex's `tui.terminal_title` "activity" item.
fn starts_with_braille(title: &str) -> bool {
    title
        .chars()
        .next()
        .is_some_and(|c| ('\u{2800}'..='\u{28FF}').contains(&c))
}

/// Running / Needs input / Done for `agent`, from the latest title, when output last arrived
/// and when the last BEL rang. `None` when there is no agent.
pub fn compute(
    agent: Option<AgentKind>,
    title: Option<&str>,
    now: Instant,
    last_output_at: Option<Instant>,
    last_bell_at: Option<Instant>,
) -> Option<AgentStatus> {
    let agent = agent?;
    let title = title.unwrap_or("");
    Some(match agent {
        AgentKind::Codex => {
            if starts_with_braille(title) {
                AgentStatus::Running
            } else if title.contains("Action Required") {
                AgentStatus::NeedsInput
            } else {
                AgentStatus::Done
            }
        }
        AgentKind::Gemini => {
            if title.starts_with('✦') {
                AgentStatus::Running
            } else if title.starts_with('✋') {
                AgentStatus::NeedsInput
            } else {
                AgentStatus::Done // "◇" (Ready), or no marker yet
            }
        }
        // Claude Code: its title prefix says busy or not. Without one, fall back to output
        // activity (which keystroke echo and redraws also trip, hence only a fallback).
        AgentKind::Claude => {
            if title.starts_with(CLAUDE_BUSY_PREFIXES) {
                return Some(AgentStatus::Running);
            }
            let idle = title.starts_with(CLAUDE_IDLE_PREFIX);
            let active_recently =
                last_output_at.is_some_and(|t| now.duration_since(t) < CLAUDE_RUNNING_WINDOW);
            if !idle && active_recently {
                AgentStatus::Running
            } else if last_bell_at.is_some_and(|bell| last_output_at.is_none_or(|out| bell >= out)) {
                // A BEL that landed after the last output (not superseded by fresh output)
                // means the agent is still waiting on that prompt.
                AgentStatus::NeedsInput
            } else {
                AgentStatus::Done
            }
        }
    })
}

/// Fill `info`'s `title`, `bells` and `status` from what the Host read in the Session's output
/// (`None` when it read nothing yet).
pub fn apply(info: &mut SessionInfo, marks: Option<&Marks>, now: Instant) {
    let (title, bells, out, bell) = match marks {
        Some(m) => (m.title.clone(), m.bells, m.last_output_at, m.last_bell_at),
        None => (None, 0, None, None),
    };
    info.status = compute(info.agent, title.as_deref(), now, out, bell);
    info.title = title;
    info.bells = bells;
}

#[cfg(test)]
mod tests {
    //! The cases `src/lib/agentStatus.test.ts` had, one for one.

    use super::*;
    use std::sync::LazyLock;

    static BASE: LazyLock<Instant> = LazyLock::new(Instant::now);

    fn at(ms: u64) -> Instant {
        *BASE + Duration::from_millis(ms)
    }

    fn status(
        agent: AgentKind,
        title: &str,
        now: u64,
        out: Option<u64>,
        bell: Option<u64>,
    ) -> AgentStatus {
        compute(Some(agent), Some(title), at(now), out.map(at), bell.map(at)).unwrap()
    }

    #[test]
    fn no_agent_no_status() {
        assert_eq!(compute(None, Some("⠋ x"), at(0), None, None), None);
    }

    #[test]
    fn codex_follows_its_title() {
        use AgentKind::Codex;
        assert_eq!(status(Codex, "⠋ jack", 0, None, None), AgentStatus::Running);
        assert_eq!(status(Codex, "[ ! ] Action Required", 0, None, None), AgentStatus::NeedsInput);
        assert_eq!(status(Codex, "jack", 0, None, None), AgentStatus::Done);
        assert_eq!(status(Codex, "", 0, None, None), AgentStatus::Done);
    }

    #[test]
    fn gemini_follows_its_title() {
        use AgentKind::Gemini;
        assert_eq!(status(Gemini, "✦ Working… (jack)", 0, None, None), AgentStatus::Running);
        assert_eq!(status(Gemini, "✋ Action Required (jack)", 0, None, None), AgentStatus::NeedsInput);
        assert_eq!(status(Gemini, "◇ Ready (jack)", 0, None, None), AgentStatus::Done);
        assert_eq!(status(Gemini, "", 0, None, None), AgentStatus::Done);
    }

    #[test]
    fn claude_without_a_title_prefix_falls_back_to_output_and_the_bell() {
        use AgentKind::Claude;
        let window = CLAUDE_RUNNING_WINDOW.as_millis() as u64;
        assert_eq!(status(Claude, "", 5000, Some(5000 - (window - 1)), None), AgentStatus::Running);
        assert_eq!(status(Claude, "", 10_000, Some(10_000 - window), None), AgentStatus::Done);
        assert_eq!(status(Claude, "", 10_000, Some(5000), Some(5500)), AgentStatus::NeedsInput);
        assert_eq!(status(Claude, "", 10_000, Some(9999), Some(5500)), AgentStatus::Running, "fresh output beats a stale bell");
        assert_eq!(status(Claude, "", 0, None, None), AgentStatus::Done);
        assert_eq!(status(Claude, "", 10_000, None, Some(5500)), AgentStatus::NeedsInput, "a bell with no output since");
    }

    #[test]
    fn claude_title_prefixes_win() {
        use AgentKind::Claude;
        for title in ["◐ Fix the badge", "◑ Fix the badge"] {
            assert_eq!(status(Claude, title, 60_000, Some(0), None), AgentStatus::Running);
        }
        assert_eq!(status(Claude, "✳ Fix the badge", 1000, Some(999), None), AgentStatus::Done, "idle prefix, even while output flows");
        assert_eq!(status(Claude, "✳ Fix the badge", 10_000, Some(5000), Some(5500)), AgentStatus::NeedsInput);
    }

    #[test]
    fn apply_fills_the_info_from_the_marks() {
        let mut info = SessionInfo::empty(1);
        info.agent = Some(AgentKind::Codex);
        apply(&mut info, None, at(0));
        assert_eq!((info.title.as_deref(), info.bells, info.status), (None, 0, Some(AgentStatus::Done)));
        let marks = Marks {
            title: Some("⠋ jack".into()),
            bells: 2,
            last_bell_at: Some(at(1)),
            last_output_at: Some(at(2)),
        };
        apply(&mut info, Some(&marks), at(3));
        assert_eq!(info.title.as_deref(), Some("⠋ jack"));
        assert_eq!(info.bells, 2);
        assert_eq!(info.status, Some(AgentStatus::Running));
        info.agent = None;
        apply(&mut info, Some(&marks), at(3));
        assert_eq!(info.status, None, "no agent: no status, the title stays a fact");
        assert_eq!(info.title.as_deref(), Some("⠋ jack"));
    }
}
