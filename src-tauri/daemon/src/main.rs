//! sidebar-termd: the headless Host daemon (ADR 0002). Runs the sidebar-term core on a machine
//! with no display (a Linux box under `systemctl --user`, `packaging/systemd/`), or on a Mac for
//! a smoke test, and serves its Sessions over Remote. See docs/architecture.md "Host daemon".
//!
//! It starts the Session core, the layout (its own `layout.json`: every Tab's Session is spawned
//! at launch, one Tab on a fresh install), the monitor, Resume and the Remote server, exactly as
//! the app does, and stops on SIGTERM after writing the layout, recording Resume entries and
//! killing every Session, as the app does on quit. Phones list its Tabs and drive their
//! Sessions, and create, close, rename and move its Tabs and Groups over the Host protocol.
//! Process facts and Activity on Linux come from `/proc`.
//!
//! Flags: `--data-dir <dir>` (default: see [`default_data_dir`]), `--port <n>` (kept in
//! `remote.json`), `--web-root <dir>` (the built phone page, `pnpm build`'s `build/`; default
//! `<data dir>/web`), `--pair` (start a pairing at launch and print its code). SIGUSR1 starts a
//! pairing at any time: `systemctl --user kill -s USR1 sidebar-termd`, then read the code in the
//! journal. Everything is logged to stderr.

use sidebar_term_core::host::{Asset, Assets, Events, Host, Paths};
use sidebar_term_core::layout::Layout;
use sidebar_term_core::model::{
    LayoutSnapshot, RemoteSnapshot, SessionExit, EVENT_LAYOUT, EVENT_REMOTE, EVENT_SESSION_EXIT,
};
use sidebar_term_core::remote::{Remote, Taps};
use sidebar_term_core::resume::{self, Resume};
use sidebar_term_core::session::SessionManager;
use sidebar_term_core::{monitor, store};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::broadcast;

/// The Mac app's bundle identifier (`tauri.conf.json`), whose app-data dir the daemon's default
/// data dir sits in on a Mac.
#[cfg(target_os = "macos")]
const APP_IDENTIFIER: &str = "com.bencooper.sidebarterm";

const USAGE: &str = "\
usage: sidebar-termd [--data-dir <dir>] [--port <n>] [--web-root <dir>] [--pair]

  --data-dir <dir>   where the Host keeps remote.json, resume.json, ... (default: see README)
  --port <n>         the port the Remote server binds on 127.0.0.1; kept in remote.json
  --web-root <dir>   the built phone page (pnpm build's build/); default <data dir>/web
  --pair             start a pairing at launch and print its code (SIGUSR1 does it any time)
  --help, --version";

#[derive(Debug, Default, PartialEq, Eq)]
struct Args {
    data_dir: Option<PathBuf>,
    port: Option<u16>,
    web_root: Option<PathBuf>,
    pair: bool,
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
            "--help" | "-h" => out.help = true,
            "--version" | "-V" => out.version = true,
            other => return Err(format!("unknown argument {other}")),
        }
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
    monitor::spawn(
        host.events.clone(),
        move || for_monitor.probe_targets(),
        move |id| for_marks.marks(id),
        move |infos| for_observe.observe(infos),
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

    let outcome = runtime.block_on(run(log_reader, &remote));
    if let Err(e) = &outcome {
        log(e);
    }
    // As the app on quit: write the layout, record what was running before killing it, so the
    // next run can resume it.
    log("stopping: writing the layout, recording Resume entries, then killing every Session");
    layout.flush();
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
                help: false,
                version: false,
            }
        );
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
