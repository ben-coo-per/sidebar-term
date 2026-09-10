//! Persistence of the sidebar layout (Groups, Tabs, Titles, order). The webview owns the
//! layout model; Rust only stores the JSON blob in the app data dir.

use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const FILE: &str = "layout.json";

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(FILE))
}

pub fn load(app: &AppHandle) -> Result<Option<serde_json::Value>, String> {
    let p = path(app)?;
    match fs::read_to_string(&p) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => Ok(Some(v)),
            Err(e) => {
                // Keep the bad file for inspection, start fresh.
                let _ = fs::rename(&p, p.with_extension("json.corrupt"));
                eprintln!("layout.json unreadable ({e}); starting with an empty layout");
                Ok(None)
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Atomic write: temp file + rename.
pub fn save(app: &AppHandle, value: &serde_json::Value) -> Result<(), String> {
    let p = path(app)?;
    let tmp = p.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    fs::write(&tmp, body).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &p).map_err(|e| e.to_string())
}
