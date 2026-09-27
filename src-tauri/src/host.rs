//! The app's answer to `sidebar_term_core::host` (ADR 0002): events go to the webview through
//! Tauri, the core's files live in Tauri's app-data dir, the phone's page is the bundled
//! frontend, and the Remote server runs on Tauri's tokio runtime.

use sidebar_term_core::host::{Asset, Assets, Events, Host, Paths};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// One `AppHandle` behind every trait the core needs.
pub struct AppHost(pub AppHandle);

impl Events for AppHost {
    fn emit_value(&self, name: &str, payload: serde_json::Value) {
        if let Err(e) = self.0.emit(name, payload) {
            eprintln!("{name}: emit failed: {e}");
        }
    }
}

impl Paths for AppHost {
    fn data_dir(&self) -> Result<PathBuf, String> {
        let dir = self.0.path().app_data_dir().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir)
    }
}

impl Assets for AppHost {
    fn asset(&self, path: &str) -> Option<Asset> {
        let asset = self.0.asset_resolver().get(path.to_string())?;
        Some(Asset {
            bytes: asset.bytes,
            mime: asset.mime_type,
        })
    }
}

/// The `Host` the core runs with in the app.
pub fn host(app: &AppHandle) -> Host {
    let shared = Arc::new(AppHost(app.clone()));
    Host {
        events: shared.clone(),
        paths: shared.clone(),
        assets: shared,
        runtime: tauri::async_runtime::handle().inner().clone(),
    }
}
