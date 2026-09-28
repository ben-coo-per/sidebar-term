//! The Host's files: atomic JSON read and write in its data dir (`host::Paths`: the app-data dir
//! in the app, `$XDG_DATA_HOME/sidebar-term` for the daemon). `layout.json` is the core's own
//! (`layout/`); `settings.json` is the webview's, stored opaque; the rest belong to the modules
//! named below.

use crate::host::Paths;
use std::fs;
use std::path::{Path, PathBuf};

/// Groups, Tabs and the active Tab (`layout/file.rs`).
pub const LAYOUT: &str = "layout.json";
/// The webview's settings, one section per owner; opaque to the core except for the one-time
/// migration in `layout/file.rs`.
pub const SETTINGS: &str = "settings.json";
/// What each Session was running, for Resume (`resume.rs`).
pub const RESUME: &str = "resume.json";
/// Whether Remote is on and the phones paired with it (`remote/`).
pub const REMOTE: &str = "remote.json";
/// Processes Memory Guard has frozen, for the next launch to thaw after a crash (`guard.rs`).
pub const FROZEN: &str = "frozen.json";
/// Claude Code's last usage answer and when it was read, so a launch shows it without a request
/// (`usage.rs`).
pub const USAGE: &str = "usage.json";
/// The directory of the Journal: what every Agent session did, a file per month (`journal.rs`).
pub const JOURNAL: &str = "journal";

pub fn path(paths: &dyn Paths, file: &str) -> Result<PathBuf, String> {
    Ok(paths.data_dir()?.join(file))
}

pub fn load(paths: &dyn Paths, file: &str) -> Result<Option<serde_json::Value>, String> {
    let p = path(paths, file)?;
    match fs::read_to_string(&p) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => Ok(Some(v)),
            Err(e) => {
                // Keep the bad file for inspection, start fresh.
                let _ = fs::rename(&p, p.with_extension("json.corrupt"));
                eprintln!("{file} unreadable ({e}); starting fresh");
                Ok(None)
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn save(paths: &dyn Paths, file: &str, value: &serde_json::Value) -> Result<(), String> {
    write(&path(paths, file)?, value)
}

/// Atomic write: temp file + rename.
pub fn write(p: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let tmp = p.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    fs::write(&tmp, body).map_err(|e| e.to_string())?;
    fs::rename(&tmp, p).map_err(|e| e.to_string())
}
