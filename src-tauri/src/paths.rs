//! File paths printed in a Terminal. The webview finds text that looks like a path; this checks
//! which of those name a file or directory on this Mac, and opens one in its default app.
//!
//! A relative path is looked up in the Foreground process's cwd, then in the root of the Worktree
//! that cwd is in (git prints repo-relative paths). A remote Session's paths name files on the far
//! side, so none of its paths resolve.

use std::path::{Path, PathBuf};

use crate::model::SessionInfo;

/// Longest candidate looked at; anything longer is not a path someone printed to click on.
const MAX_LEN: usize = 1024;

/// Directories a relative path is tried in, in order.
pub fn bases(info: &SessionInfo) -> Vec<PathBuf> {
    if info.remote {
        return Vec::new();
    }
    let mut bases: Vec<PathBuf> = info.cwd.iter().map(PathBuf::from).collect();
    if let Some(root) = info.git.as_ref().map(|g| PathBuf::from(&g.worktree_root)) {
        if !bases.contains(&root) {
            bases.push(root);
        }
    }
    bases
}

/// The absolute path of an existing file or directory that `candidate` names, or `None`.
/// `~` and `~/...` are the user's home; relative paths are tried in each of `bases`.
pub fn resolve(candidate: &str, bases: &[PathBuf], home: Option<&Path>) -> Option<String> {
    if candidate.is_empty() || candidate.len() > MAX_LEN || bases.is_empty() {
        return None;
    }
    let tried: Vec<PathBuf> = if candidate == "~" {
        home.map(Path::to_path_buf).into_iter().collect()
    } else if let Some(rest) = candidate.strip_prefix("~/") {
        home.map(|h| h.join(rest)).into_iter().collect()
    } else if candidate.starts_with('/') {
        vec![PathBuf::from(candidate)]
    } else {
        bases.iter().map(|b| b.join(candidate)).collect()
    };
    tried
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
}

/// Open an existing file or directory in its default app, as a double-click in Finder does.
pub fn open(path: &str) -> Result<(), String> {
    if !Path::new(path).is_absolute() {
        return Err(format!("not an absolute path: {path}"));
    }
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::testutil::TempDir;
    use crate::model::GitInfo;

    fn git(root: &str) -> GitInfo {
        GitInfo {
            repo_name: "r".into(),
            common_dir: format!("{root}/.git"),
            worktree_root: root.into(),
            worktree_name: None,
            branch: Some("main".into()),
            head_short: None,
        }
    }

    #[test]
    fn bases_are_cwd_then_worktree_root() {
        let mut info = SessionInfo::empty(1);
        info.cwd = Some("/r/src".into());
        info.git = Some(git("/r"));
        assert_eq!(bases(&info), vec![PathBuf::from("/r/src"), PathBuf::from("/r")]);

        info.cwd = Some("/r".into());
        assert_eq!(bases(&info), vec![PathBuf::from("/r")], "no duplicate when cwd is the root");

        info.remote = true;
        assert!(bases(&info).is_empty(), "a remote Session's paths are on the far side");
    }

    #[test]
    fn resolves_existing_paths_only() {
        let tmp = TempDir::new("paths");
        let root = PathBuf::from(tmp.canonical_str());
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "").unwrap();
        std::fs::write(root.join("README.md"), "").unwrap();
        let src = root.join("src");
        let bases = [src.clone(), root.clone()];
        let at = |p: &Path| Some(p.to_string_lossy().into_owned());

        assert_eq!(resolve("main.rs", &bases, None), at(&src.join("main.rs")));
        assert_eq!(resolve("README.md", &bases, None), at(&root.join("README.md")), "repo-relative");
        assert_eq!(resolve("src/main.rs", &bases, None), at(&root.join("src/main.rs")));
        assert_eq!(resolve("../README.md", &bases, None), at(&src.join("../README.md")));
        assert_eq!(resolve(&format!("{}/README.md", root.display()), &[], None), None);
        let abs = root.join("README.md");
        assert_eq!(resolve(abs.to_str().unwrap(), &bases, None), at(&abs));
        assert_eq!(resolve("~/README.md", &bases, Some(&root)), at(&root.join("README.md")));
        assert_eq!(resolve("~", &bases, Some(&root)), at(&root));
        assert_eq!(resolve("~/README.md", &bases, None), None);
        assert_eq!(resolve("missing.md", &bases, None), None);
        assert_eq!(resolve("", &bases, None), None);
    }

    #[test]
    fn opens_only_absolute_paths() {
        assert!(open("README.md").is_err());
    }
}
