//! Resolve repo / Worktree / branch for a directory by reading `.git` files directly
//! (gitfile -> commondir -> HEAD). No git subprocess on the hot path. OWNER: detection agent.
//!
//! Discovery, like `git rev-parse` (docs/research/cwd-git.md section 3):
//!
//! 1. Walk up from the canonical `dir` to the first `.git` entry. A directory is a git dir if
//!    it holds `HEAD` (otherwise keep walking, as git does). A file is a gitfile,
//!    `gitdir: <path>` with `<path>` relative to the file's directory; an unusable gitfile ends
//!    the search with `None` (git errors out there too).
//! 2. `<git dir>/commondir`, if present, is the shared common dir (relative to the git dir);
//!    otherwise common dir = git dir. They differ exactly in a linked Worktree, whose name is
//!    the git dir's basename (`<common>/worktrees/<name>`).
//! 3. `<git dir>/HEAD` is `ref: refs/heads/<branch>` or a detached sha.
//!
//! `repo_name` rule: in a main Worktree (git dir == common dir), the basename of the Worktree
//! root. In a linked Worktree, the basename of the main Worktree, i.e. the parent of the common
//! dir when the common dir is a dot-dir (`.git`, or `.bare` in the bare-repo-plus-worktrees
//! layout); otherwise the common dir's basename minus a `.git` suffix (bare `repo.git` ->
//! `repo`; a submodule's `<super>/.git/modules/<name>` -> `<name>`). Submodules and
//! `--separate-git-dir` checkouts are their own repos, named after their own directory.
//!
//! Ignored on purpose: `GIT_DIR` / `GIT_WORK_TREE` / `core.worktree` (the Badge follows the
//! filesystem), and reftable repos' real HEAD (their `HEAD` file is a `refs/heads/.invalid`
//! stub; branch and sha then read as `None`).

use crate::model::GitInfo;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest `.git` / `HEAD` / `commondir` file we read; real ones are a line.
const MAX_FILE: u64 = 64 * 1024;

/// Repo / Worktree / branch facts for `dir`, or `None` outside a repo. All paths in the result
/// are canonical. Never panics on odd or partial files.
pub fn resolve(dir: &Path) -> Option<GitInfo> {
    let start = fs::canonicalize(dir).ok()?;
    let mut cur = start.as_path();
    loop {
        let dotgit = cur.join(".git");
        if let Ok(md) = fs::metadata(&dotgit) {
            if md.is_file() {
                return read_gitfile(&dotgit, cur).and_then(|git_dir| describe(cur, &git_dir));
            }
            if md.is_dir() {
                if let Some(info) = describe(cur, &dotgit) {
                    return Some(info);
                }
            }
        }
        cur = cur.parent()?;
    }
}

/// Target of a gitfile (`gitdir: <path>`), resolved against the file's directory.
fn read_gitfile(file: &Path, dir: &Path) -> Option<PathBuf> {
    let text = read_small(file)?;
    let target = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    if target.is_empty() {
        return None;
    }
    Some(dir.join(target))
}

fn describe(worktree_root: &Path, git_dir: &Path) -> Option<GitInfo> {
    let git_dir = fs::canonicalize(git_dir).ok()?;
    let head = read_small(&git_dir.join("HEAD"))?;
    let common = read_small(&git_dir.join("commondir"))
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .and_then(|rel| fs::canonicalize(git_dir.join(rel)).ok())
        .unwrap_or_else(|| git_dir.clone());
    let linked = common != git_dir;
    let (branch, head_short) = parse_head(&head);
    Some(GitInfo {
        repo_name: repo_name(&common, worktree_root, linked),
        common_dir: path_string(&common),
        worktree_root: path_string(worktree_root),
        worktree_name: if linked { basename(&git_dir) } else { None },
        branch,
        head_short,
    })
}

/// `(branch, head_short)` from the contents of a `HEAD` file.
fn parse_head(head: &str) -> (Option<String>, Option<String>) {
    let head = head.trim();
    if let Some(target) = head.strip_prefix("ref:") {
        let target = target.trim();
        let branch = target.strip_prefix("refs/heads/").unwrap_or(target);
        if branch.is_empty() || branch == ".invalid" {
            return (None, None);
        }
        return (Some(branch.to_owned()), None);
    }
    if head.len() >= 7 && head.bytes().all(|b| b.is_ascii_hexdigit()) {
        return (None, head.get(..7).map(str::to_owned));
    }
    (None, None)
}

fn repo_name(common: &Path, worktree_root: &Path, linked: bool) -> String {
    let name = if linked {
        match basename(common) {
            Some(b) if b.starts_with('.') => common.parent().and_then(basename),
            Some(b) => Some(
                b.strip_suffix(".git")
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&b)
                    .to_owned(),
            ),
            None => None,
        }
    } else {
        basename(worktree_root)
    };
    name.unwrap_or_else(|| path_string(worktree_root))
}

fn basename(p: &Path) -> Option<String> {
    p.file_name().map(|s| s.to_string_lossy().into_owned())
}

fn path_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Read up to `MAX_FILE` bytes of a small text file, lossily.
fn read_small(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_FILE)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::testutil::{git, init_repo, TempDir};
    use std::process::Command;

    /// What `git` itself says about `dir`, in `GitInfo` terms (everything but `repo_name`).
    fn git_says(dir: &Path) -> (String, String, Option<String>) {
        let canon = |s: String| fs::canonicalize(s).unwrap().to_string_lossy().into_owned();
        let top = canon(git(
            dir,
            &["rev-parse", "--path-format=absolute", "--show-toplevel"],
        ));
        let common = canon(git(
            dir,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        ));
        let branch = git(dir, &["branch", "--show-current"]);
        (top, common, Some(branch).filter(|b| !b.is_empty()))
    }

    fn assert_agrees_with_git(dir: &Path, info: &GitInfo) {
        let (top, common, branch) = git_says(dir);
        assert_eq!(
            info.worktree_root,
            top,
            "worktree_root for {}",
            dir.display()
        );
        assert_eq!(info.common_dir, common, "common_dir for {}", dir.display());
        assert_eq!(info.branch, branch, "branch for {}", dir.display());
    }

    #[test]
    fn main_worktree_on_a_branch_and_subdirectory() {
        let tmp = TempDir::new("git-main");
        let repo = init_repo(tmp.path(), "myrepo");
        fs::create_dir_all(repo.join("a/b")).unwrap();

        let want = GitInfo {
            repo_name: "myrepo".into(),
            common_dir: tmp.canon("myrepo/.git"),
            worktree_root: tmp.canon("myrepo"),
            worktree_name: None,
            branch: Some("main".into()),
            head_short: None,
        };
        assert_eq!(resolve(&repo), Some(want.clone()));
        assert_eq!(resolve(&repo.join("a/b")), Some(want.clone()));
        assert_agrees_with_git(&repo.join("a/b"), &want);
        // The walk starts from the canonical path, so non-canonical input gives canonical output.
        assert_eq!(resolve(&repo.join("a/../a/b/.")).unwrap(), want);
        // Inside the git dir itself: still the same repo.
        assert_eq!(
            resolve(&repo.join(".git/refs")).unwrap().common_dir,
            want.common_dir
        );
    }

    #[test]
    fn linked_worktrees() {
        let tmp = TempDir::new("git-linked");
        let repo = init_repo(tmp.path(), "myrepo");
        git(
            &repo,
            &["worktree", "add", "-q", "-b", "feature/x", "../wt-feature"],
        );
        // Nested inside the main worktree, like `.claude/worktrees/<name>`.
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "agent-1",
                ".claude/worktrees/agent-1",
            ],
        );
        // Relative gitfile / gitdir links (git 2.48+); skip quietly on older git.
        let relative = Command::new("git")
            .args([
                "-c",
                "worktree.useRelativePaths=true",
                "worktree",
                "add",
                "-q",
                "-b",
                "rel",
                "../wt-rel",
            ])
            .current_dir(&repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        let common = tmp.canon("myrepo/.git");
        let wt = tmp.path().join("wt-feature");
        fs::create_dir_all(wt.join("src")).unwrap();
        let info = resolve(&wt.join("src")).unwrap();
        assert_eq!(
            info,
            GitInfo {
                repo_name: "myrepo".into(),
                common_dir: common.clone(),
                worktree_root: tmp.canon("wt-feature"),
                worktree_name: Some("wt-feature".into()),
                branch: Some("feature/x".into()),
                head_short: None,
            }
        );
        assert_agrees_with_git(&wt.join("src"), &info);

        let nested = repo.join(".claude/worktrees/agent-1");
        let info = resolve(&nested).unwrap();
        assert_eq!(info.repo_name, "myrepo");
        assert_eq!(info.common_dir, common);
        assert_eq!(
            info.worktree_root,
            tmp.canon("myrepo/.claude/worktrees/agent-1")
        );
        assert_eq!(info.worktree_name.as_deref(), Some("agent-1"));
        assert_eq!(info.branch.as_deref(), Some("agent-1"));
        assert_agrees_with_git(&nested, &info);

        if relative {
            let rel = tmp.path().join("wt-rel");
            assert!(fs::read_to_string(rel.join(".git"))
                .unwrap()
                .contains("gitdir: ../"));
            let info = resolve(&rel).unwrap();
            assert_eq!(info.common_dir, common);
            assert_eq!(info.worktree_name.as_deref(), Some("wt-rel"));
            assert_agrees_with_git(&rel, &info);
        }

        // The main worktree is unaffected by its linked siblings.
        let main = resolve(&repo).unwrap();
        assert_eq!(main.worktree_name, None);
        assert_eq!(main.common_dir, common);
    }

    #[test]
    fn detached_head() {
        let tmp = TempDir::new("git-detached");
        let repo = init_repo(tmp.path(), "myrepo");
        let sha = git(&repo, &["rev-parse", "HEAD"]);
        git(
            &repo,
            &["worktree", "add", "-q", "--detach", "../wt-detached"],
        );
        git(&repo, &["checkout", "-q", "--detach"]);

        for dir in [repo.clone(), tmp.path().join("wt-detached")] {
            let info = resolve(&dir).unwrap();
            assert_eq!(info.branch, None);
            assert_eq!(info.head_short.as_deref(), Some(&sha[..7]));
            assert_eq!(info.repo_name, "myrepo");
            assert_agrees_with_git(&dir, &info);
        }
    }

    #[test]
    fn non_repo_dir() {
        let tmp = TempDir::new("git-none");
        fs::create_dir_all(tmp.path().join("x/y")).unwrap();
        assert_eq!(resolve(&tmp.path().join("x/y")), None);
        assert_eq!(resolve(&tmp.path().join("missing")), None);
        assert_eq!(resolve(Path::new("")), None);
    }

    #[test]
    fn submodule_is_its_own_repo() {
        let tmp = TempDir::new("git-submodule");
        let lib = init_repo(tmp.path(), "lib");
        let sup = init_repo(tmp.path(), "super");
        git(
            &sup,
            &["submodule", "add", "-q", lib.to_str().unwrap(), "libs/lib"],
        );

        let sub = sup.join("libs/lib");
        let info = resolve(&sub).unwrap();
        assert_eq!(info.repo_name, "lib");
        assert_eq!(info.common_dir, tmp.canon("super/.git/modules/libs/lib"));
        assert_eq!(info.worktree_root, tmp.canon("super/libs/lib"));
        assert_eq!(info.worktree_name, None);
        assert_agrees_with_git(&sub, &info);

        let outer = resolve(&sup.join("libs")).unwrap();
        assert_eq!(outer.repo_name, "super");
        assert_eq!(outer.common_dir, tmp.canon("super/.git"));
    }

    #[test]
    fn bare_repo_with_worktrees_layout() {
        // project/.bare (bare clone) + project/.git ("gitdir: ./.bare") + project/<branch> trees.
        let tmp = TempDir::new("git-bare");
        let src = init_repo(tmp.path(), "src");
        let project = tmp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        git(
            &project,
            &["clone", "-q", "--bare", src.to_str().unwrap(), ".bare"],
        );
        fs::write(project.join(".git"), "gitdir: ./.bare\n").unwrap();
        git(&project, &["worktree", "add", "-q", "main-tree", "main"]);

        let info = resolve(&project.join("main-tree")).unwrap();
        assert_eq!(info.repo_name, "project");
        assert_eq!(info.common_dir, tmp.canon("project/.bare"));
        assert_eq!(info.worktree_name.as_deref(), Some("main-tree"));
        assert_eq!(info.branch.as_deref(), Some("main"));

        // A plain bare repo named `repo.git` with a linked worktree.
        git(
            tmp.path(),
            &["clone", "-q", "--bare", src.to_str().unwrap(), "repo.git"],
        );
        git(
            &tmp.path().join("repo.git"),
            &["worktree", "add", "-q", "../repo-wt", "main"],
        );
        assert_eq!(
            resolve(&tmp.path().join("repo-wt")).unwrap().repo_name,
            "repo"
        );
    }

    #[test]
    fn odd_and_partial_files_never_panic() {
        let tmp = TempDir::new("git-odd");
        let p = tmp.path();
        let case = |name: &str| {
            let d = p.join(name);
            fs::create_dir_all(&d).unwrap();
            d
        };

        // Gitfile variants: garbage, empty, missing target, target without HEAD.
        for (name, body) in [
            ("garbage", "hello world\n"),
            ("empty", ""),
            ("blank-target", "gitdir:   \n"),
            ("missing", "gitdir: /definitely/not/here\n"),
            ("binary", "\u{0}\u{1}\u{ff}"),
        ] {
            let d = case(name);
            fs::write(d.join(".git"), body).unwrap();
            assert_eq!(resolve(&d), None, "{name}");
        }
        let d = case("no-head");
        fs::create_dir_all(d.join("gd")).unwrap();
        fs::write(d.join(".git"), "gitdir: gd").unwrap();
        assert_eq!(resolve(&d), None);

        // A `.git` dir without HEAD is not a repo; the walk continues (and finds nothing here).
        let d = case("empty-dotgit");
        fs::create_dir_all(d.join(".git")).unwrap();
        assert_eq!(resolve(&d), None);

        // HEAD with junk; commondir pointing nowhere falls back to the git dir.
        let d = case("junk-head");
        fs::create_dir_all(d.join(".git")).unwrap();
        fs::write(d.join(".git/HEAD"), "not a ref\n").unwrap();
        fs::write(d.join(".git/commondir"), "../../nowhere\n").unwrap();
        let info = resolve(&d).unwrap();
        assert_eq!((info.branch, info.head_short), (None, None));
        assert_eq!(info.worktree_name, None);
        assert_eq!(info.common_dir, tmp.canon("junk-head/.git"));
        assert_eq!(info.repo_name, "junk-head");

        // A `.git` dir with no HEAD inside a real repo: the walk skips it and finds the outer one.
        let outer = init_repo(p, "outer");
        let inner = outer.join("inner");
        fs::create_dir_all(inner.join(".git")).unwrap();
        assert_eq!(resolve(&inner).unwrap().repo_name, "outer");
    }

    #[test]
    fn parse_head_forms() {
        let sha1 = "3f2a9c1e0b7d4a5f6e8c9b0a1d2e3f4a5b6c7d8e";
        let sha256 = "3f2a9c1e0b7d4a5f6e8c9b0a1d2e3f4a5b6c7d8e3f2a9c1e0b7d4a5f6e8c9b0a";
        let cases: &[(&str, Option<&str>, Option<&str>)] = &[
            ("ref: refs/heads/main\n", Some("main"), None),
            (
                "ref: refs/heads/feature/deep/name",
                Some("feature/deep/name"),
                None,
            ),
            ("ref:refs/heads/x\r\n", Some("x"), None),
            (
                "ref: refs/remotes/origin/main\n",
                Some("refs/remotes/origin/main"),
                None,
            ),
            ("ref: refs/heads/.invalid\n", None, None), // reftable stub
            ("ref: ", None, None),
            (sha1, None, Some("3f2a9c1")),
            (sha256, None, Some("3f2a9c1")),
            ("3f2a9c", None, None),
            ("zzzzzzzzzz", None, None),
            ("", None, None),
            ("é€漢字漢字漢字", None, None),
        ];
        for (head, branch, short) in cases {
            let (b, s) = parse_head(head);
            assert_eq!((b.as_deref(), s.as_deref()), (*branch, *short), "{head:?}");
        }
    }

    /// Read-only sanity check against real repos and linked worktrees on the dev machine.
    /// Run with `cargo test real_worktrees -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_worktrees() {
        let roots = [
            "/Users/bencooper/Dev/jack",
            concat!(env!("CARGO_MANIFEST_DIR"), "/.."),
        ];
        let mut checked = 0;
        for root in roots {
            let out = Command::new("find")
                .args([
                    root,
                    "-maxdepth",
                    "6",
                    "-name",
                    ".git",
                    "-not",
                    "-path",
                    "*/node_modules/*",
                ])
                .output()
                .unwrap();
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let dir = Path::new(line).parent().unwrap();
                let Some(info) = resolve(dir) else {
                    println!("{} -> None", dir.display());
                    continue;
                };
                println!("{} -> {info:?}", dir.display());
                let ok = Command::new("git")
                    .arg("-C")
                    .arg(dir)
                    .args(["rev-parse", "--git-dir"])
                    .output();
                if ok.is_ok_and(|o| o.status.success()) {
                    assert_agrees_with_git(dir, &info);
                    checked += 1;
                }
            }
        }
        println!("checked {checked} worktrees against git");
    }
}
