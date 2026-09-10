//! Facts about a Session derived from the OS and the filesystem, never from the pty stream.
//! OWNER: detection agent. See docs/research/agent-detection.md and docs/research/cwd-git.md.

pub mod git;
pub mod process;

use crate::model::{ProbeTarget, SessionInfo};

/// Compute the current `SessionInfo` for one Session. Pure w.r.t. app state: reads only
/// libproc and the filesystem. Must be fast (target: well under 1 ms per call when the
/// git dir is cached by the OS) and must never panic.
pub fn probe(target: &ProbeTarget) -> SessionInfo {
    SessionInfo::empty(target.session_id)
}
