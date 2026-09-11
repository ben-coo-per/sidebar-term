//! Files dropped onto a Terminal. `dragDropEnabled` is off (Tauri's handler would swallow the
//! sidebar's HTML5 drags), so the webview gets the drop as DOM `File`s, which WebKit strips of
//! their paths. The real paths are still on the macOS drag pasteboard; files that are not there
//! (file promises such as the screenshot thumbnail, images dragged out of a browser) are saved to
//! a temp dir from the bytes the webview read. So are files whose pasteboard path is no use to a
//! shell (see `pasteable`): the screenshot thumbnail also puts its own staging copy there.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::ipc::{InvokeBody, Request};

/// File paths on the drag pasteboard: the files of the most recent drag, if it carried any.
#[cfg(target_os = "macos")]
pub fn pasteboard_paths() -> Vec<String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardNameDrag, NSPasteboardTypeFileURL};
    use objc2_foundation::NSURL;

    // SAFETY: plain AppKit calls on retained objects.
    unsafe {
        let pb = NSPasteboard::pasteboardWithName(NSPasteboardNameDrag);
        let Some(items) = pb.pasteboardItems() else {
            return Vec::new();
        };
        items
            .iter()
            .filter_map(|item| item.stringForType(NSPasteboardTypeFileURL))
            .filter_map(|url| NSURL::URLWithString(&url))
            .filter_map(|url| url.path())
            .map(|path| path.to_string())
            .filter(|path| pasteable(Path::new(path)))
            .collect()
    }
}

/// Whether a path from the drag pasteboard can be pasted as is: the app can open it, and it is not
/// in a `TemporaryItems` staging directory. A file promise's provider stages the file there while
/// it is in flight: the screenshot thumbnail's drag carries a URL to
/// `$TMPDIR/TemporaryItems/NSIRD_screencaptureui_*/Screenshot ….png`, which macOS keeps other
/// processes from reading (Claude Code pasted it as text) and which goes once the drop is done.
/// The webview then saves the file from the bytes WebKit received for the promise instead.
fn pasteable(path: &Path) -> bool {
    !staged(path) && std::fs::File::open(path).is_ok()
}

fn staged(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == "TemporaryItems")
}

#[cfg(not(target_os = "macos"))]
pub fn pasteboard_paths() -> Vec<String> {
    Vec::new()
}

/// Save one dropped file's bytes (raw invoke body) under the temp dir, keeping its name so the
/// extension survives, and return the path. The name arrives URI-encoded in `x-file-name`.
pub fn save(request: Request<'_>) -> Result<String, String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected raw file bytes".into());
    };
    let name = request
        .headers()
        .get("x-file-name")
        .and_then(|v| v.to_str().ok())
        .map(percent_decode)
        .unwrap_or_default();
    // Only the final component: a name must not escape the drop dir.
    let name = Path::new(&name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "dropped-file".into());

    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir: PathBuf = std::env::temp_dir().join("sidebar-term-drops").join(nanos.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

/// Decode `encodeURIComponent` output (UTF-8 percent escapes). Malformed escapes pass through.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{pasteable, percent_decode, staged};
    use std::path::Path;

    #[test]
    fn only_readable_paths_outside_staging_dirs_are_pasted() {
        let dir = std::env::temp_dir().join(format!("sidebar-term-pasteable-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Screenshot\u{202f}PM.png");
        std::fs::write(&file, b"png").unwrap();

        assert!(pasteable(&file));
        assert!(pasteable(&dir), "a dropped folder");
        assert!(!pasteable(&dir.join("gone.png")));
        assert!(!pasteable(Path::new("/var/root/.profile")), "another user's file");
        std::fs::remove_dir_all(&dir).unwrap();

        // Checked on the path alone: macOS will not let a test delete a `TemporaryItems` dir.
        let thumbnail = "/var/folders/xf/abc/T/TemporaryItems/NSIRD_screencaptureui_wOXp1M/Screenshot 2026-09-11 at 1.30.41\u{202f}PM.png";
        assert!(staged(Path::new(thumbnail)));
        assert!(!staged(Path::new("/Users/you/Downloads/Screenshot.png")));
        assert!(!staged(Path::new("/Users/you/TemporaryItemsBackup/x.png")));
    }

    #[test]
    fn decodes_uri_component() {
        assert_eq!(percent_decode("Screen%20Shot%E2%80%AFAM.png"), "Screen Shot\u{202f}AM.png");
        assert_eq!(percent_decode("plain.png"), "plain.png");
        assert_eq!(percent_decode("bad%zz%4"), "bad%zz%4");
    }
}
