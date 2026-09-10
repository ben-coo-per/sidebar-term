//! Caffeinate: keeps this Mac awake while on, for the Tray's Caffeinate button.
//!
//! Runs `/usr/bin/caffeinate -d -i -w <app pid>` as a hidden child of the app, in no Session, so no
//! Terminal shows it. `-d` keeps the display awake, `-i` stops idle sleep, and `-w` ends it when the
//! app exits, a crash included. Off at launch: the state is not persisted.
//!
//! While on, a thread checks the run every second. If it ended without being turned off (someone
//! ran `killall caffeinate`), Caffeinate is off and `on_ended` runs (lib.rs: emit `caffeinate`).

use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

const CAFFEINATE: &str = "/usr/bin/caffeinate";
/// Time between checks that a run is still going.
const CHECK_EVERY: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Run {
    /// The running `caffeinate`; None while off.
    child: Option<Child>,
    /// Bumped on every start, so a checking thread knows whether its run is still the current one.
    generation: u64,
}

type OnEnded = Arc<dyn Fn() + Send + Sync>;

/// Handle to the `caffeinate` run (Tauri state). Starts off.
pub struct Caffeinate {
    run: Arc<Mutex<Run>>,
    on_ended: OnEnded,
}

impl Caffeinate {
    /// `on_ended` runs when a run ends without being turned off.
    pub fn new(on_ended: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            run: Arc::default(),
            on_ended: Arc::new(on_ended),
        }
    }

    pub fn is_on(&self) -> bool {
        alive(&mut lock(&self.run))
    }

    /// Turn Caffeinate on or off; returns whether it is on now.
    pub fn set(&self, on: bool) -> Result<bool, String> {
        let mut run = lock(&self.run);
        if on == alive(&mut run) {
            return Ok(on);
        }
        if !on {
            if let Some(mut child) = run.child.take() {
                // Dying releases its power assertions; `wait` reaps it.
                let _ = child.kill();
                let _ = child.wait();
            }
            return Ok(false);
        }
        let child = Command::new(CAFFEINATE)
            .args(["-d", "-i", "-w", &std::process::id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("caffeinate: {e}"))?;
        run.child = Some(child);
        run.generation += 1;
        let (shared, on_ended, generation) =
            (self.run.clone(), self.on_ended.clone(), run.generation);
        thread::spawn(move || check(shared, on_ended, generation));
        Ok(true)
    }
}

/// Every `CHECK_EVERY` while run `generation` is the current one, check it is still going; if it
/// ended on its own, call `on_ended`.
fn check(run: Arc<Mutex<Run>>, on_ended: OnEnded, generation: u64) {
    loop {
        thread::sleep(CHECK_EVERY);
        let mut run = lock(&run);
        // Turned off, restarted, or already found dead by `is_on`/`set`, whose caller has the state.
        if run.generation != generation || run.child.is_none() {
            return;
        }
        if !alive(&mut run) {
            drop(run);
            on_ended();
            return;
        }
    }
}

/// True while the run is going; forgets (and reaps) one that has ended.
fn alive(run: &mut Run) -> bool {
    let going = matches!(run.child.as_mut().map(Child::try_wait), Some(Ok(None)));
    if !going {
        run.child = None;
    }
    going
}

fn lock(run: &Mutex<Run>) -> MutexGuard<'_, Run> {
    run.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn run_pid(c: &Caffeinate) -> u32 {
        lock(&c.run).child.as_ref().expect("a run").id()
    }

    /// The command line of process `pid`, or None when there is none.
    fn command(pid: u32) -> Option<String> {
        let out = Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    #[test]
    fn runs_caffeinate_until_turned_off() {
        let ended = Arc::new(AtomicBool::new(false));
        let flag = ended.clone();
        let c = Caffeinate::new(move || flag.store(true, Ordering::SeqCst));
        assert!(!c.is_on());

        assert_eq!(c.set(true), Ok(true));
        assert!(c.is_on());
        let pid = run_pid(&c);
        let expected = format!("{CAFFEINATE} -d -i -w {}", std::process::id());
        assert_eq!(command(pid).as_deref(), Some(expected.as_str()));
        assert_eq!(c.set(true), Ok(true), "turning on again keeps the same run");
        assert_eq!(run_pid(&c), pid);

        assert_eq!(c.set(false), Ok(false));
        assert!(!c.is_on());
        assert_eq!(command(pid), None, "turning off ends the run");
        thread::sleep(CHECK_EVERY * 2);
        assert!(
            !ended.load(Ordering::SeqCst),
            "turning off is not ending on its own"
        );
    }

    #[test]
    fn a_run_killed_elsewhere_turns_off() {
        let ended = Arc::new(AtomicBool::new(false));
        let flag = ended.clone();
        let c = Caffeinate::new(move || flag.store(true, Ordering::SeqCst));
        assert_eq!(c.set(true), Ok(true));
        let pid = run_pid(&c);

        Command::new("/bin/kill")
            .arg(pid.to_string())
            .status()
            .unwrap();
        thread::sleep(CHECK_EVERY * 2 + Duration::from_millis(500));
        assert!(ended.load(Ordering::SeqCst), "on_ended runs");
        assert!(!c.is_on());

        assert_eq!(c.set(true), Ok(true), "and it can be turned on again");
        assert_eq!(c.set(false), Ok(false));
    }
}
