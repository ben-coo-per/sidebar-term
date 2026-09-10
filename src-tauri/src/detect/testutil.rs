//! Test helpers for `detect`: self-cleaning temp dirs, child processes in their own process
//! group, and a hermetic `git` runner.

use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// A fresh directory under `$TMPDIR` (a symlinked path on macOS: `/var/...` is
/// `/private/var/...`), removed on drop.
pub struct TempDir {
    path: PathBuf,
    canonical: String,
}

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("sidebar-term-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        let canonical = fs::canonicalize(&path)
            .expect("canonicalize temp dir")
            .to_string_lossy()
            .into_owned();
        Self { path, canonical }
    }

    /// The path as created (may go through a symlink).
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn canonical_str(&self) -> &str {
        &self.canonical
    }

    /// Canonical path of `rel` inside this dir (which must exist).
    pub fn canon(&self, rel: &str) -> String {
        fs::canonicalize(self.path.join(rel))
            .expect("canonicalize")
            .to_string_lossy()
            .into_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A child process killed and reaped on drop, so failing tests leave nothing behind.
pub struct ChildGuard(Child);

impl ChildGuard {
    pub fn pid(&self) -> i32 {
        self.0.id() as i32
    }

    /// Kill the child's whole process group (it is the leader), then reap the child.
    pub fn kill(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            // SAFETY: plain syscall; the pgid is our own live child's pid.
            unsafe { libc::killpg(self.pid(), libc::SIGKILL) };
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Makes the `sleeper` test sleep instead of returning at once.
const SLEEPER_ENV: &str = "SIDEBAR_TERM_TEST_SLEEPER";

/// Arguments that make a `fake_binary` sleep for 30 s.
pub const SLEEPER_ARGS: [&str; 4] = [
    "detect::testutil::sleeper",
    "--exact",
    "--ignored",
    "--quiet",
];

/// Not a real test: the body of `fake_binary` processes.
#[test]
#[ignore]
fn sleeper() {
    if std::env::var_os(SLEEPER_ENV).is_some() {
        std::thread::sleep(Duration::from_secs(30));
    }
}

/// A process image whose kernel `comm` is `name`, for impersonating `claude`, `ssh`, ...:
/// a hard link (or copy) of this test binary, run with `SLEEPER_ARGS`. The kernel takes
/// `comm` from the resolved file name, so a symlink to `/bin/sleep` would read as `sleep`,
/// and copies of Apple's arm64e binaries are killed on launch.
pub fn fake_binary(dir: &Path, name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let p = dir.join(name);
    if fs::hard_link(&exe, &p).is_err() {
        fs::copy(&exe, &p).expect("copy test binary");
    }
    p
}

/// Spawn `prog args` in `cwd` as the leader of a new process group (pgid == pid).
pub fn spawn_in_own_group(prog: impl AsRef<Path>, args: &[&str], cwd: &Path) -> ChildGuard {
    let child = Command::new(prog.as_ref())
        .args(args)
        .current_dir(cwd)
        .env(SLEEPER_ENV, "1")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn child");
    ChildGuard(child)
}

/// Poll `cond` for up to 3 s.
pub fn wait_until(mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    cond()
}

/// Run `git args` in `dir`, ignoring the user's global and system config. Panics on failure;
/// returns trimmed stdout.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "advice.detachedHead=false",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// `git init -b main <dir>/<name>` with one commit. Returns the repo path.
pub fn init_repo(parent: &Path, name: &str) -> PathBuf {
    let repo = parent.join(name);
    fs::create_dir_all(&repo).expect("mkdir repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    repo
}
