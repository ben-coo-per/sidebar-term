//! What a Host does for Handoff (#30, docs/architecture.md "Handoff"): moving a Tab to another
//! Host reruns its Resume entry there, and a Claude Code conversation goes with it as its one
//! transcript file (docs/research/claude-session-portability.md).
//!
//! On the Mac that hands off: the git status of the Session's checkout (one `git` run, so the
//! confirmation can say what will not move), and the conversation's files: where they are
//! (`locate`, while Claude Code still runs, from its own config dir), their contents once the
//! Session is dead (`read`, after the file has stopped changing), and the Mac copy's removal
//! once the Host has it (`forget`: two copies under two non-current keys make `claude --resume`
//! refuse on purpose).
//!
//! On the Host that takes the Tab: `place` writes the transcript (and the project's `memory/`)
//! under this Host's Claude config dir, `<config dir>/projects/<key of the checkout>/`, where
//! `claude --resume <id>` run from that checkout finds it first. It never writes anywhere else:
//! the session id must be a uuid-shaped id, memory names must be plain relative paths, and the
//! destination is checked to sit under `projects/`.

use crate::model::{ClaudeConversation, ConversationFile, ConversationFiles, GitStatus};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Claude Code's transcripts live here, under its config dir.
const PROJECTS: &str = "projects";
/// Auto memory beside a project's transcripts (`MEMORY.md` and notes).
const MEMORY: &str = "memory";
/// The largest memory file shipped (they are notes; anything bigger is not one).
const MEMORY_FILE_MAX: u64 = 4 * 1024 * 1024;
/// The largest transcript shipped.
const TRANSCRIPT_MAX: u64 = 256 * 1024 * 1024;
/// `read` waits until the transcript has been unchanged this long (Claude Code appends a line
/// at a time; the dying process may still be writing one), and gives up waiting after `SETTLE_MAX`.
const SETTLE_QUIET: Duration = Duration::from_millis(250);
const SETTLE_MAX: Duration = Duration::from_secs(3);

/// The project directory name Claude Code derives from a working directory: every character
/// that is not a letter or digit becomes `-` (`/Users/you/Dev/x` -> `-Users-you-Dev-x`). Claude
/// Code takes the path as its process sees it (symlinks resolved), so pass a canonical path
/// where the directory exists. Names over 200 characters get a hash appended by Claude Code;
/// not reproduced here, and harmless: `--resume` searches every project directory anyway.
pub fn project_key(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// A uuid in practice; what `detect::resume` accepts as a session id: nothing that needs
/// quoting or could be read as a flag or a path.
pub fn is_session_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A memory file's name as it may be shipped and placed: a relative path of plain components
/// (no `..`, `.`, empty parts, absolute paths, or NUL), at most 8 deep.
pub fn is_memory_name(name: &str) -> bool {
    let parts: Vec<&str> = name.split('/').collect();
    !name.is_empty()
        && !name.contains('\0')
        && parts.len() <= 8
        && parts
            .iter()
            .all(|part| !part.is_empty() && *part != "." && *part != "..")
}

/// `$CLAUDE_CONFIG_DIR`, else `~/.claude`, as this process sees them (the Host's own).
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".claude"))
}

/// The key of `cwd` as Claude Code would derive it here: from the canonical path when the
/// directory exists, else from the path as given.
fn key_of(cwd: &str) -> String {
    let canonical = fs::canonicalize(cwd)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| cwd.to_owned());
    project_key(&canonical)
}

// ---------------------------------------------------------------------------------------------
// The Mac that hands off
// ---------------------------------------------------------------------------------------------

/// Where the conversation `id` started in `cwd` keeps its transcript under `config_dir`:
/// `projects/<key of cwd>/<id>.jsonl`, or, when it is not there (the conversation moved with
/// `/cd`, or started elsewhere), the one other project directory holding it. `memory` is the
/// project's `memory/` directory beside the transcript, when there is one.
pub fn locate(config_dir: &Path, cwd: &str, id: &str) -> Option<ClaudeConversation> {
    if !is_session_id(id) {
        return None;
    }
    let projects = config_dir.join(PROJECTS);
    let file = format!("{id}.jsonl");
    let mut dir = projects.join(key_of(cwd));
    if !dir.join(&file).is_file() {
        let found: Vec<PathBuf> = fs::read_dir(&projects)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.join(&file).is_file())
            .collect();
        dir = match found.as_slice() {
            [one] => one.clone(),
            _ => return None,
        };
    }
    let memory = dir.join(MEMORY);
    Some(ClaudeConversation {
        id: id.to_owned(),
        cwd: cwd.to_owned(),
        transcript: dir.join(file).to_string_lossy().into_owned(),
        memory: memory
            .is_dir()
            .then(|| memory.to_string_lossy().into_owned()),
    })
}

/// The transcript's text and the memory files' (relative name, text), once the transcript has
/// stopped changing. A memory file that is not UTF-8 text, or too big, is left out.
pub fn read(conversation: &ClaudeConversation) -> Result<ConversationFiles, String> {
    let transcript = Path::new(&conversation.transcript);
    check_transcript_path(transcript, &conversation.id)?;
    settle(transcript);
    let meta = fs::metadata(transcript).map_err(|e| format!("{}: {e}", transcript.display()))?;
    if meta.len() > TRANSCRIPT_MAX {
        return Err(format!(
            "the transcript is {} MB, more than Handoff ships",
            meta.len() / (1024 * 1024)
        ));
    }
    let text = fs::read_to_string(transcript)
        .map_err(|e| format!("reading {}: {e}", transcript.display()))?;
    let mut memory = Vec::new();
    if let Some(dir) = conversation.memory.as_deref().map(Path::new) {
        collect_memory(dir, dir, &mut memory);
    }
    memory.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ConversationFiles {
        transcript: text,
        memory,
    })
}

/// Delete the Mac's copy of the transcript, once the Host has it. Only ever a
/// `projects/<key>/<id>.jsonl` file; the memory directory stays (other Sessions of the repo
/// read it).
pub fn forget(conversation: &ClaudeConversation) -> Result<(), String> {
    let transcript = Path::new(&conversation.transcript);
    check_transcript_path(transcript, &conversation.id)?;
    match fs::remove_file(transcript) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("removing {}: {e}", transcript.display())),
    }
}

/// `path` must be `<something>/projects/<key>/<id>.jsonl` with a valid id: what `locate`
/// produces, and nothing a client could point elsewhere.
fn check_transcript_path(path: &Path, id: &str) -> Result<(), String> {
    let ok = is_session_id(id)
        && path.is_absolute()
        && path.file_name().and_then(|f| f.to_str()) == Some(&format!("{id}.jsonl"))
        && path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|f| f.to_str())
            == Some(PROJECTS)
        && !path.components().any(|c| c == Component::ParentDir);
    if ok {
        Ok(())
    } else {
        Err(format!("not a transcript path: {}", path.display()))
    }
}

/// Wait until `path`'s size and mtime have held still for `SETTLE_QUIET`, at most `SETTLE_MAX`.
fn settle(path: &Path) {
    let stamp = |p: &Path| fs::metadata(p).ok().map(|m| (m.len(), m.modified().ok()));
    let start = Instant::now();
    let mut last = stamp(path);
    let mut since = Instant::now();
    while start.elapsed() < SETTLE_MAX {
        std::thread::sleep(Duration::from_millis(50));
        let now = stamp(path);
        if now != last {
            last = now;
            since = Instant::now();
        } else if since.elapsed() >= SETTLE_QUIET {
            return;
        }
    }
}

fn collect_memory(root: &Path, dir: &Path, out: &mut Vec<ConversationFile>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            collect_memory(root, &path, out);
            continue;
        }
        if !meta.is_file() || meta.len() > MEMORY_FILE_MAX {
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else { continue };
        let name = rel.to_string_lossy().into_owned();
        if !is_memory_name(&name) {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&path) {
            out.push(ConversationFile { name, content });
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The Host that takes the Tab
// ---------------------------------------------------------------------------------------------

/// Write the conversation `id` for the checkout `cwd` under `config_dir`:
/// `projects/<key of cwd>/<id>.jsonl` and `projects/<key>/memory/<name>` for each memory file.
/// Returns the transcript's path. Refuses a bad id, a relative `cwd`, or a memory name that is
/// not a plain relative path; every write is checked to land under `projects/`. A copy of the
/// same id under another key here is removed, so `--resume <id>` finds exactly one.
pub fn place(
    config_dir: &Path,
    cwd: &str,
    id: &str,
    files: &ConversationFiles,
) -> Result<PathBuf, String> {
    if !is_session_id(id) {
        return Err(format!("not a session id: {id:?}"));
    }
    if !Path::new(cwd).is_absolute() || cwd.contains('\0') {
        return Err(format!("the checkout must be an absolute path: {cwd:?}"));
    }
    for f in &files.memory {
        if !is_memory_name(&f.name) {
            return Err(format!("not a memory file name: {:?}", f.name));
        }
    }
    let projects = config_dir.join(PROJECTS);
    fs::create_dir_all(&projects).map_err(|e| format!("{}: {e}", projects.display()))?;
    let dir = projects.join(key_of(cwd));
    let transcript = dir.join(format!("{id}.jsonl"));
    write_inside(&projects, &transcript, files.transcript.as_bytes())?;
    for f in &files.memory {
        write_inside(&projects, &dir.join(MEMORY).join(&f.name), f.content.as_bytes())?;
    }
    drop_other_copies(&projects, &dir, id);
    Ok(transcript)
}

/// Remove `<other key>/<id>.jsonl` under `projects` (an earlier Handoff of the same
/// conversation to another checkout here): one copy of an id per config dir, since two copies
/// under two keys that are not the current one make `claude --resume <id>` refuse on purpose.
fn drop_other_copies(projects: &Path, keep: &Path, id: &str) {
    let file = format!("{id}.jsonl");
    let Ok(entries) = fs::read_dir(projects) else { return };
    for entry in entries.flatten() {
        let dir = entry.path();
        if dir == keep || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let copy = dir.join(&file);
        if fs::symlink_metadata(&copy).is_ok_and(|m| m.is_file()) {
            let _ = fs::remove_file(copy);
        }
    }
}

/// Write `path` only if it sits under `root`: no `..`, no symbolic link anywhere below `root`
/// (a directory or the file itself; checked before any directory is made), and its parent,
/// canonicalised, inside `root` canonicalised.
fn write_inside(root: &Path, path: &Path, bytes: &[u8]) -> Result<(), String> {
    let outside = || format!("{} is outside {}", path.display(), root.display());
    let rel = path.strip_prefix(root).map_err(|_| outside())?;
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(outside());
    }
    let mut at = root.to_path_buf();
    for part in rel.components() {
        at.push(part);
        if fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!("{} is a symbolic link; not followed", at.display()));
        }
    }
    let parent = path.parent().ok_or("no parent")?;
    fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let root = fs::canonicalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let parent = fs::canonicalize(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    if !parent.starts_with(&root) || path.components().any(|c| c == Component::ParentDir) {
        return Err(format!("{} is outside {}", path.display(), root.display()));
    }
    fs::write(path, bytes).map_err(|e| format!("writing {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------------------------
// The checkout's git status, for the confirmation
// ---------------------------------------------------------------------------------------------

/// What will not move with the Tab: uncommitted changes and unpushed commits in the checkout
/// holding `dir`, plus what a clone on the Host would need (`origin`'s URL). One `git status`
/// run and one `git remote`; `Err` when `dir` is not in a repo or git is missing.
pub fn git_status(dir: &str) -> Result<GitStatus, String> {
    let out = git(dir, &["status", "--porcelain=v1", "--branch", "--untracked-files=normal"])?;
    let mut status = parse_status(&out);
    status.remote_url = git(dir, &["remote", "get-url", "origin"])
        .ok()
        .map(|u| u.trim().to_owned())
        .filter(|u| !u.is_empty());
    Ok(status)
}

fn git(dir: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `git status --porcelain=v1 --branch`: a `## <branch>...<upstream> [ahead N, behind M]` line,
/// then one line per changed or untracked path.
fn parse_status(out: &str) -> GitStatus {
    let mut status = GitStatus {
        branch: None,
        upstream: None,
        ahead: 0,
        changes: 0,
        remote_url: None,
    };
    for line in out.lines() {
        if let Some(head) = line.strip_prefix("## ") {
            let (names, tracking) = match head.split_once(" [") {
                Some((n, t)) => (n, Some(t.trim_end_matches(']'))),
                None => (head, None),
            };
            let (branch, upstream) = match names.split_once("...") {
                Some((b, u)) => (b, Some(u)),
                None => (names, None),
            };
            // `## HEAD (no branch)` when detached; `## No commits yet on main` on an empty repo.
            if !branch.starts_with("HEAD") && !branch.starts_with("No commits") {
                status.branch = Some(branch.to_owned());
            }
            status.upstream = upstream.map(str::to_owned);
            if let Some(t) = tracking {
                status.ahead = t
                    .split(", ")
                    .find_map(|part| part.strip_prefix("ahead ")?.parse().ok())
                    .unwrap_or(0);
            }
        } else if !line.is_empty() {
            status.changes += 1;
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::testutil::{git as tgit, init_repo, TempDir};

    #[test]
    fn the_project_key_is_claude_codes() {
        assert_eq!(project_key("/Users/you/Dev/sidebar-term"), "-Users-you-Dev-sidebar-term");
        assert_eq!(project_key("/private/tmp/x/-Users-y"), "-private-tmp-x--Users-y");
        assert_eq!(project_key("/home/ben/Dev/a.b_c"), "-home-ben-Dev-a-b-c");
    }

    #[test]
    fn ids_and_memory_names_are_checked() {
        assert!(is_session_id("e4a3ccd1-89b2-4b2d-b752-1cfe0b6a02ae"));
        for bad in ["", "--help", "a/b", "a b", "$(x)", "../x"] {
            assert!(!is_session_id(bad), "{bad}");
        }
        for good in ["MEMORY.md", "notes/api.md", "a-b_c.txt"] {
            assert!(is_memory_name(good), "{good}");
        }
        for bad in ["", "/abs", "../x", "a/../b", "./x", "a//b", "a\0b", "1/2/3/4/5/6/7/8/9"] {
            assert!(!is_memory_name(bad), "{bad:?}");
        }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn a_conversation_is_found_under_its_key_or_the_one_other_project_holding_it() {
        let tmp = TempDir::new("handoff-locate");
        let config = tmp.path().join("config");
        let checkout = tmp.path().join("repo");
        fs::create_dir_all(&checkout).unwrap();
        let cwd = tmp.canon("repo");
        let key = project_key(&cwd);
        let id = "5b6d103b-2842-4d0a-9e1f-000000000001";
        assert!(locate(&config, &cwd, id).is_none(), "nothing yet");

        // Under another key only: found there.
        write(&config.join("projects/-elsewhere").join(format!("{id}.jsonl")), "{}\n");
        let c = locate(&config, &cwd, id).unwrap();
        assert!(c.transcript.ends_with(&format!("/projects/-elsewhere/{id}.jsonl")));
        assert_eq!(c.memory, None);

        // Under its own key too: that one wins, with its memory beside it.
        write(&config.join("projects").join(&key).join(format!("{id}.jsonl")), "{}\n");
        write(&config.join("projects").join(&key).join("memory/MEMORY.md"), "# m\n");
        let c = locate(&config, &cwd, id).unwrap();
        assert!(c.transcript.ends_with(&format!("/projects/{key}/{id}.jsonl")));
        assert!(c.memory.as_deref().unwrap().ends_with(&format!("/projects/{key}/memory")));

        // Two other projects and none current: ambiguous, as for `claude --resume`.
        fs::remove_file(config.join("projects").join(&key).join(format!("{id}.jsonl"))).unwrap();
        write(&config.join("projects/-third").join(format!("{id}.jsonl")), "{}\n");
        assert!(locate(&config, &cwd, id).is_none());
        assert!(locate(&config, &cwd, "../x").is_none());
    }

    #[test]
    fn read_ships_the_transcript_and_memory_and_forget_removes_only_the_transcript() {
        let tmp = TempDir::new("handoff-read");
        let config = tmp.path().join("config");
        let cwd = tmp.canonical_str().to_owned();
        let id = "5b6d103b-2842-4d0a-9e1f-000000000002";
        let dir = config.join("projects").join(project_key(&cwd));
        write(&dir.join(format!("{id}.jsonl")), "{\"a\":1}\n{\"b\":2}\n");
        write(&dir.join("memory/MEMORY.md"), "# memory\n");
        write(&dir.join("memory/notes/one.md"), "one\n");
        fs::write(dir.join("memory/blob.bin"), [0xff, 0xfe, 0x00]).unwrap();
        let c = locate(&config, &cwd, id).unwrap();
        let files = read(&c).unwrap();
        assert_eq!(files.transcript, "{\"a\":1}\n{\"b\":2}\n");
        assert_eq!(
            files.memory.iter().map(|f| (f.name.as_str(), f.content.as_str())).collect::<Vec<_>>(),
            [("MEMORY.md", "# memory\n"), ("notes/one.md", "one\n")]
        );
        forget(&c).unwrap();
        assert!(!dir.join(format!("{id}.jsonl")).exists());
        assert!(dir.join("memory/MEMORY.md").exists());
        forget(&c).unwrap(); // already gone: fine

        // Nothing but a transcript path is ever removed.
        let stray = ClaudeConversation {
            id: id.into(),
            cwd: cwd.clone(),
            transcript: tmp.path().join("other.jsonl").to_string_lossy().into_owned(),
            memory: None,
        };
        assert!(forget(&stray).is_err());
        let wrong_name = ClaudeConversation {
            transcript: dir.join("nope.jsonl").to_string_lossy().into_owned(),
            ..stray.clone()
        };
        assert!(read(&wrong_name).is_err());
    }

    #[test]
    fn place_writes_under_the_checkouts_key_and_nowhere_else() {
        let tmp = TempDir::new("handoff-place");
        let config = tmp.path().join("config");
        let checkout = tmp.path().join("srv/repo");
        fs::create_dir_all(&checkout).unwrap();
        let cwd = tmp.canon("srv/repo");
        let id = "5b6d103b-2842-4d0a-9e1f-000000000003";
        let files = ConversationFiles {
            transcript: "{\"x\":1}\n".into(),
            memory: vec![
                ConversationFile { name: "MEMORY.md".into(), content: "m\n".into() },
                ConversationFile { name: "notes/a.md".into(), content: "a\n".into() },
            ],
        };
        let path = place(&config, &cwd, id, &files).unwrap();
        let dir = config.join("projects").join(project_key(&cwd));
        assert_eq!(path, dir.join(format!("{id}.jsonl")));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"x\":1}\n");
        assert_eq!(fs::read_to_string(dir.join("memory/MEMORY.md")).unwrap(), "m\n");
        assert_eq!(fs::read_to_string(dir.join("memory/notes/a.md")).unwrap(), "a\n");
        // The path as given, not canonical, still lands under the canonical key.
        let via_symlink = tmp.path().join("srv/repo").to_string_lossy().into_owned();
        assert_eq!(place(&config, &via_symlink, id, &files).unwrap(), path);
        // A checkout that does not exist yet keys by the path as given; placing it there
        // drops the copy placed for the first checkout (one copy of an id per config dir).
        let missing = "/nonexistent/srv/other";
        let p = place(&config, missing, id, &files).unwrap();
        assert!(p.starts_with(config.join("projects").join(project_key(missing))));
        assert!(!path.exists(), "the earlier copy is gone");
        assert!(dir.join("memory/MEMORY.md").exists(), "memory stays");
        place(&config, &cwd, id, &files).unwrap();
        assert!(!p.exists() && path.exists());

        assert!(place(&config, "relative/path", id, &files).is_err());
        assert!(place(&config, &cwd, "../../etc/passwd", &files).is_err());
        assert!(place(&config, &cwd, "id with space", &files).is_err());
        let traversal = ConversationFiles {
            transcript: "x".into(),
            memory: vec![ConversationFile { name: "../../escape.md".into(), content: "x".into() }],
        };
        assert!(place(&config, &cwd, id, &traversal).is_err());
        assert!(!config.join("escape.md").exists());
        assert!(!config.join("projects/escape.md").exists());

        // A symbolic link under projects/ is never followed: not the key's directory, not a
        // memory directory, not the transcript itself.
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        let other = "/srv/linked";
        let linked_dir = config.join("projects").join(project_key(other));
        std::os::unix::fs::symlink(&outside, &linked_dir).unwrap();
        assert!(place(&config, other, id, &files).is_err());
        let dir = config.join("projects").join(project_key(&cwd));
        fs::remove_file(dir.join(format!("{id}.jsonl"))).unwrap();
        std::os::unix::fs::symlink(outside.join("t.jsonl"), dir.join(format!("{id}.jsonl"))).unwrap();
        assert!(place(&config, &cwd, id, &files).is_err());
        fs::remove_file(dir.join(format!("{id}.jsonl"))).unwrap();
        fs::remove_dir_all(dir.join("memory/notes")).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("memory/notes")).unwrap();
        assert!(place(&config, &cwd, id, &files).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0, "nothing written outside");
    }

    #[test]
    fn git_status_counts_changes_and_unpushed_commits() {
        let tmp = TempDir::new("handoff-git");
        let origin = init_repo(tmp.path(), "origin");
        tgit(tmp.path(), &["clone", "-q", origin.to_str().unwrap(), "work"]);
        let work = tmp.path().join("work");
        let dir = work.to_str().unwrap();

        let clean = git_status(dir).unwrap();
        assert_eq!(clean.branch.as_deref(), Some("main"));
        assert_eq!(clean.upstream.as_deref(), Some("origin/main"));
        assert_eq!((clean.ahead, clean.changes), (0, 0));
        assert_eq!(clean.remote_url.as_deref(), Some(origin.to_str().unwrap()));

        fs::write(work.join("new.txt"), "x").unwrap();
        fs::write(work.join("also.txt"), "y").unwrap();
        tgit(&work, &["add", "also.txt"]);
        tgit(&work, &["commit", "-q", "-m", "one"]);
        let dirty = git_status(dir).unwrap();
        assert_eq!((dirty.ahead, dirty.changes), (1, 1));

        tgit(&work, &["switch", "-q", "-c", "feat"]);
        let no_upstream = git_status(dir).unwrap();
        assert_eq!(no_upstream.branch.as_deref(), Some("feat"));
        assert_eq!(no_upstream.upstream, None);
        assert_eq!(no_upstream.ahead, 0);

        tgit(&work, &["switch", "-q", "--detach"]);
        assert_eq!(git_status(dir).unwrap().branch, None);
        assert!(git_status(tmp.path().to_str().unwrap()).is_err(), "not a repo");
    }

    #[test]
    fn status_lines_parse() {
        let s = parse_status("## main...origin/main [ahead 2, behind 1]\n M a.rs\n?? b\n");
        assert_eq!((s.branch.as_deref(), s.upstream.as_deref(), s.ahead, s.changes), (Some("main"), Some("origin/main"), 2, 2));
        let s = parse_status("## HEAD (no branch)\n");
        assert_eq!((s.branch, s.ahead, s.changes), (None, 0, 0));
        let s = parse_status("## feat\n");
        assert_eq!((s.branch.as_deref(), s.upstream), (Some("feat"), None));
    }
}
