//! Where a Session's output goes before a client with a Terminal attaches to it. The Host spawns
//! a Session as it makes its Tab (at launch, or on `tab_new`), before any Terminal exists for it;
//! the Mac webview then attaches (`session_attach`) and installs its sink. Until then the output
//! is held here, in order, and handed over first thing on attach, under the same lock the reader
//! thread delivers through, so nothing is lost or repeated between the two. A Host nobody
//! attaches to (the daemon: phones read the Session's tap instead) keeps at most
//! [`HOLD_MAX`] bytes per Session, cut on a line like the tap's ring.
//!
//! Attaching again replaces the sink; output already delivered to the old one is not replayed
//! (the Mac webview never re-attaches: a reload respawns its Sessions, see `layout/mod.rs`).

use crate::host::OutputSink;
use crate::model::SessionId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Bytes held per Session before its first attach.
pub const HOLD_MAX: usize = 256 * 1024;
const HOLD_KEEP: usize = HOLD_MAX * 3 / 4;
const LINE_SEARCH: usize = 4 * 1024;

/// One Session's outlet: the sink once attached, the held output before.
#[derive(Default)]
pub struct Outlet {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    sink: Option<OutputSink>,
    held: Vec<u8>,
    /// A sink was installed at some point: a client has (or had) a Terminal on this Session.
    attached: bool,
}

impl Outlet {
    /// The reader thread's delivery: to the sink, or held.
    pub fn deliver(&self, bytes: Vec<u8>) {
        let mut s = lock(&self.state);
        match s.sink.as_mut() {
            Some(sink) => sink(bytes),
            None => {
                s.held.extend_from_slice(&bytes);
                trim(&mut s.held);
            }
        }
    }

    /// Install `sink`, after handing it what was held.
    pub fn attach(&self, mut sink: OutputSink) {
        let mut s = lock(&self.state);
        let held = std::mem::take(&mut s.held);
        if !held.is_empty() {
            sink(held);
        }
        s.sink = Some(sink);
        s.attached = true;
    }

    /// Whether a client ever attached (and so holds a Terminal that a reload would orphan).
    pub fn was_attached(&self) -> bool {
        lock(&self.state).attached
    }
}

/// The outlets of every live Session, by id.
#[derive(Default)]
pub struct Outlets {
    by_id: Mutex<HashMap<SessionId, Arc<Outlet>>>,
}

impl Outlets {
    pub fn insert(&self, id: SessionId, outlet: Arc<Outlet>) {
        lock(&self.by_id).insert(id, outlet);
    }

    pub fn remove(&self, id: SessionId) {
        lock(&self.by_id).remove(&id);
    }

    pub fn get(&self, id: SessionId) -> Option<Arc<Outlet>> {
        lock(&self.by_id).get(&id).cloned()
    }

    /// Attach `sink` to Session `id`'s output. `Err` for a Session that is gone.
    pub fn attach(&self, id: SessionId, sink: OutputSink) -> Result<(), String> {
        let outlet = self.get(id).ok_or_else(|| format!("no Session {id}"))?;
        outlet.attach(sink);
        Ok(())
    }

    /// The Sessions a client has attached to.
    pub fn attached(&self) -> Vec<SessionId> {
        lock(&self.by_id)
            .iter()
            .filter(|(_, o)| o.was_attached())
            .map(|(id, _)| *id)
            .collect()
    }
}

/// Keep what is held under `HOLD_MAX`, cutting to `HOLD_KEEP` on a line break when one is near.
fn trim(held: &mut Vec<u8>) {
    if held.len() <= HOLD_MAX {
        return;
    }
    let mut cut = held.len() - HOLD_KEEP;
    let search_end = (cut + LINE_SEARCH).min(held.len());
    if let Some(nl) = held[cut..search_end].iter().position(|&b| b == b'\n') {
        cut += nl + 1;
    }
    held.drain(..cut);
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording() -> (OutputSink, Arc<Mutex<Vec<Vec<u8>>>>) {
        let seen: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
        let into = seen.clone();
        (Box::new(move |bytes| lock(&into).push(bytes)), seen)
    }

    #[test]
    fn output_before_attach_is_handed_over_first_then_streams_live() {
        let outlet = Outlet::default();
        outlet.deliver(b"one ".to_vec());
        outlet.deliver(b"two".to_vec());
        assert!(!outlet.was_attached());
        let (sink, seen) = recording();
        outlet.attach(sink);
        assert!(outlet.was_attached());
        outlet.deliver(b" three".to_vec());
        assert_eq!(*lock(&seen), [b"one two".to_vec(), b" three".to_vec()]);
    }

    #[test]
    fn nothing_held_means_nothing_replayed() {
        let outlet = Outlet::default();
        let (sink, seen) = recording();
        outlet.attach(sink);
        assert!(lock(&seen).is_empty());
        outlet.deliver(b"x".to_vec());
        assert_eq!(*lock(&seen), [b"x".to_vec()]);
    }

    #[test]
    fn what_is_held_is_bounded_and_cut_on_a_line() {
        let outlet = Outlet::default();
        let line = b"0123456789abcdef\n".to_vec();
        let mut total = 0;
        while total <= HOLD_MAX {
            outlet.deliver(line.clone());
            total += line.len();
        }
        let (sink, seen) = recording();
        outlet.attach(sink);
        let held = &lock(&seen)[0];
        assert!(held.len() <= HOLD_KEEP);
        assert!(held.len() > HOLD_KEEP - LINE_SEARCH);
        assert_eq!(&held[..17], &line[..], "the replay starts on a line");
    }

    #[test]
    fn outlets_are_looked_up_by_session_and_know_which_were_attached() {
        let outlets = Outlets::default();
        outlets.insert(1, Arc::default());
        outlets.insert(2, Arc::default());
        let (sink, _) = recording();
        outlets.attach(1, sink).unwrap();
        assert!(outlets.attach(9, Box::new(|_| {})).is_err());
        assert_eq!(outlets.attached(), [1]);
        outlets.remove(1);
        assert!(outlets.get(1).is_none());
        assert!(outlets.attached().is_empty());
    }
}
