//! Persistence of the JSON blobs the webview owns: the sidebar layout (Groups, Tabs, Titles,
//! order) and the app settings (Hotkeys). Rust only stores them in the app data dir.

use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub const LAYOUT: &str = "layout.json";
pub const SETTINGS: &str = "settings.json";

fn path(app: &AppHandle, file: &str) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(file))
}

pub fn load(app: &AppHandle, file: &str) -> Result<Option<serde_json::Value>, String> {
    let p = path(app, file)?;
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

/// Atomic write: temp file + rename.
pub fn save(app: &AppHandle, file: &str, value: &serde_json::Value) -> Result<(), String> {
    let p = path(app, file)?;
    let tmp = p.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    fs::write(&tmp, body).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &p).map_err(|e| e.to_string())
}
