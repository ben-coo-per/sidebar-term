//! Files dropped onto a Terminal. `dragDropEnabled` is off (Tauri's handler would swallow the
//! sidebar's HTML5 drags), so the webview gets the drop as DOM `File`s, which WebKit strips of
//! their paths. The real paths are still on the macOS drag pasteboard; files that are not there
//! (file promises such as the screenshot thumbnail, images dragged out of a browser) are saved to
//! a temp dir from the bytes the webview read.

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
            .collect()
    }
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
    use super::percent_decode;

    #[test]
    fn decodes_uri_component() {
        assert_eq!(percent_decode("Screen%20Shot%E2%80%AFAM.png"), "Screen Shot\u{202f}AM.png");
        assert_eq!(percent_decode("plain.png"), "plain.png");
        assert_eq!(percent_decode("bad%zz%4"), "bad%zz%4");
    }
}
