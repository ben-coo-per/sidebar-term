//! sidebar-termd: the headless Host daemon (ADR 0002). Runs the sidebar-term core on a machine
//! with no display (a Linux box under `systemctl --user`, `packaging/systemd/`), or on a Mac for
//! a smoke test, and serves its Sessions over Remote. See docs/architecture.md "Host daemon".
//!
//! It starts the Session core, the layout (its own `layout.json`: every Tab's Session is spawned
//! at launch, one Tab on a fresh install), the monitor, the Journal, Resume and the Remote server,
//! exactly as the app does, and stops on SIGTERM after writing the layout, writing the Journal's
//! tallies, recording Resume entries and killing every Session, as the app does on quit. Phones list its Tabs and drive their
//! Sessions, and create, close, rename and move its Tabs and Groups over the Host protocol.
//! Process facts and Activity on Linux come from `/proc`.
//!
//! Flags: `--data-dir <dir>` (default: see [`default_data_dir`]), `--port <n>` (kept in
//! `remote.json`), `--web-root <dir>` (the built phone page, `pnpm build`'s `build/`; default
//! `<data dir>/web`), `--pair` (start a pairing at launch and print its code). SIGUSR1 starts a
//! pairing at any time: `systemctl --user kill --kill-whom=main -s USR1 sidebar-termd` (without
//! `--kill-whom=main` the signal reaches every Session's processes and kills them), then read the
//! code in the journal. Everything is logged to stderr.
//!
//! `--cgroups` (Linux, with `Delegate=yes` in the unit: `packaging/systemd/`) splits the
//! daemon's cgroup so the Sessions' memory limit never throttles the daemon itself, puts each
//! Session in a cgroup of its own, and sends clients a `pressure` sample every few seconds
//! (`cgroup.rs`). `--sessions-memory-high`, `--sessions-memory-max` and `--sessions-swap-max`
//! (`9G`, `512M`, bytes) set the limits on all the Sessions together. `--close-idle-after
//! <hours>` (default 24, `0` never) closes Tabs whose Sessions sat idle that long (`idle.rs`),
//! logging how to resume each.

use sidebar_term_core::agents::Agents;
use sidebar_term_core::cgroup::{self, Cgroups, Limits};
use sidebar_term_core::idle::{Candidate, IdleCloser};
use sidebar_term_core::host::{Asset, Assets, Events, Host, Paths};
use sidebar_term_core::journal::{self, Journal};
use sidebar_term_core::layout::Layout;
use sidebar_term_core::model::{
    HostPressure, LayoutSnapshot, PressureLevel, RemoteSnapshot, SessionExit, EVENT_LAYOUT,
    EVENT_REMOTE, EVENT_SESSION_EXIT,
};
use sidebar_term_core::remote::{Remote, Taps};
use sidebar_term_core::resume::{self, Resume};
use sidebar_term_core::session::SessionManager;
use sidebar_term_core::{monitor, store};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::broadcast;

/// The Mac app's bundle identifier (`tauri.conf.json`), whose app-data dir the daemon's default
/// data dir sits in on a Mac.
#[cfg(target_os = "macos")]
const APP_IDENTIFIER: &str = "com.bencooper.sidebarterm";

const USAGE: &str = "\
usage: sidebar-termd [--data-dir <dir>] [--port <n>] [--web-root <dir>] [--pair]
                     [--cgroups [--sessions-memory-high <size>] [--sessions-memory-max <size>]
                                [--sessions-swap-max <size>]]
                     [--close-idle-after <hours>]

  --data-dir <dir>   where the Host keeps remote.json, resume.json, ... (default: see README)
  --port <n>         the port the Remote server binds on 127.0.0.1; kept in remote.json
  --web-root <dir>   the built phone page (pnpm build's build/); default <data dir>/web
  --pair             start a pairing at launch and print its code (SIGUSR1 does it any time)
  --cgroups          Linux, cgroup v2, a delegated cgroup (Delegate=yes): run the daemon and
                     the Sessions in cgroups of their own, so the Sessions' memory limit never
                     throttles the daemon (packaging/systemd/README.md)
  --sessions-memory-high <size>, --sessions-memory-max <size>, --sessions-swap-max <size>
                     limits on every Session together (9G, 512M, bytes); with --cgroups
  --close-idle-after <hours>
                     close Tabs idle that long, logging how to resume each (default 24; 0 never)
  --help, --version";

/// `--close-idle-after` when not given.
const CLOSE_IDLE_AFTER_HOURS: f64 = 24.0;
/// How often the Sessions' memory is sampled and sent to clients.
const PRESSURE_EVERY: Duration = Duration::from_secs(2);
/// How often empty Session cgroups are swept, in samples.
const SWEEP_EVERY: u32 = 30;
/// How often idle Tabs are looked for.
const IDLE_EVERY: Duration = Duration::from_secs(60);
/// How often Tailscale is asked again while it does not publish the server.
const TAILSCALE_EVERY: Duration = Duration::from_secs(60);

#[derive(Debug, Default, PartialEq)]
struct Args {
    data_dir: Option<PathBuf>,
    port: Option<u16>,
    web_root: Option<PathBuf>,
    pair: bool,
    cgroups: bool,
    limits: Limits,
    /// Hours; `None`: the default.
    close_idle_after: Option<f64>,
    help: bool,
    version: bool,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut out = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| args.next().ok_or_else(|| format!("{flag} needs a value"));
        match arg.as_str() {
            "--data-dir" => out.data_dir = Some(PathBuf::from(value("--data-dir")?)),
            "--web-root" => out.web_root = Some(PathBuf::from(value("--web-root")?)),
            "--port" => {
                let raw = value("--port")?;
                out.port = Some(
                    raw.parse::<u16>()
                        .ok()
                        .filter(|p| *p > 0)
                        .ok_or_else(|| format!("--port {raw}: not a port number"))?,
                );
            }
            "--pair" => out.pair = true,
            "--cgroups" => out.cgroups = true,
            flag @ ("--sessions-memory-high" | "--sessions-memory-max" | "--sessions-swap-max") => {
                let raw = value(flag)?;
                let size = cgroup::parse_size(&raw).ok_or_else(|| format!("{flag} {raw}: not a size (9G, 512M, bytes)"))?;
                match flag {
                    "--sessions-memory-high" => out.limits.high = Some(size),
                    "--sessions-memory-max" => out.limits.max = Some(size),
                    _ => out.limits.swap_max = Some(size),
                }
            }
            "--close-idle-after" => {
                let raw = value("--close-idle-after")?;
                out.close_idle_after = Some(
                    raw.parse::<f64>()
                        .ok()
                        .filter(|h| h.is_finite() && *h >= 0.0)
                        .ok_or_else(|| format!("--close-idle-after {raw}: not a number of hours"))?,
                );
            }
            "--help" | "-h" => out.help = true,
            "--version" | "-V" => out.version = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if out.limits != Limits::default() && !out.cgroups {
        return Err("the --sessions-* limits need --cgroups".into());
    }
    Ok(out)
}

/// Where the daemon keeps its files when `--data-dir` is not given: `$XDG_DATA_HOME/sidebar-term`
/// (`~/.local/share/sidebar-term`).
#[cfg(not(target_os = "macos"))]
fn default_data_dir(home: Option<&Path>, xdg_data_home: Option<&Path>) -> Option<PathBuf> {
    xdg_data_home
        .filter(|p| p.is_absolute())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|h| h.join(".local/share")))
        .map(|d| d.join("sidebar-term"))
}

/// Where the daemon keeps its files when `--data-dir` is not given. On a Mac the app already has
/// `~/Library/Application Support/<identifier>`; the daemon takes its own `daemon/` inside it, so
/// a daemon and the app on one Mac never read each other's `remote.json` or `resume.json`.
#[cfg(target_os = "macos")]
fn default_data_dir(home: Option<&Path>, _xdg_data_home: Option<&Path>) -> Option<PathBuf> {
    home.map(|h| {
        h.join("Library/Application Support")
            .join(APP_IDENTIFIER)
            .join("daemon")
    })
}

// --- The daemon's Host ---------------------------------------------------------------------------

/// One event from the core, as the bus carries it.
#[derive(Clone, Debug)]
struct Event {
    name: String,
    payload: serde_json::Value,
}

/// The daemon's event bus. A broadcast, so the log and, from #28, the Remote server each read
/// every event; a reader that falls behind loses the oldest, never the sender.
struct Bus(broadcast::Sender<Event>);

impl Events for Bus {
    fn emit_value(&self, name: &str, payload: serde_json::Value) {
        // Err means no reader right now; the events are not owed to anyone.
        let _ = self.0.send(Event {
            name: name.to_owned(),
            payload,
        });
    }
}

struct DataDir(PathBuf);

impl Paths for DataDir {
    fn data_dir(&self) -> Result<PathBuf, String> {
        std::fs::create_dir_all(&self.0).map_err(|e| format!("{}: {e}", self.0.display()))?;
        Ok(self.0.clone())
    }
}

/// The built phone page, served from a directory.
struct WebRoot(PathBuf);

impl WebRoot {
    /// The file `path` names under the root, or `None` for a path that would leave it.
    fn file(&self, path: &str) -> Option<PathBuf> {
        let rel = path.strip_prefix('/')?;
        if rel.is_empty()
            || rel.split('/').any(|part| part.is_empty() || part == "." || part == "..")
        {
            return None;
        }
        Some(self.0.join(rel))
    }
}

/// The MIME type of a built asset, by extension; what the app's resolver would say.
fn mime_of(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html",
        Some("js" | "mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json" | "map") => "application/json",
        Some("webmanifest") => "application/manifest+json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("txt") => "text/plain",
        _ => "application/octet-stream",
    }
}

impl Assets for WebRoot {
    fn asset(&self, path: &str) -> Option<Asset> {
        let file = self.file(path)?;
        let bytes = std::fs::read(&file).ok()?;
        Some(Asset {
            bytes,
            mime: mime_of(&file).to_owned(),
        })
    }
}

// --- Running ------------------------------------------------------------------------------------

fn log(msg: &str) {
    eprintln!("sidebar-termd: {msg}");
}

fn log_remote(snap: &RemoteSnapshot) {
    let url = snap
        .url
        .as_deref()
        .unwrap_or("no tailnet URL (Tailscale is not publishing the server)");
    log(&format!(
        "Remote {} on 127.0.0.1:{}; {}; {} paired phone(s), {} connected{}",
        if snap.on { "on" } else { "off" },
        snap.port,
        url,
        snap.devices.len(),
        snap.clients,
        snap.tailscale
            .error
            .as_deref()
            .map(|e| format!("; tailscale: {e}"))
            .unwrap_or_default()
    ));
}

fn pair(remote: &Remote) {
    let pairing = remote.pair_begin();
    log(&format!(
        "pairing code {} (valid ten minutes): {}",
        pairing.code,
        pairing
            .url
            .as_deref()
            .unwrap_or("no URL: the server is not listening")
    ));
}

/// What the log says about an event; nothing for the chatty ones.
fn log_event(event: &Event) {
    if event.name == EVENT_REMOTE {
        if let Ok(snap) = serde_json::from_value::<RemoteSnapshot>(event.payload.clone()) {
            log_remote(&snap);
        }
    } else if event.name == EVENT_SESSION_EXIT {
        if let Ok(exit) = serde_json::from_value::<SessionExit>(event.payload.clone()) {
            log(&format!(
                "Session {} exited ({})",
                exit.session_id,
                exit.code.map_or("signal".to_owned(), |c| format!("code {c}"))
            ));
        }
    } else if event.name == EVENT_LAYOUT {
        if let Ok(snap) = serde_json::from_value::<LayoutSnapshot>(event.payload.clone()) {
            log(&format!(
                "layout: {} Group(s), {} Tab(s), active {}",
                snap.groups.len(),
                snap.tabs.len(),
                snap.active_tab_id.as_deref().unwrap_or("none")
            ));
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
}

/// The Tab a Session belongs to, named as the log names it: its rename, else its cwd.
fn tab_of(layout: &Layout, session_id: u32) -> Option<(String, String)> {
    let snap = layout.snapshot();
    let tab = snap.tabs.values().find(|t| t.session_id == Some(session_id))?;
    let name = tab
        .custom_title
        .clone()
        .or_else(|| layout.fact(session_id).and_then(|f| f.cwd))
        .or_else(|| tab.last_cwd.clone())
        .unwrap_or_else(|| tab.id.clone());
    Some((tab.id.clone(), name))
}

/// One line on the Sessions' memory, when its level changes.
fn log_pressure(p: &HostPressure, layout: &Layout) {
    let limit = p.high.or(p.max).map_or("no limit".to_owned(), gb);
    let top = p.top.first().map_or(String::new(), |t| {
        let name = tab_of(layout, t.session_id).map_or(String::new(), |(_, n)| format!(" ({n})"));
        format!("; most: Session {}{name} {}", t.session_id, gb(t.mem))
    });
    log(&format!(
        "Sessions' memory {:?}: {} of {limit}, swap {}, stalled {:.0}% (all) / {:.0}% (some){top}",
        p.level,
        gb(p.mem),
        gb(p.swap),
        p.full,
        p.some
    ));
}

/// Sample the Sessions' memory for the clients every [`PRESSURE_EVERY`], log each change of
/// level, and sweep away the cgroups of Sessions that are gone.
fn spawn_pressure(cgroups: Arc<Cgroups>, remote: Arc<Remote>, layout: Arc<Layout>, sessions: Arc<SessionManager>) {
    let started = std::thread::Builder::new()
        .name("pressure".into())
        .spawn(move || {
            let mut last = PressureLevel::Ok;
            let mut n: u32 = 0;
            loop {
                if let Some(p) = cgroups.sample() {
                    if p.level != last {
                        log_pressure(&p, &layout);
                        last = p.level;
                    }
                    remote.publish_pressure(&p);
                }
                n = n.wrapping_add(1);
                if n.is_multiple_of(SWEEP_EVERY) {
                    let live: Vec<u32> = sessions.probe_targets().iter().map(|t| t.session_id).collect();
                    cgroups.sweep(&live);
                }
                std::thread::sleep(PRESSURE_EVERY);
            }
        });
    if let Err(e) = started {
        log(&format!("pressure: could not start the thread: {e}"));
    }
}

/// Every [`IDLE_EVERY`], close the Tabs whose Sessions sat idle `after` (`idle.rs`), logging
/// each one's Resume entry first so its work can be picked up again by hand.
fn spawn_idle_close(after: Duration, layout: Arc<Layout>, sessions: Arc<SessionManager>) {
    let after_ms = after.as_millis() as u64;
    let started = std::thread::Builder::new()
        .name("idle-close".into())
        .spawn(move || {
            let mut closer = IdleCloser::default();
            loop {
                std::thread::sleep(IDLE_EVERY);
                let snap = layout.snapshot();
                let live: Vec<(String, _, u64)> = snap
                    .tabs
                    .values()
                    .filter_map(|t| {
                        let sid = t.session_id?;
                        Some((t.id.clone(), layout.fact(sid)?, sessions.last_io(sid)?))
                    })
                    .collect();
                let candidates: Vec<Candidate> = live
                    .iter()
                    .map(|(tab_id, info, last_io)| Candidate { tab_id, info, last_io: *last_io })
                    .collect();
                let close = closer.tick(now_ms(), after_ms, &candidates, snap.active_tab_id.as_deref(), snap.tabs.len());
                for tab_id in close {
                    let Some(sid) = snap.tabs.get(&tab_id).and_then(|t| t.session_id) else { continue };
                    let name = tab_of(&layout, sid).map_or(tab_id.clone(), |(_, n)| n);
                    let resume = sessions
                        .probe_target(sid)
                        .and_then(|t| sidebar_term_core::detect::resume::entry(&tab_id, &t))
                        .map_or("nothing to resume".to_owned(), |e| {
                            format!("resume with `{}` in {}", e.line, e.cwd.as_deref().unwrap_or("its cwd"))
                        });
                    log(&format!(
                        "closing Tab {tab_id} ({name}): idle {:.0} h; {resume}",
                        after.as_secs_f64() / 3600.0
                    ));
                    if let Err(e) = layout.tab_close(&tab_id) {
                        log(&format!("closing Tab {tab_id}: {e}"));
                    }
                }
            }
        });
    if let Err(e) = started {
        log(&format!("idle close: could not start the thread: {e}"));
    }
}

/// While Tailscale does not publish the server (it was not up when the daemon started), ask it
/// again every [`TAILSCALE_EVERY`]; the `remote` event logs the URL once it does.
fn spawn_tailscale_retry(remote: Arc<Remote>) {
    let started = std::thread::Builder::new()
        .name("tailscale-retry".into())
        .spawn(move || loop {
            std::thread::sleep(TAILSCALE_EVERY);
            remote.retry_tailscale();
        });
    if let Err(e) = started {
        log(&format!("tailscale: could not start the retry thread: {e}"));
    }
}

/// Until SIGTERM or SIGINT: log the core's events, and start a pairing on SIGUSR1.
async fn run(mut events: broadcast::Receiver<Event>, remote: &Remote) -> Result<(), String> {
    let mut term = signal(SignalKind::terminate()).map_err(|e| format!("SIGTERM: {e}"))?;
    let mut int = signal(SignalKind::interrupt()).map_err(|e| format!("SIGINT: {e}"))?;
    let mut usr1 = signal(SignalKind::user_defined1()).map_err(|e| format!("SIGUSR1: {e}"))?;
    loop {
        tokio::select! {
            _ = term.recv() => return Ok(()),
            _ = int.recv() => return Ok(()),
            _ = usr1.recv() => pair(remote),
            received = events.recv() => match received {
                Ok(event) => log_event(&event),
                Err(broadcast::error::RecvError::Lagged(n)) => log(&format!("{n} events not logged")),
                Err(broadcast::error::RecvError::Closed) => return Err("the event bus closed".into()),
            },
        }
    }
}

fn main() {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("sidebar-termd: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    if args.help {
        println!("{USAGE}");
        return;
    }
    if args.version {
        println!("sidebar-termd {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let home = std::env::var_os("HOME").map(PathBuf::from);
    let xdg = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let Some(data_dir) = args
        .data_dir
        .or_else(|| default_data_dir(home.as_deref(), xdg.as_deref()))
    else {
        log("no HOME: pass --data-dir");
        std::process::exit(2);
    };
    let web_root = args.web_root.unwrap_or_else(|| data_dir.join("web"));
    // First, before any thread or Session: the daemon moves into its own cgroup.
    let cgroups = if args.cgroups {
        match Cgroups::setup(args.limits) {
            Ok(cg) => {
                let show = |v: Option<u64>| v.map_or("none".to_owned(), gb);
                log(&format!(
                    "cgroups: daemon apart from the Sessions in {}; their limits: high {}, max {}, swap {}",
                    cg.sessions_dir().display(),
                    show(args.limits.high),
                    show(args.limits.max),
                    show(args.limits.swap_max)
                ));
                Some(Arc::new(cg))
            }
            Err(e) => {
                log(&format!("cgroups: not split ({e}); the Sessions share the daemon's cgroup and its limits"));
                None
            }
        }
    } else {
        None
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            log(&format!("could not start the runtime: {e}"));
            std::process::exit(1);
        }
    };
    let (bus, log_reader) = broadcast::channel(256);
    let host = Host {
        events: Arc::new(Bus(bus)),
        paths: Arc::new(DataDir(data_dir.clone())),
        assets: Arc::new(WebRoot(web_root.clone())),
        runtime: runtime.handle().clone(),
    };
    log(&format!(
        "{} starting; data dir {}",
        env!("CARGO_PKG_VERSION"),
        data_dir.display()
    ));
    if !web_root.join("index.html").is_file() {
        log(&format!(
            "no phone page at {} (run `pnpm build` and copy build/ there): pairing and /ws work, /m is 404",
            web_root.display()
        ));
    }

    // The core, wired as the app wires it (src-tauri/src/lib.rs), minus what a headless Host has
    // no use for yet: Activity and Memory Guard (not wired in yet), Usage (app-only), Caffeinate (app-only).
    let taps = Arc::new(Taps::default());
    let sessions = Arc::new(SessionManager::new(taps.clone(), host.events.clone()));
    if let Some(cg) = &cgroups {
        let for_spawn = cg.clone();
        sessions.set_spawn_hook(Box::new(move |id, argv| for_spawn.wrap(id, argv)));
        let for_exit = cg.clone();
        sessions.on_exit(Arc::new(move |id| for_exit.release(id)));
    }
    // Claude Code's hooks, up before the layout spawns the first Session.
    let agents = Agents::start(&host);
    let for_env = agents.clone();
    sessions.set_env_hook(Box::new(move |env| for_env.extend_env(env)));
    let for_exit = agents.clone();
    sessions.on_exit(Arc::new(move |id| for_exit.forget(id)));
    // The Journal: what each Agent session does from here on, kept for Rewind.
    let journal_dir = store::path(&*host.paths, store::JOURNAL)
        .inspect_err(|e| log(&format!("journal: no data dir ({e}); not kept")))
        .ok();
    let journal = Journal::open(journal_dir);
    let for_exit = journal.clone();
    sessions.on_exit(Arc::new(move |id| for_exit.end(id)));
    let for_steps = journal.clone();
    agents.watch_steps(Box::new(move |id, step, at| for_steps.step(id, step, at)));
    journal::spawn(journal.clone());
    // Resume first: what the last run left running becomes leftover before the Tabs respawn.
    let resume_file = store::path(&*host.paths, store::RESUME)
        .inspect_err(|e| log(&format!("resume: no data dir ({e}); not persisted")))
        .ok();
    let resume = Arc::new(Resume::open(resume_file));
    // The layout: Tabs and Groups from this Host's layout.json, each Tab's Session spawned now.
    // No Memory Guard here, so nothing to thaw before a kill.
    let layout = Layout::open(
        host.paths.clone(),
        host.events.clone(),
        sessions.clone(),
        Box::new(|_| {}),
    );
    {
        let snap = layout.snapshot();
        log(&format!(
            "layout: {} Group(s), {} Tab(s) respawned",
            snap.groups.len(),
            snap.tabs.len()
        ));
    }
    let (for_monitor, for_marks, for_observe) = (sessions.clone(), sessions.clone(), layout.clone());
    let for_journal = journal.clone();
    monitor::spawn(
        host.events.clone(),
        agents.clone(),
        move || for_monitor.probe_targets(),
        move |id| for_marks.marks(id),
        move |infos| {
            for_observe.observe(infos);
            for_journal.observe(infos);
        },
    );
    let (for_recorder, recorder) = (sessions.clone(), resume.clone());
    resume::spawn(move || recorder.record(resume::entries(&for_recorder.keyed_targets())));
    let remote_file = store::path(&*host.paths, store::REMOTE)
        .inspect_err(|e| log(&format!("remote: no data dir ({e}); pairings not persisted")))
        .ok();
    // Clients get this Host's layout and each Session's facts from the layout, through Remote.
    let remote = Arc::new(Remote::open(
        host.clone(),
        sessions.clone(),
        taps,
        layout.clone(),
        agents,
        remote_file,
    ));
    if let Some(port) = args.port {
        remote.set_port(port);
    }
    // As `remote_set(true)` in the app: bind the port, then ask Tailscale to publish it. Where
    // it stands is logged from the `remote` event it emits, once `run` reads the bus.
    if let Err(e) = remote.set(true) {
        log(&format!("Remote could not start: {e}"));
        std::process::exit(1);
    }
    if args.pair {
        pair(&remote);
    }
    spawn_tailscale_retry(remote.clone());
    if let Some(cg) = cgroups {
        spawn_pressure(cg, remote.clone(), layout.clone(), sessions.clone());
    }
    let idle_hours = args.close_idle_after.unwrap_or(CLOSE_IDLE_AFTER_HOURS);
    if idle_hours > 0.0 {
        log(&format!("closing Tabs idle {idle_hours} h or more"));
        spawn_idle_close(Duration::from_secs_f64(idle_hours * 3600.0), layout.clone(), sessions.clone());
    }

    let outcome = runtime.block_on(run(log_reader, &remote));
    if let Err(e) = &outcome {
        log(e);
    }
    // As the app on quit: write the layout and the Journal's tallies, record what was running
    // before killing it, so the next run can resume it.
    log("stopping: writing the layout, recording Resume entries, then killing every Session");
    layout.flush();
    journal.finish();
    resume.finish(resume::entries(&sessions.keyed_targets()));
    sessions.kill_all();
    if outcome.is_err() {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        parse_args(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn flags_parse_and_bad_ones_are_refused() {
        assert_eq!(args(&[]).unwrap(), Args::default());
        let all = args(&["--data-dir", "/d", "--port", "5000", "--web-root", "/w", "--pair"]).unwrap();
        assert_eq!(
            all,
            Args {
                data_dir: Some("/d".into()),
                port: Some(5000),
                web_root: Some("/w".into()),
                pair: true,
                ..Args::default()
            }
        );
        let limited = args(&[
            "--cgroups",
            "--sessions-memory-high", "9G",
            "--sessions-memory-max", "10G",
            "--sessions-swap-max", "2G",
            "--close-idle-after", "36",
        ])
        .unwrap();
        assert!(limited.cgroups);
        assert_eq!(
            limited.limits,
            Limits { high: Some(9 << 30), max: Some(10 << 30), swap_max: Some(2 << 30) }
        );
        assert_eq!(limited.close_idle_after, Some(36.0));
        assert!(args(&["--sessions-memory-high", "9G"]).unwrap_err().contains("need --cgroups"));
        assert!(args(&["--cgroups", "--sessions-memory-max", "lots"]).unwrap_err().contains("not a size"));
        assert!(args(&["--close-idle-after", "-1"]).unwrap_err().contains("hours"));
        assert!(args(&["--help"]).unwrap().help);
        assert!(args(&["-V"]).unwrap().version);
        assert!(args(&["--port"]).unwrap_err().contains("needs a value"));
        assert!(args(&["--port", "x"]).unwrap_err().contains("not a port"));
        assert!(args(&["--port", "0"]).unwrap_err().contains("not a port"));
        assert!(args(&["--port", "70000"]).unwrap_err().contains("not a port"));
        assert!(args(&["--bogus"]).unwrap_err().contains("unknown argument"));
    }

    #[test]
    fn the_default_data_dir_is_the_platforms() {
        let home = Path::new("/home/u");
        let dir = default_data_dir(Some(home), None).unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(
                dir,
                Path::new("/home/u/Library/Application Support/com.bencooper.sidebarterm/daemon")
            );
        } else {
            assert_eq!(dir, Path::new("/home/u/.local/share/sidebar-term"));
            assert_eq!(
                default_data_dir(Some(home), Some(Path::new("/xdg"))).unwrap(),
                Path::new("/xdg/sidebar-term")
            );
            // A relative XDG_DATA_HOME is invalid and ignored, as the spec says.
            assert_eq!(
                default_data_dir(Some(home), Some(Path::new("rel"))).unwrap(),
                Path::new("/home/u/.local/share/sidebar-term")
            );
        }
        assert_eq!(default_data_dir(None, None), None);
    }

    #[test]
    fn the_web_root_serves_its_files_and_nothing_outside() {
        let dir = std::env::temp_dir().join(format!("sidebar-termd-web-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("_app")).unwrap();
        std::fs::write(dir.join("index.html"), "<html>").unwrap();
        std::fs::write(dir.join("_app/a.js"), "js").unwrap();
        let root = WebRoot(dir.clone());

        let page = root.asset("/index.html").expect("index");
        assert_eq!(page.bytes, b"<html>");
        assert_eq!(page.mime, "text/html");
        assert_eq!(root.asset("/_app/a.js").unwrap().mime, "text/javascript");
        assert!(root.asset("/missing.js").is_none());
        for bad in ["index.html", "/", "//index.html", "/../index.html", "/_app/../../x", "/./index.html"] {
            assert!(root.file(bad).is_none(), "{bad}");
        }
        assert_eq!(mime_of(Path::new("m.webmanifest")), "application/manifest+json");
        assert_eq!(mime_of(Path::new("x.bin")), "application/octet-stream");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
