//! Session lifecycle: spawn a login shell on a pty, stream its output to the webview,
//! accept input, resize, pause/resume for flow control, and tear down.
//! OWNER: pty agent. See docs/research/pty.md and docs/architecture.md.
//!
//! Layout:
//! - [`PtyHost`] is the pty core: the registry of live Sessions, their threads, signals and
//!   reaping. It knows nothing about Tauri; it reports output and exit through closures, so the
//!   tests at the bottom drive real ptys without an `AppHandle`.
//! - [`SessionManager`] is the thin Tauri glue: output goes to a `Channel` as
//!   `InvokeResponseBody::Raw`, exit goes out as the `session-exit` event.
//!
//! Per Session there are two threads:
//! - a **reader** that polls a dup of the master, coalesces macOS's ~1 KiB reads into chunks of up
//!   to [`CHUNK_MAX`], parks while paused (kernel back-pressure then stalls the child), and when the
//!   shell is gone reaps it, drops the Session from the registry and reports the exit;
//! - a **writer** that drains an input queue with `write_all`. Tauri runs sync commands on the main
//!   thread, and a write to the master blocks while the tty input queue is full (a big paste into a
//!   child that is not reading), so `write()` only enqueues and never blocks the UI.
//!
//! Locks: the registry lock is only ever held for map operations and the `tcgetpgrp` ioctl in
//! `probe_targets` (never across I/O, never while taking another lock). The child is signalled and
//! reaped only under its own `proc` lock, and never signalled once reaped, so a recycled pid is
//! never hit.

use crate::model::{ProbeTarget, SessionExit, SessionId, EVENT_SESSION_EXIT};
use portable_pty::{
    native_pty_system, Child, CommandBuilder, ExitStatus, MasterPty, PtyPair, PtySize,
};
use std::collections::{BTreeMap, HashMap};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter};

/// Largest chunk handed to the output callback (one IPC message).
const CHUNK_MAX: usize = 64 * 1024;
/// Per-`read()` buffer. macOS returns at most ~1 KiB per read on a pty master anyway.
const READ_BUF: usize = 16 * 1024;
/// A read at least this big suggests a burst is in flight, so linger briefly for more.
const BIG_READ: usize = 512;
/// How long to wait for the next piece of a burst before flushing.
const LINGER_STEP: Duration = Duration::from_millis(1);
/// Upper bound on how long one chunk may be held back while coalescing.
const LINGER_BUDGET: Duration = Duration::from_millis(4);
/// Reader housekeeping period while idle or paused: reap check, kill escalation.
const TICK: Duration = Duration::from_millis(500);
/// After `kill`, how long the shell gets to exit on SIGHUP before SIGKILL.
const KILL_GRACE: Duration = Duration::from_secs(2);
/// After EOF on the master, grace before SIGHUP / SIGKILL of a shell that is still alive
/// (it closed its tty but kept running).
const EOF_HUP_AFTER: Duration = Duration::from_millis(500);
const EOF_KILL_AFTER: Duration = Duration::from_secs(3);
/// Cap on output drained after the shell has been reaped without the master reaching EOF.
const EXIT_DRAIN_MAX: usize = 1 << 20;

/// Live pty threads (readers + writers). Lets tests prove nothing leaks.
static LIVE_THREADS: AtomicUsize = AtomicUsize::new(0);

struct ThreadGuard;

impl ThreadGuard {
    fn enter() -> Self {
        LIVE_THREADS.fetch_add(1, Ordering::SeqCst);
        ThreadGuard
    }
}

impl Drop for ThreadGuard {
    fn drop(&mut self) {
        LIVE_THREADS.fetch_sub(1, Ordering::SeqCst);
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------------------------
// Tauri glue
// ---------------------------------------------------------------------------------------------

/// Registry of live Sessions. Managed as Tauri state (`app.manage(SessionManager::default())`).
#[derive(Default)]
pub struct SessionManager {
    host: PtyHost,
    /// The webview's key for each Session that has one (its Tab id), for Resume. Keys of Sessions
    /// that have exited are dropped by `keyed_targets`.
    resume_keys: Mutex<HashMap<SessionId, String>>,
}

impl SessionManager {
    /// Spawn the user's login shell (`$SHELL -l`, fallback `/bin/zsh`) on a new pty of `cols`x`rows`
    /// in `cwd` (fallback `$HOME`). Output bytes go to `on_data` as `InvokeResponseBody::Raw`.
    /// When the shell exits, emit `EVENT_SESSION_EXIT` with `SessionExit` via `app`,
    /// then drop the Session from the registry. `resume_key` is the webview's key for the Session
    /// in Resume entries (`resume.rs`); a Session without one is never resumed.
    pub fn spawn(
        &self,
        app: AppHandle,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
        resume_key: Option<String>,
        on_data: Channel<InvokeResponseBody>,
    ) -> Result<SessionId, String> {
        let spec = SpawnSpec::login_shell(cwd.as_deref(), cols, rows);
        let id = self.host.spawn(
            spec,
            move |bytes| {
                // Err only when the webview is gone; nothing useful to do about it here.
                let _ = on_data.send(InvokeResponseBody::Raw(bytes));
            },
            move |session_id, code| {
                let _ = app.emit(EVENT_SESSION_EXIT, SessionExit { session_id, code });
            },
        )?;
        if let Some(key) = resume_key {
            lock(&self.resume_keys).insert(id, key);
        }
        Ok(id)
    }

    pub fn write(&self, id: SessionId, data: &[u8]) -> Result<(), String> {
        self.host.write(id, data)
    }

    pub fn resize(&self, id: SessionId, cols: u16, rows: u16) -> Result<(), String> {
        self.host.resize(id, cols, rows)
    }

    /// Park the reader thread so the kernel applies back-pressure to the child.
    pub fn pause(&self, id: SessionId) -> Result<(), String> {
        self.host.pause(id)
    }

    pub fn resume(&self, id: SessionId) -> Result<(), String> {
        self.host.resume(id)
    }

    /// SIGHUP the Session's process group and clean up. Idempotent for unknown ids.
    pub fn kill(&self, id: SessionId) -> Result<(), String> {
        self.host.kill(id)
    }

    /// One `ProbeTarget` per live Session. Called by the monitor every tick; must be cheap
    /// and must not hold locks across the returned value.
    pub fn probe_targets(&self) -> Vec<ProbeTarget> {
        self.host.probe_targets()
    }

    pub fn probe_target(&self, id: SessionId) -> Option<ProbeTarget> {
        self.host.probe_target(id)
    }

    /// `probe_targets` of the Sessions with a Resume key, with their key.
    pub fn keyed_targets(&self) -> Vec<(String, ProbeTarget)> {
        let targets = self.host.probe_targets();
        let mut keys = lock(&self.resume_keys);
        keys.retain(|id, _| targets.iter().any(|t| t.session_id == *id));
        targets
            .into_iter()
            .filter_map(|t| Some((keys.get(&t.session_id)?.clone(), t)))
            .collect()
    }

    /// Kill every Session. Called on app exit.
    pub fn kill_all(&self) {
        self.host.kill_all()
    }
}

// ---------------------------------------------------------------------------------------------
// What to spawn
// ---------------------------------------------------------------------------------------------

/// A command to run on a new pty. [`SpawnSpec::login_shell`] builds the one the app uses.
pub(crate) struct SpawnSpec {
    pub argv: Vec<OsString>,
    pub cwd: PathBuf,
    /// The complete environment of the child (the app's own env is not inherited on top).
    pub env: BTreeMap<OsString, OsString>,
    pub cols: u16,
    pub rows: u16,
}

impl SpawnSpec {
    /// `$SHELL -l` (fallback `/bin/zsh`) in `cwd` if it is an existing directory, else `$HOME`.
    pub fn login_shell(cwd: Option<&str>, cols: u16, rows: u16) -> Self {
        let shell = resolve_shell(std::env::var_os("SHELL"));
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let cwd = resolve_cwd(cwd, home.as_deref());
        let env = session_env(std::env::vars_os(), &shell, &cwd);
        SpawnSpec {
            argv: vec![shell.into_os_string(), "-l".into()],
            cwd,
            env,
            cols,
            rows,
        }
    }

    fn command(&self) -> CommandBuilder {
        let mut cmd = CommandBuilder::from_argv(self.argv.clone());
        cmd.env_clear();
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        cmd.cwd(&self.cwd);
        cmd
    }
}

fn resolve_shell(var: Option<OsString>) -> PathBuf {
    let usable = |p: &Path| {
        p.is_absolute()
            && std::fs::metadata(p)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    match var.map(PathBuf::from) {
        Some(p) if usable(&p) => p,
        _ => PathBuf::from("/bin/zsh"),
    }
}

fn resolve_cwd(cwd: Option<&str>, home: Option<&Path>) -> PathBuf {
    if let Some(dir) = cwd
        .filter(|c| !c.is_empty())
        .map(Path::new)
        .filter(|p| p.is_dir())
    {
        return dir.to_path_buf();
    }
    match home.filter(|h| h.is_dir()) {
        Some(h) => h.to_path_buf(),
        None => PathBuf::from("/"),
    }
}

/// Variables that would leak the launching terminal or agent into every shell.
const STRIP_EXACT: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "AI_AGENT",
    "LC_TERMINAL",
    "LC_TERMINAL_VERSION",
    "TERMINFO",
    "SHLVL",
    "OLDPWD",
];
const STRIP_PREFIX: &[&str] = &[
    "CLAUDE_CODE_",
    "ITERM_",
    "VSCODE_",
    "GHOSTTY_",
    "WEZTERM_",
    "KITTY_",
    "TERM_SESSION_ID",
];

fn leaks(key: &OsStr) -> bool {
    let k = key.as_bytes();
    STRIP_EXACT.iter().any(|s| k == s.as_bytes())
        || STRIP_PREFIX.iter().any(|p| k.starts_with(p.as_bytes()))
}

fn is_utf8_locale(v: &OsStr) -> bool {
    let v = v.to_string_lossy().to_ascii_lowercase();
    v.contains("utf-8") || v.contains("utf8")
}

/// The child's environment: `base` minus leaky variables, plus what a terminal must set.
fn session_env(
    base: impl IntoIterator<Item = (OsString, OsString)>,
    shell: &Path,
    cwd: &Path,
) -> BTreeMap<OsString, OsString> {
    let mut env: BTreeMap<OsString, OsString> =
        base.into_iter().filter(|(k, _)| !leaks(k)).collect();
    let mut set = |k: &str, v: &OsStr| {
        env.insert(k.into(), v.to_owned());
    };
    set("TERM", OsStr::new("xterm-256color"));
    set("COLORTERM", OsStr::new("truecolor"));
    set("TERM_PROGRAM", OsStr::new("sidebar-term"));
    set(
        "TERM_PROGRAM_VERSION",
        OsStr::new(env!("CARGO_PKG_VERSION")),
    );
    set("SHELL", shell.as_os_str());
    set("PWD", cwd.as_os_str());
    // Apps launched from Finder have no LANG; without a UTF-8 locale zsh/readline mangle input.
    if !env
        .get(OsStr::new("LANG"))
        .is_some_and(|v| is_utf8_locale(v))
    {
        env.insert("LANG".into(), "en_US.UTF-8".into());
    }
    env
}

// ---------------------------------------------------------------------------------------------
// pty core
// ---------------------------------------------------------------------------------------------

/// The pty core: live Sessions, their threads and processes. No Tauri types.
#[derive(Default)]
pub(crate) struct PtyHost {
    registry: Arc<Registry>,
}

#[derive(Default)]
struct Registry {
    sessions: Mutex<HashMap<SessionId, Arc<Session>>>,
    next_id: AtomicU32,
}

struct Session {
    id: SessionId,
    shell_pid: i32,
    /// Raw fd of `master`, for `tcgetpgrp`. Valid as long as this Session is alive (it owns
    /// `master`); the registry holds a reference, so it is valid for every registered Session.
    master_fd: RawFd,
    master: Mutex<Box<dyn MasterPty + Send>>,
    /// Input queue drained by the writer thread. Dropping the Session ends that thread.
    input: mpsc::Sender<Vec<u8>>,
    gate: Gate,
    proc: Mutex<Proc>,
}

struct Proc {
    child: Box<dyn Child + Send + Sync>,
    /// `Some(code)` once reaped. Signals are only sent while this is `None`.
    exit: Option<Option<i32>>,
    /// When `kill` first sent SIGHUP.
    hup_at: Option<Instant>,
    sigkilled: bool,
}

/// Flow-control gate the reader parks on while paused.
#[derive(Default)]
struct Gate {
    paused: AtomicBool,
    /// Set by `kill`: stop honouring pause so a dying child is never stuck on a full tty.
    closing: AtomicBool,
    mu: Mutex<()>,
    cv: Condvar,
}

impl Gate {
    fn set_paused(&self, paused: bool) {
        let _g = lock(&self.mu);
        self.paused.store(paused, Ordering::SeqCst);
        self.cv.notify_all();
    }

    fn close(&self) {
        let _g = lock(&self.mu);
        self.closing.store(true, Ordering::SeqCst);
        self.cv.notify_all();
    }

    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst) && !self.closing.load(Ordering::SeqCst)
    }

    /// Block until not paused or `timeout` passes. True when open.
    fn wait_open(&self, timeout: Duration) -> bool {
        if !self.is_paused() {
            return true;
        }
        let g = lock(&self.mu);
        let _g = self
            .cv
            .wait_timeout_while(g, timeout, |_| self.is_paused())
            .unwrap_or_else(|e| e.into_inner());
        !self.is_paused()
    }
}

fn exit_code(status: &ExitStatus) -> Option<i32> {
    // Death by signal has no exit code.
    match status.signal() {
        Some(_) => None,
        None => Some(status.exit_code() as i32),
    }
}

impl Session {
    fn fg_pgid(&self) -> Option<i32> {
        // SAFETY: plain ioctl on an fd this Session keeps open (see `master_fd`).
        let pgid = unsafe { libc::tcgetpgrp(self.master_fd) };
        (pgid > 0).then_some(pgid)
    }

    fn probe(&self) -> ProbeTarget {
        ProbeTarget {
            session_id: self.id,
            shell_pid: self.shell_pid,
            fg_pgid: self.fg_pgid(),
        }
    }

    /// Reap the shell if it has exited. Non-blocking; true once reaped.
    fn try_reap(p: &mut Proc) -> bool {
        if p.exit.is_some() {
            return true;
        }
        match p.child.try_wait() {
            Ok(Some(status)) => {
                p.exit = Some(exit_code(&status));
                true
            }
            Ok(None) => false,
            // ECHILD: nothing left to reap, so nothing left to signal either.
            Err(_) => {
                p.exit = Some(None);
                true
            }
        }
    }

    /// Signal the shell's process group (the shell is its leader after `setsid`).
    fn signal_locked(&self, p: &Proc, sig: libc::c_int) {
        if p.exit.is_some() {
            return;
        }
        // SAFETY: plain syscalls; `shell_pid` is our unreaped child, so it cannot be recycled.
        unsafe {
            if libc::killpg(self.shell_pid, sig) != 0 {
                libc::kill(self.shell_pid, sig);
            }
        }
    }

    fn sigkill_locked(&self, p: &mut Proc) {
        if p.exit.is_some() {
            return;
        }
        if let Some(fg) = self.fg_pgid().filter(|&fg| fg != self.shell_pid) {
            // SAFETY: plain syscall on the tty's current foreground group.
            unsafe { libc::killpg(fg, libc::SIGKILL) };
        }
        self.signal_locked(p, libc::SIGKILL);
        p.sigkilled = true;
    }

    /// SIGHUP now; the reader escalates to SIGKILL after [`KILL_GRACE`].
    fn hangup(&self) {
        {
            let mut p = lock(&self.proc);
            if p.exit.is_none() {
                p.hup_at.get_or_insert_with(Instant::now);
                self.signal_locked(&p, libc::SIGHUP);
            }
        }
        self.gate.close();
    }

    /// Reader housekeeping: reap if exited, escalate a pending kill. True once reaped.
    fn tick(&self) -> bool {
        let mut p = lock(&self.proc);
        if Self::try_reap(&mut p) {
            return true;
        }
        if let Some(at) = p.hup_at {
            if !p.sigkilled && at.elapsed() >= KILL_GRACE {
                self.sigkill_locked(&mut p);
            }
        }
        false
    }

    /// Reap after the master hit EOF. Normally immediate; a shell that closed its tty but keeps
    /// running gets SIGHUP, then SIGKILL.
    fn reap(&self) -> Option<i32> {
        let start = Instant::now();
        let mut hupped = false;
        loop {
            {
                let mut p = lock(&self.proc);
                if Self::try_reap(&mut p) {
                    return p.exit.flatten();
                }
                let waited = start.elapsed();
                if waited >= EOF_KILL_AFTER && !p.sigkilled {
                    self.sigkill_locked(&mut p);
                } else if waited >= EOF_HUP_AFTER && !hupped {
                    self.signal_locked(&p, libc::SIGHUP);
                    hupped = true;
                }
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
}

impl PtyHost {
    /// Spawn `spec` on a new pty. `on_output` gets the output in order, in chunks of at most
    /// [`CHUNK_MAX`] bytes, on the Session's reader thread (possibly before this returns).
    /// `on_exit(id, code)` runs once after the last output, when the child has been reaped and
    /// the Session removed from the registry. `code` is `None` when the child died by a signal.
    pub fn spawn<O, E>(
        &self,
        spec: SpawnSpec,
        on_output: O,
        on_exit: E,
    ) -> Result<SessionId, String>
    where
        O: FnMut(Vec<u8>) + Send + 'static,
        E: FnOnce(SessionId, Option<i32>) + Send + 'static,
    {
        let size = PtySize {
            rows: spec.rows.max(1),
            cols: spec.cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };
        let PtyPair { slave, master } = native_pty_system()
            .openpty(size)
            .map_err(|e| format!("openpty failed: {e:#}"))?;
        let master_fd = master.as_raw_fd().ok_or("pty master has no fd")?;
        let src = dup_fd(master_fd)?;
        let dst = dup_fd(master_fd)?;

        let mut child = slave
            .spawn_command(spec.command())
            .map_err(|e| format!("spawning {} failed: {e:#}", spec.argv[0].to_string_lossy()))?;
        // Close our copy of the slave so the master sees EOF once the child's copies close.
        drop(slave);
        let Some(shell_pid) = child.process_id().and_then(|p| i32::try_from(p).ok()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("spawned child has no pid".into());
        };

        let id = self.registry.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let (input, queue) = mpsc::channel::<Vec<u8>>();
        let writer = thread::Builder::new()
            .name(format!("pty-write-{id}"))
            .spawn(move || writer_loop(dst, queue));
        if let Err(e) = writer {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("spawning pty writer thread failed: {e}"));
        }

        let session = Arc::new(Session {
            id,
            shell_pid,
            master_fd,
            master: Mutex::new(master),
            input,
            gate: Gate::default(),
            proc: Mutex::new(Proc {
                child,
                exit: None,
                hup_at: None,
                sigkilled: false,
            }),
        });
        // Register before the reader starts: the reader is what unregisters.
        lock(&self.registry.sessions).insert(id, session.clone());

        let for_reader = session.clone();
        let registry = self.registry.clone();
        let reader = thread::Builder::new()
            .name(format!("pty-read-{id}"))
            .spawn(move || reader_loop(for_reader, registry, src, on_output, on_exit));
        if let Err(e) = reader {
            lock(&self.registry.sessions).remove(&id);
            session.hangup();
            let _ = session.reap();
            return Err(format!("spawning pty reader thread failed: {e}"));
        }
        Ok(id)
    }

    fn get(&self, id: SessionId) -> Option<Arc<Session>> {
        lock(&self.registry.sessions).get(&id).cloned()
    }

    fn require(&self, id: SessionId) -> Result<Arc<Session>, String> {
        self.get(id).ok_or_else(|| format!("no session {id}"))
    }

    /// Queue `data` for the child. Never blocks on the pty.
    pub fn write(&self, id: SessionId, data: &[u8]) -> Result<(), String> {
        let s = self.require(id)?;
        if data.is_empty() {
            return Ok(());
        }
        s.input
            .send(data.to_vec())
            .map_err(|_| format!("session {id} input is closed"))
    }

    /// Set the pty size; the kernel sends SIGWINCH to the foreground process group.
    pub fn resize(&self, id: SessionId, cols: u16, rows: u16) -> Result<(), String> {
        let s = self.require(id)?;
        let size = PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        };
        let master = lock(&s.master);
        master
            .resize(size)
            .map_err(|e| format!("resize session {id}: {e:#}"))
    }

    /// Idempotent; unknown ids are ignored (the Session may just have exited).
    pub fn pause(&self, id: SessionId) -> Result<(), String> {
        if let Some(s) = self.get(id) {
            s.gate.set_paused(true);
        }
        Ok(())
    }

    /// Idempotent; unknown ids are ignored.
    pub fn resume(&self, id: SessionId) -> Result<(), String> {
        if let Some(s) = self.get(id) {
            s.gate.set_paused(false);
        }
        Ok(())
    }

    /// SIGHUP the shell's process group; SIGKILL follows if it has not exited after
    /// [`KILL_GRACE`]. The exit callback fires when it is gone. Idempotent for unknown ids.
    pub fn kill(&self, id: SessionId) -> Result<(), String> {
        if let Some(s) = self.get(id) {
            s.hangup();
        }
        Ok(())
    }

    /// Only the registry lock is held, and only across one `tcgetpgrp` ioctl per Session.
    pub fn probe_targets(&self) -> Vec<ProbeTarget> {
        let sessions = lock(&self.registry.sessions);
        let mut targets: Vec<ProbeTarget> = sessions.values().map(|s| s.probe()).collect();
        drop(sessions);
        targets.sort_by_key(|t| t.session_id);
        targets
    }

    pub fn probe_target(&self, id: SessionId) -> Option<ProbeTarget> {
        lock(&self.registry.sessions).get(&id).map(|s| s.probe())
    }

    /// SIGHUP every Session. Does not wait: on process exit every master closes and the kernel
    /// hangs up any shell still alive.
    pub fn kill_all(&self) {
        let all: Vec<Arc<Session>> = lock(&self.registry.sessions).values().cloned().collect();
        for s in all {
            s.hangup();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Threads and fd helpers
// ---------------------------------------------------------------------------------------------

fn dup_fd(fd: RawFd) -> Result<File, String> {
    // SAFETY: `fd` is the master's fd, owned by a live `MasterPty` for the whole call.
    let owned = unsafe { BorrowedFd::borrow_raw(fd) }
        .try_clone_to_owned() // F_DUPFD_CLOEXEC
        .map_err(|e| format!("dup pty master: {e}"))?;
    Ok(File::from(owned))
}

/// True when `fd` is readable (or hung up / errored, which the next read reports).
fn poll_readable(fd: RawFd, timeout: Duration) -> io::Result<bool> {
    let ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        // SAFETY: one valid pollfd.
        let r = unsafe { libc::poll(&mut pfd, 1, ms) };
        if r >= 0 {
            return Ok(r > 0);
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

/// `read`, retrying EINTR. `Ok(0)` is EOF, including EIO (the slave side is gone).
fn read_some(src: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        match src.read(buf) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if e.raw_os_error() == Some(libc::EIO) => return Ok(0),
            r => return r,
        }
    }
}

/// Read what is available into `chunk` (up to [`CHUNK_MAX`]), lingering briefly while a burst
/// is in flight so ~1 KiB kernel reads become one message. Stops early when paused.
/// Returns false on EOF or a read error.
fn coalesce(src: &mut File, fd: RawFd, buf: &mut [u8], chunk: &mut Vec<u8>, gate: &Gate) -> bool {
    let started = Instant::now();
    loop {
        let room = (CHUNK_MAX - chunk.len()).min(buf.len());
        let n = match read_some(src, &mut buf[..room]) {
            Ok(0) | Err(_) => return false,
            Ok(n) => n,
        };
        chunk.extend_from_slice(&buf[..n]);
        if chunk.len() >= CHUNK_MAX || gate.is_paused() {
            return true;
        }
        let linger = n >= BIG_READ && started.elapsed() < LINGER_BUDGET;
        match poll_readable(fd, if linger { LINGER_STEP } else { Duration::ZERO }) {
            Ok(true) => continue,
            // Nothing more right now (or a poll error the next loop iteration will surface).
            Ok(false) | Err(_) => return true,
        }
    }
}

fn reader_loop<O, E>(
    session: Arc<Session>,
    registry: Arc<Registry>,
    mut src: File,
    mut on_output: O,
    on_exit: E,
) where
    O: FnMut(Vec<u8>),
    E: FnOnce(SessionId, Option<i32>),
{
    let _live = ThreadGuard::enter();
    let fd = src.as_raw_fd();
    let mut buf = vec![0u8; READ_BUF];
    let mut chunk: Vec<u8> = Vec::with_capacity(CHUNK_MAX);
    let mut flush = |chunk: &mut Vec<u8>| {
        if !chunk.is_empty() {
            // split_off(0) keeps `chunk`'s capacity for the next round.
            on_output(chunk.split_off(0));
        }
    };
    let mut last_tick = Instant::now();

    loop {
        if last_tick.elapsed() >= TICK {
            last_tick = Instant::now();
            if session.tick() {
                // Shell reaped but the master never reached EOF (something else still holds the
                // tty). Deliver what is buffered, then end the Session anyway.
                let mut drained = 0;
                while drained < EXIT_DRAIN_MAX && poll_readable(fd, Duration::ZERO).unwrap_or(false)
                {
                    let open = coalesce(&mut src, fd, &mut buf, &mut chunk, &session.gate);
                    drained += chunk.len();
                    flush(&mut chunk);
                    if !open {
                        break;
                    }
                }
                break;
            }
        }
        if !session.gate.wait_open(TICK) {
            continue; // still paused: go round for housekeeping
        }
        match poll_readable(fd, TICK) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(_) => break,
        }
        if session.gate.is_paused() {
            continue; // paused while we waited: park before reading
        }
        let open = coalesce(&mut src, fd, &mut buf, &mut chunk, &session.gate);
        flush(&mut chunk);
        if !open {
            break;
        }
    }

    let code = session.reap();
    let id = session.id;
    lock(&registry.sessions).remove(&id);
    // Last references: closes the master and the input queue (which ends the writer thread).
    drop(session);
    drop(src);
    on_exit(id, code);
}

fn writer_loop(mut dst: File, queue: mpsc::Receiver<Vec<u8>>) {
    let _live = ThreadGuard::enter();
    for data in queue {
        if dst.write_all(&data).is_err() {
            break; // EIO: the child side is gone; later writes fail at `send`
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tests: real ptys, real shells
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;

    const T: Duration = Duration::from_secs(10);

    /// pty tests run one at a time so thread / fd accounting and timings are not disturbed.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[derive(Default)]
    struct Output {
        bytes: Vec<u8>,
        chunks: Vec<usize>,
    }

    struct Pty {
        host: PtyHost,
        id: SessionId,
        out: Arc<(Mutex<Output>, Condvar)>,
        exits: Receiver<(SessionId, Option<i32>)>,
        _serial: MutexGuard<'static, ()>,
    }

    fn zsh_spec() -> SpawnSpec {
        let cwd = std::env::temp_dir();
        let shell = Path::new("/bin/zsh");
        SpawnSpec {
            argv: vec!["/bin/zsh".into(), "-f".into()],
            env: session_env(std::env::vars_os(), shell, &cwd),
            cwd,
            cols: 80,
            rows: 24,
        }
    }

    impl Pty {
        fn start(spec: SpawnSpec) -> Pty {
            Pty::start_serial(spec, lock(&SERIAL))
        }

        fn start_serial(spec: SpawnSpec, serial: MutexGuard<'static, ()>) -> Pty {
            let host = PtyHost::default();
            let out: Arc<(Mutex<Output>, Condvar)> = Arc::default();
            let sink = out.clone();
            let (tx, exits) = mpsc::channel();
            let id = host
                .spawn(
                    spec,
                    move |bytes| {
                        let mut o = lock(&sink.0);
                        o.chunks.push(bytes.len());
                        o.bytes.extend_from_slice(&bytes);
                        sink.1.notify_all();
                    },
                    move |id, code| {
                        let _ = tx.send((id, code));
                    },
                )
                .expect("spawn");
            Pty {
                host,
                id,
                out,
                exits,
                _serial: serial,
            }
        }

        fn zsh() -> Pty {
            Pty::zsh_serial(lock(&SERIAL))
        }

        fn zsh_serial(serial: MutexGuard<'static, ()>) -> Pty {
            let p = Pty::start_serial(zsh_spec(), serial);
            p.send("echo READY_$((6*7))\r");
            assert!(
                p.wait_for("READY_42", T),
                "shell never became ready: {}",
                p.text()
            );
            p.settle();
            p
        }

        fn send(&self, s: &str) {
            self.host.write(self.id, s.as_bytes()).expect("write");
        }

        fn len(&self) -> usize {
            lock(&self.out.0).bytes.len()
        }

        fn text(&self) -> String {
            String::from_utf8_lossy(&lock(&self.out.0).bytes).into_owned()
        }

        fn wait_for(&self, needle: &str, timeout: Duration) -> bool {
            let deadline = Instant::now() + timeout;
            let needle = needle.as_bytes();
            let mut scanned: usize = 0; // only scan new bytes: a burst is ~23 MB
            let mut o = lock(&self.out.0);
            loop {
                let from = scanned.saturating_sub(needle.len());
                if o.bytes[from..].windows(needle.len()).any(|w| w == needle) {
                    return true;
                }
                scanned = o.bytes.len();
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return false;
                }
                o = self.out.1.wait_timeout(o, left).unwrap().0;
            }
        }

        /// Wait until no output has arrived for 200 ms.
        fn settle(&self) {
            let mut last = self.len();
            loop {
                thread::sleep(Duration::from_millis(200));
                let now = self.len();
                if now == last {
                    return;
                }
                last = now;
            }
        }

        fn target(&self) -> ProbeTarget {
            self.host.probe_target(self.id).expect("session is live")
        }

        fn wait_exit(&self, timeout: Duration) -> Option<(SessionId, Option<i32>)> {
            self.exits.recv_timeout(timeout).ok()
        }
    }

    impl Drop for Pty {
        fn drop(&mut self) {
            if self.host.probe_target(self.id).is_some() {
                let _ = self.host.kill(self.id);
                let _ = self.exits.recv_timeout(T);
            }
        }
    }

    fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        cond()
    }

    fn open_fds() -> usize {
        std::fs::read_dir("/dev/fd").unwrap().count()
    }

    #[test]
    fn echo_round_trip() {
        let p = Pty::zsh();
        p.send("echo RT_$((40+2))\r");
        assert!(p.wait_for("RT_42", T), "{}", p.text());
    }

    #[test]
    fn keystroke_echo_is_not_held_back() {
        let p = Pty::zsh();
        let mut samples = Vec::new();
        for _ in 0..20 {
            let before = p.len();
            let t = Instant::now();
            p.send("x");
            let mut o = lock(&p.out.0);
            while o.bytes.len() == before {
                o = p.out.1.wait_timeout(o, T).unwrap().0;
            }
            samples.push(t.elapsed());
            drop(o);
            thread::sleep(Duration::from_millis(20));
        }
        p.send("\x15"); // ^U: clear the line
        samples.sort();
        let median = samples[samples.len() / 2];
        eprintln!(
            "echo latency: median {median:?}, max {:?}",
            samples.last().unwrap()
        );
        assert!(
            median < Duration::from_millis(5),
            "median echo latency {median:?}"
        );
    }

    #[test]
    fn stty_size_follows_resize() {
        let p = Pty::zsh();
        p.send("stty size\r");
        assert!(p.wait_for("24 80", T), "{}", p.text());
        p.host.resize(p.id, 100, 40).unwrap();
        p.send("stty size\r");
        assert!(p.wait_for("40 100", T), "{}", p.text());
    }

    #[test]
    fn pause_stops_delivery_and_resume_restores_it() {
        let p = Pty::zsh();
        p.host.pause(p.id).unwrap();
        p.host.pause(p.id).unwrap(); // idempotent
        let before = p.len();
        // Enough output to fill the tty queue, so the child really stalls on back-pressure.
        p.send("head -c 300000 /dev/zero | od -v; echo PAUSED_$((2+2))\r");
        thread::sleep(Duration::from_millis(700));
        assert_eq!(p.len(), before, "output was delivered while paused");
        p.host.resume(p.id).unwrap();
        p.host.resume(p.id).unwrap(); // idempotent
        assert!(p.wait_for("PAUSED_4", T), "{}", p.text());
        // 300000 bytes / 16 per line; the final offset line is 300000 in octal.
        assert!(p.text().contains(&format!("{:o}", 300000)));
        // Unknown ids are fine.
        p.host.pause(999_999).unwrap();
        p.host.resume(999_999).unwrap();
    }

    #[test]
    fn kill_reports_exit_and_reaps() {
        let p = Pty::zsh();
        let pid = p.target().shell_pid;
        p.host.kill(p.id).unwrap();
        let (id, _code) = p.wait_exit(T).expect("exit callback");
        assert_eq!(id, p.id);
        assert!(
            p.host.probe_target(p.id).is_none(),
            "still registered after exit"
        );
        assert!(p.host.probe_targets().is_empty());
        // Reaped, not a zombie: waitpid has nothing to collect, and the pid is gone.
        let r = unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
        assert_eq!(r, -1, "child {pid} was not reaped");
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "child {pid} still exists"
        );
        // Idempotent, and a gone Session rejects input.
        p.host.kill(p.id).unwrap();
        p.host.kill(999_999).unwrap();
        assert!(p.host.write(p.id, b"x").is_err());
    }

    #[test]
    fn exit_code_is_reported() {
        let p = Pty::zsh();
        p.send("exit 7\r");
        assert_eq!(p.wait_exit(T), Some((p.id, Some(7))));
    }

    #[test]
    fn kill_escalates_when_hup_is_ignored() {
        let cwd = std::env::temp_dir();
        let spec = SpawnSpec {
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "trap '' HUP; echo IGNORING_$((1+1)); while :; do sleep 1; done".into(),
            ],
            env: session_env(std::env::vars_os(), Path::new("/bin/sh"), &cwd),
            cwd,
            cols: 80,
            rows: 24,
        };
        let p = Pty::start(spec);
        assert!(p.wait_for("IGNORING_2", T), "{}", p.text());
        let started = Instant::now();
        p.host.kill(p.id).unwrap();
        let (_, code) = p.wait_exit(T).expect("exit after SIGKILL escalation");
        assert_eq!(code, None, "died by signal");
        assert!(started.elapsed() >= KILL_GRACE, "exited before escalation?");
    }

    #[test]
    fn foreground_pgid_tracks_the_running_job() {
        let p = Pty::zsh();
        let shell = p.target().shell_pid;
        assert_eq!(
            p.target().fg_pgid,
            Some(shell),
            "idle shell should own the tty"
        );

        p.send("sleep 2\r");
        assert!(
            wait_until(
                Duration::from_millis(1500),
                || matches!(p.target().fg_pgid, Some(g) if g != shell)
            ),
            "fg pgid never left the shell"
        );
        let fg = p.target().fg_pgid.unwrap();
        let comm = std::process::Command::new("ps")
            .args(["-o", "comm=", "-p", &fg.to_string()])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&comm.stdout).contains("sleep"),
            "fg {fg} is not sleep"
        );

        assert!(
            wait_until(Duration::from_secs(5), || p.target().fg_pgid == Some(shell)),
            "fg pgid never returned to the shell"
        );
    }

    #[test]
    fn burst_arrives_complete_and_in_order() {
        let p = Pty::zsh();
        let before = p.len();
        // `od -v` so repeated lines are not collapsed: ~23 MB through the pty.
        p.send("head -c 5000000 /dev/zero | od -v; echo BURST_$((2+3))\r");
        assert!(
            p.wait_for("BURST_5", Duration::from_secs(60)),
            "burst never finished"
        );

        let o = lock(&p.out.0);
        let text = String::from_utf8_lossy(&o.bytes[before..]).into_owned();
        let start = text.find("\r\n0000000 ").expect("od output") + 2;
        let mut lines = text[start..].split("\r\n");
        let rows = 5_000_000 / 16;
        for i in 0..rows {
            let line = lines
                .next()
                .unwrap_or_else(|| panic!("output ended at row {i}"));
            let mut tokens = line.split_whitespace();
            let offset = tokens.next().map(|t| u64::from_str_radix(t, 8));
            assert_eq!(offset, Some(Ok(i * 16)), "row {i} out of order: {line:?}");
            assert_eq!(
                tokens.filter(|t| *t == "000000").count(),
                8,
                "row {i} damaged: {line:?}"
            );
        }
        assert_eq!(lines.next(), Some(format!("{:o}", 5_000_000).as_str()));

        let chunks = &o.chunks;
        let max = chunks.iter().max().copied().unwrap_or(0);
        assert!(max <= CHUNK_MAX, "chunk of {max} bytes exceeds the cap");
        eprintln!(
            "burst: {} bytes in {} chunks (max {max}, mean {})",
            o.bytes.len() - before,
            chunks.len(),
            (o.bytes.len() - before) / chunks.len().max(1)
        );
    }

    #[test]
    fn threads_and_fds_are_released() {
        // Let earlier tests' threads finish before taking a baseline.
        let serial = lock(&SERIAL);
        assert!(
            wait_until(T, || LIVE_THREADS.load(Ordering::SeqCst) == 0),
            "threads leaked earlier"
        );
        let fds = open_fds();

        let p = Pty::zsh_serial(serial);
        assert_eq!(
            LIVE_THREADS.load(Ordering::SeqCst),
            2,
            "one reader + one writer"
        );
        assert!(open_fds() > fds);
        p.host.kill(p.id).unwrap();
        p.wait_exit(T).expect("exit");
        assert!(
            wait_until(T, || LIVE_THREADS.load(Ordering::SeqCst) == 0),
            "pty threads leaked"
        );
        assert!(
            wait_until(T, || open_fds() == fds),
            "fds leaked: {} -> {}",
            fds,
            open_fds()
        );
    }

    #[test]
    fn kill_all_ends_every_session() {
        let _serial = lock(&SERIAL);
        let host = PtyHost::default();
        let (tx, rx) = mpsc::channel();
        for _ in 0..3 {
            let tx = tx.clone();
            host.spawn(zsh_spec(), |_| {}, move |id, _| tx.send(id).unwrap())
                .unwrap();
        }
        assert_eq!(host.probe_targets().len(), 3);
        host.kill_all();
        for _ in 0..3 {
            rx.recv_timeout(T).expect("exit");
        }
        assert!(host.probe_targets().is_empty());
    }

    #[test]
    fn cwd_is_used_when_it_exists() {
        let dir = std::env::temp_dir().join(format!("sidebar-term-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut spec = zsh_spec();
        spec.cwd = resolve_cwd(dir.to_str(), None);
        let p = Pty::start(spec);
        p.send("echo \"CWD=[$(pwd -P)]\"\r");
        let want = format!("CWD=[{}]", dir.canonicalize().unwrap().display());
        assert!(p.wait_for(&want, T), "{}", p.text());
        drop(p);
        std::fs::remove_dir(&dir).unwrap();
    }

    /// The real thing: the user's `$SHELL -l` with their rc files, via `SpawnSpec::login_shell`.
    #[test]
    fn login_shell_gets_a_clean_terminal_env() {
        let p = Pty::start(SpawnSpec::login_shell(
            Some("/definitely/not/a/dir"),
            80,
            24,
        ));
        p.send("echo \"ENV=[$TERM|$COLORTERM|$TERM_PROGRAM|${CLAUDECODE:-none}|${CLAUDE_CODE_ENTRYPOINT:-none}|$PWD]\"\r");
        let home = std::env::var("HOME").unwrap();
        let want = format!("ENV=[xterm-256color|truecolor|sidebar-term|none|none|{home}]");
        assert!(p.wait_for(&want, Duration::from_secs(20)), "{}", p.text());
    }

    #[test]
    fn env_strips_leaks_and_sets_terminal_vars() {
        let base = [
            ("PATH", "/usr/bin"),
            ("HOME", "/Users/x"),
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_ENTRYPOINT", "cli"),
            ("ITERM_SESSION_ID", "w0t0"),
            ("VSCODE_PID", "1"),
            ("GHOSTTY_RESOURCES_DIR", "/x"),
            ("WEZTERM_PANE", "1"),
            ("KITTY_WINDOW_ID", "1"),
            ("TERM_SESSION_ID", "abc"),
            ("TERM", "xterm-ghostty"),
            ("TERM_PROGRAM", "iTerm.app"),
            ("SHLVL", "3"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        let env = session_env(base, Path::new("/bin/zsh"), Path::new("/tmp"));
        let get = |k: &str| {
            env.get(OsStr::new(k))
                .map(|v| v.to_str().unwrap().to_owned())
        };
        for k in [
            "CLAUDECODE",
            "CLAUDE_CODE_ENTRYPOINT",
            "ITERM_SESSION_ID",
            "VSCODE_PID",
            "GHOSTTY_RESOURCES_DIR",
            "WEZTERM_PANE",
            "KITTY_WINDOW_ID",
            "TERM_SESSION_ID",
            "SHLVL",
        ] {
            assert_eq!(get(k), None, "{k} leaked");
        }
        assert_eq!(get("PATH").as_deref(), Some("/usr/bin"));
        assert_eq!(get("TERM").as_deref(), Some("xterm-256color"));
        assert_eq!(get("COLORTERM").as_deref(), Some("truecolor"));
        assert_eq!(get("TERM_PROGRAM").as_deref(), Some("sidebar-term"));
        assert_eq!(
            get("TERM_PROGRAM_VERSION").as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            get("LANG").as_deref(),
            Some("en_US.UTF-8"),
            "LANG unset -> UTF-8 default"
        );
        assert_eq!(get("PWD").as_deref(), Some("/tmp"));
    }

    #[test]
    fn env_lang_is_kept_only_when_utf8() {
        let with = |lang: &str| {
            let base = [(OsString::from("LANG"), OsString::from(lang))];
            let env = session_env(base, Path::new("/bin/zsh"), Path::new("/"));
            env[OsStr::new("LANG")].to_str().unwrap().to_owned()
        };
        assert_eq!(with("de_DE.UTF-8"), "de_DE.UTF-8");
        assert_eq!(with("fr_FR.utf8"), "fr_FR.utf8");
        assert_eq!(with("C"), "en_US.UTF-8");
        assert_eq!(with("en_US.ISO8859-1"), "en_US.UTF-8");
    }

    #[test]
    fn shell_and_cwd_fallbacks() {
        assert_eq!(resolve_shell(None), PathBuf::from("/bin/zsh"));
        assert_eq!(resolve_shell(Some("".into())), PathBuf::from("/bin/zsh"));
        assert_eq!(
            resolve_shell(Some("/no/such/shell".into())),
            PathBuf::from("/bin/zsh")
        );
        assert_eq!(
            resolve_shell(Some("bash".into())),
            PathBuf::from("/bin/zsh"),
            "must be absolute"
        );
        assert_eq!(
            resolve_shell(Some("/bin/bash".into())),
            PathBuf::from("/bin/bash")
        );

        let home = std::env::temp_dir();
        assert_eq!(resolve_cwd(Some("/"), Some(&home)), PathBuf::from("/"));
        assert_eq!(resolve_cwd(Some("/no/such/dir"), Some(&home)), home);
        assert_eq!(resolve_cwd(None, Some(&home)), home);
        assert_eq!(resolve_cwd(Some(""), None), PathBuf::from("/"));
        assert_eq!(
            resolve_cwd(Some("/etc/hosts"), Some(&home)),
            home,
            "a file is not a cwd"
        );
    }
}
