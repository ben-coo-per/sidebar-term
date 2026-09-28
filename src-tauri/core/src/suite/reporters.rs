//! The reporters and wrappers that stream a test run's progress to its Session's progress
//! directory (`src-tauri/reporters/`, see docs/architecture.md "Suites"). They are compiled into
//! the core and written under the Host's data dir at every launch, so the Mac app and
//! `sidebar-termd` ship the same ones with nothing to bundle.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The directory under the Host's data dir.
pub const DIR: &str = "reporters";

/// Each file: its name, its text, and whether it is run directly (a wrapper) rather than loaded.
const FILES: [(&str, &str, bool); 7] = [
    ("cargo-runner.sh", include_str!("../../../reporters/cargo-runner.sh"), true),
    ("go-exec.sh", include_str!("../../../reporters/go-exec.sh"), true),
    ("jest.cjs", include_str!("../../../reporters/jest.cjs"), false),
    ("mocha.cjs", include_str!("../../../reporters/mocha.cjs"), false),
    ("playwright.cjs", include_str!("../../../reporters/playwright.cjs"), false),
    ("sidebar_progress.py", include_str!("../../../reporters/sidebar_progress.py"), false),
    ("vitest.mjs", include_str!("../../../reporters/vitest.mjs"), false),
];

/// Write every reporter under `data_dir`, replacing what an older version wrote. The directory.
pub fn install(data_dir: &Path) -> Result<PathBuf, String> {
    let dir = data_dir.join(DIR);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for (name, text, run) in FILES {
        let path = dir.join(name);
        fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        let mode = if run { 0o755 } else { 0o644 };
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_every_reporter_with_the_wrappers_runnable() {
        let data = std::env::temp_dir().join(format!("sidebar-term-reporters-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        let dir = install(&data).unwrap();
        for (name, text, run) in FILES {
            let path = dir.join(name);
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o111 != 0, run, "{name}");
        }
        let _ = fs::remove_dir_all(&data);
    }
}
