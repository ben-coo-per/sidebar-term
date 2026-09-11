//! What it would take to start a Session's Foreground job again after the app closes (Resume):
//! a Claude Code conversation, reopened with `claude --resume <id>`, or the job's command line,
//! rebuilt from argv. `crate::resume` records these while the app runs; see
//! docs/architecture.md "Resume". OWNER: detection agent.

use super::process;
use crate::model::{AgentKind, ProbeTarget, ResumeEntry, ResumeKind};
use serde::Deserialize;
use std::borrow::Cow;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Longest command line worth typing back into a shell.
const MAX_LINE: usize = 4096;

/// Claude Code flags carried over to `claude --resume`: switches, then options taking one value.
/// Everything else (a prompt, `--continue`, variadic options like `--add-dir`) is dropped.
const CLAUDE_SWITCHES: &[&str] = &[
    "--dangerously-skip-permissions",
    "--allow-dangerously-skip-permissions",
    "--chrome",
    "--no-chrome",
    "--ide",
    "--verbose",
];
const CLAUDE_OPTIONS: &[&str] = &[
    "--model",
    "--permission-mode",
    "--effort",
    "--agent",
    "--fallback-model",
];

/// The Resume entry, keyed `key`, for the Session behind `target`. `None` at a prompt; for Codex
/// and Gemini (not resumable by id yet); for Claude Code without a session file (before 2.1, or
/// `--print`); and for a job whose argv is unreadable (another uid: `sudo`) or not typeable.
pub fn entry(key: &str, target: &ProbeTarget) -> Option<ResumeEntry> {
    let shell = target.shell_pid;
    let pgid = target.fg_pgid.filter(|&p| p > 0 && p != shell)?;
    let mut jobs: Vec<Job> = process::group_members(pgid)
        .into_iter()
        .filter_map(Job::read)
        .collect();
    // Leader first, then ascending pid, as `detect::probe` orders them.
    jobs.sort_by_key(|j| (j.pid != pgid, j.pid));
    let (kind, (line, cwd)) = resume_of(&jobs, shell)?;
    if line.len() > MAX_LINE || line.chars().any(char::is_control) {
        return None;
    }
    Some(ResumeEntry {
        key: key.to_owned(),
        kind,
        line,
        cwd: cwd.or_else(|| process::cwd(shell)),
    })
}

/// A command line and the directory to run it in.
type Line = (String, Option<String>);

/// What resumes a Foreground process group, given its members (leader first) and its shell.
fn resume_of(jobs: &[Job], shell: i32) -> Option<(ResumeKind, Line)> {
    match jobs.iter().find_map(|j| j.agent().map(|a| (a, j))) {
        Some((AgentKind::Claude, j)) => Some((ResumeKind::Claude, claude(j)?)),
        Some(_) => None,
        None => Some((ResumeKind::Command, command(jobs, shell)?)),
    }
}

/// One member of the Foreground process group, with its argv and environment when readable.
struct Job {
    pid: i32,
    ppid: i32,
    comm: String,
    path: String,
    /// Empty when unreadable (another uid).
    argv: Vec<String>,
    env: Vec<String>,
}

impl Job {
    fn read(pid: i32) -> Option<Self> {
        let p = process::short_info(pid)?;
        let (argv, env) = process::argv_env(pid).unwrap_or_default();
        Some(Self {
            pid,
            ppid: p.ppid,
            comm: p.comm,
            path: process::exe_path(pid).unwrap_or_default(),
            argv,
            env,
        })
    }

    fn agent(&self) -> Option<AgentKind> {
        process::classify_agent(&self.comm, &self.path, &self.argv)
    }

    /// The value of environment variable `name` in this process.
    fn var(&self, name: &str) -> Option<&str> {
        self.env
            .iter()
            .find_map(|e| e.strip_prefix(name)?.strip_prefix('='))
    }
}

// ---------------------------------------------------------------------------------------------
// Claude Code
// ---------------------------------------------------------------------------------------------

/// What Claude Code (2.1+) writes to `<config dir>/sessions/<pid>.json` for each running
/// instance, and rewrites as the conversation changes (it follows `/clear`). Only the fields
/// Resume needs.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeSession {
    pid: i32,
    session_id: String,
    cwd: Option<String>,
    /// `interactive` for a conversation in a terminal; absent in older versions.
    kind: Option<String>,
}

/// `claude <carried flags> --resume <id>` in the conversation's directory: Claude Code keeps a
/// conversation under the directory it started in and only finds it from there.
fn claude(job: &Job) -> Option<Line> {
    let dir = claude_config_dir(job.var("CLAUDE_CONFIG_DIR"), job.var("HOME"))?;
    let session = claude_session(&dir, job.pid)?;
    let mut words: Vec<Cow<str>> = vec!["claude".into()];
    words.extend(claude_flags(&job.argv).into_iter().map(quote));
    words.push("--resume".into());
    words.push(session.session_id.into());
    let cwd = session
        .cwd
        .and_then(|c| fs::canonicalize(c).ok())
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| process::cwd(job.pid));
    Some((words.join(" "), cwd))
}

/// `$CLAUDE_CONFIG_DIR`, else `~/.claude`, as the Claude Code process itself sees them.
fn claude_config_dir(config_dir: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    if let Some(dir) = config_dir.filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let home = home
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))?;
    Some(home.join(".claude"))
}

/// The session file of the interactive Claude Code running as `pid`, if it is one.
fn claude_session(config_dir: &Path, pid: i32) -> Option<ClaudeSession> {
    let bytes = fs::read(config_dir.join("sessions").join(format!("{pid}.json"))).ok()?;
    let s: ClaudeSession = serde_json::from_slice(&bytes).ok()?;
    let interactive = s.kind.as_deref().is_none_or(|k| k == "interactive");
    (s.pid == pid && interactive && is_session_id(&s.session_id)).then_some(s)
}

/// A uuid in practice; anything that needs no quoting and cannot be read as a flag.
fn is_session_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The flags of a Claude Code argv worth keeping on resume (`CLAUDE_SWITCHES`, `CLAUDE_OPTIONS`).
fn claude_flags(argv: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut args = argv.iter().skip(1).map(String::as_str);
    while let Some(arg) = args.next() {
        let name = arg.split_once('=').map_or(arg, |(name, _)| name);
        if CLAUDE_SWITCHES.contains(&arg) || (name != arg && CLAUDE_OPTIONS.contains(&name)) {
            out.push(arg);
        } else if CLAUDE_OPTIONS.contains(&arg) {
            if let Some(value) = args.next() {
                out.extend([arg, value]);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Any other command
// ---------------------------------------------------------------------------------------------

/// The job's command line, in the first stage's directory. The stages of a pipeline are the
/// shell's children, in the order it forked them; their own children (npm's `node`, a dev
/// server's workers) are not typed. `None` if a stage's argv is unreadable.
fn command(jobs: &[Job], shell: i32) -> Option<Line> {
    let mut stages: Vec<&Job> = jobs.iter().filter(|j| j.ppid == shell).collect();
    stages.sort_by_key(|j| j.pid);
    if stages.is_empty() {
        stages.extend(jobs.first());
    }
    let first = stages.first()?;
    let mut parts = Vec::with_capacity(stages.len());
    for job in &stages {
        if job.argv.is_empty() {
            return None;
        }
        let argv = typed_argv(&job.argv, job.var("PATH"));
        parts.push(argv.into_iter().map(quote).collect::<Vec<_>>().join(" "));
    }
    Some((parts.join(" | "), process::cwd(first.pid)))
}

/// argv as the user typed it. A script run through its `#!` line reaches its interpreter as
/// `node /opt/homebrew/bin/npm run dev`; when the script is what its name finds on the job's
/// `PATH`, that is `npm run dev`.
fn typed_argv<'a>(argv: &'a [String], path_var: Option<&str>) -> Vec<&'a str> {
    if let [interpreter, script, rest @ ..] = argv {
        let name = basename(script);
        if is_interpreter(basename(interpreter))
            && script.starts_with('/')
            && which(name, path_var).is_some_and(|found| same_file(&found, Path::new(script)))
        {
            return std::iter::once(name)
                .chain(rest.iter().map(String::as_str))
                .collect();
        }
    }
    argv.iter().map(String::as_str).collect()
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Programs that run a script named by their first argument.
fn is_interpreter(name: &str) -> bool {
    matches!(
        name,
        "node" | "bun" | "deno" | "sh" | "bash" | "zsh" | "dash" | "ruby" | "perl" | "php"
    ) || name.starts_with("python")
}

/// The first executable file named `name` in the directories of `path_var`.
fn which(name: &str, path_var: Option<&str>) -> Option<PathBuf> {
    path_var?
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(d).join(name))
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

fn same_file(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

/// `word` as one zsh/bash word: bare when it is plain (`--port=3000`, `src/app.py`), else in
/// single quotes. A leading `=` stays quoted: zsh expands `=cmd` to the path of `cmd`.
pub(crate) fn quote(word: &str) -> Cow<'_, str> {
    let plain = |b: u8| b.is_ascii_alphanumeric() || b"-_./:,+@=".contains(&b);
    if !word.is_empty() && !word.starts_with('=') && word.bytes().all(plain) {
        Cow::Borrowed(word)
    } else {
        Cow::Owned(format!("'{}'", word.replace('\'', r"'\''")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::testutil::{spawn_in_own_group, TempDir};

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Shell pid of the jobs built by `job`.
    const SHELL: i32 = 500;

    /// A group member as `Job::read` would see it, without a process behind it (pids beyond
    /// any real one, so `process::cwd` finds nothing).
    fn job(pid: i32, ppid: i32, comm: &str, argv: &[&str], env: &[&str]) -> Job {
        Job {
            pid: 4_000_000 + pid,
            ppid: if ppid == SHELL {
                SHELL
            } else {
                4_000_000 + ppid
            },
            comm: comm.into(),
            path: String::new(),
            argv: strings(argv),
            env: strings(env),
        }
    }

    fn line(r: Option<(ResumeKind, Line)>) -> Option<(ResumeKind, String)> {
        r.map(|(kind, (line, _))| (kind, line))
    }

    fn executable(path: &Path) {
        fs::write(path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn quotes_only_what_needs_it() {
        for plain in [
            "npm",
            "run",
            "--port=3000",
            "src/app.py",
            "user@host:/x",
            "a+b,c",
        ] {
            assert_eq!(quote(plain), plain);
        }
        assert_eq!(quote(""), "''");
        assert_eq!(quote("two words"), "'two words'");
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote("=ls"), "'=ls'");
        for special in ["$HOME", "*.py", "a;b", "~", "!x", "^x", "%1", "(x)", "é"] {
            assert!(quote(special).starts_with('\''), "{special}");
        }
    }

    #[test]
    fn a_script_run_through_its_interpreter_reads_as_typed() {
        let dir = TempDir::new("resume-which");
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        executable(&bin.join("npm"));
        let npm = bin.join("npm").to_string_lossy().into_owned();
        let path_var = format!("/nonexistent:{}", bin.display());

        let argv = strings(&["node", &npm, "run", "dev"]);
        assert_eq!(typed_argv(&argv, Some(&path_var)), ["npm", "run", "dev"]);
        // Not what `npm` finds on PATH: kept as run.
        assert_eq!(typed_argv(&argv, Some("/nonexistent")), argv);
        assert_eq!(typed_argv(&argv, None), argv);
        // Not an interpreter: `cat /…/bin/npm` is not `npm`.
        let cat = strings(&["cat", &npm]);
        assert_eq!(typed_argv(&cat, Some(&path_var)), cat);
        // A relative script is already as typed.
        let rel = strings(&["python3", "manage.py", "runserver"]);
        assert_eq!(typed_argv(&rel, Some(&path_var)), rel);
    }

    #[test]
    fn keeps_the_claude_flags_that_shape_a_session() {
        let argv = strings(&[
            "claude",
            "--dangerously-skip-permissions",
            "--model",
            "opus",
            "--effort=high",
            "--resume",
            "old-id",
            "-c",
            "--add-dir",
            "../x",
            "fix the tests",
        ]);
        assert_eq!(
            claude_flags(&argv),
            [
                "--dangerously-skip-permissions",
                "--model",
                "opus",
                "--effort=high"
            ]
        );
        // npm installs: argv[1] is the script.
        let npm = strings(&["node", "/opt/homebrew/bin/claude", "--ide", "--model"]);
        assert_eq!(claude_flags(&npm), ["--ide"]);
    }

    #[test]
    fn reads_claude_session_files() {
        let dir = TempDir::new("resume-claude-file");
        let sessions = dir.path().join("sessions");
        fs::create_dir(&sessions).unwrap();
        let write =
            |pid: i32, body: &str| fs::write(sessions.join(format!("{pid}.json")), body).unwrap();

        write(
            10,
            r#"{"pid":10,"sessionId":"5b6d103b-2842","cwd":"/tmp","kind":"interactive","status":"busy"}"#,
        );
        let s = claude_session(dir.path(), 10).unwrap();
        assert_eq!(
            (s.session_id.as_str(), s.cwd.as_deref()),
            ("5b6d103b-2842", Some("/tmp"))
        );
        write(11, r#"{"pid":11,"sessionId":"abc"}"#);
        assert!(
            claude_session(dir.path(), 11).is_some(),
            "no kind: older versions"
        );

        write(12, r#"{"pid":12,"sessionId":"abc","kind":"print"}"#);
        write(13, r#"{"pid":99,"sessionId":"abc"}"#);
        write(14, r#"{"pid":14,"sessionId":"$(rm -rf ~)"}"#);
        write(15, r#"{"pid":15,"sessionId":"--help"}"#);
        write(16, "not json");
        for pid in [12, 13, 14, 15, 16, 17] {
            assert!(claude_session(dir.path(), pid).is_none(), "pid {pid}");
        }
    }

    #[test]
    fn claude_config_dir_follows_the_process_env() {
        assert_eq!(
            claude_config_dir(Some("/cfg"), Some("/h")),
            Some(PathBuf::from("/cfg"))
        );
        assert_eq!(
            claude_config_dir(Some(""), Some("/h")),
            Some(PathBuf::from("/h/.claude"))
        );
        assert_eq!(
            claude_config_dir(None, Some("/h")),
            Some(PathBuf::from("/h/.claude"))
        );
    }

    fn target(fg_pgid: i32) -> ProbeTarget {
        ProbeTarget {
            session_id: 1,
            shell_pid: std::process::id() as i32,
            fg_pgid: Some(fg_pgid),
        }
    }

    #[test]
    fn a_command_reruns_from_its_argv_in_its_cwd() {
        let dir = TempDir::new("resume-command");
        let child = spawn_in_own_group("/bin/sleep", &["30", "it's"], dir.path());
        let e = entry("tab_1", &target(child.pid())).expect("entry");
        assert_eq!(e.key, "tab_1");
        assert_eq!(e.kind, ResumeKind::Command);
        assert_eq!(e.line, r"/bin/sleep 30 'it'\''s'");
        assert_eq!(e.cwd.as_deref(), Some(dir.canonical_str()));
    }

    #[test]
    fn nothing_to_resume_at_a_prompt() {
        let me = std::process::id() as i32;
        let at_prompt = ProbeTarget {
            session_id: 1,
            shell_pid: me,
            fg_pgid: Some(me),
        };
        assert!(entry("k", &at_prompt).is_none());
        assert!(entry(
            "k",
            &ProbeTarget {
                fg_pgid: None,
                ..at_prompt
            }
        )
        .is_none());
        assert!(
            entry("k", &target(i32::MAX)).is_none(),
            "a group that is gone"
        );
    }

    #[test]
    fn claude_code_resumes_its_conversation_where_it_started() {
        let dir = TempDir::new("resume-claude");
        let config = dir.path().join("config");
        fs::create_dir_all(config.join("sessions")).unwrap();
        fs::create_dir(dir.path().join("project")).unwrap();
        let env = format!("CLAUDE_CONFIG_DIR={}", config.display());
        let claude = job(
            1,
            SHELL,
            "claude",
            &["claude", "--model", "opus", "fix it"],
            &[&env],
        );

        // Before Claude Code has written its session file: nothing to resume by id.
        assert_eq!(resume_of(std::slice::from_ref(&claude), SHELL), None);

        let body = format!(
            r#"{{"pid":{},"sessionId":"81cfa7b0-a597","cwd":"{}","kind":"interactive"}}"#,
            claude.pid,
            dir.path().join("project").display()
        );
        fs::write(
            config.join("sessions").join(format!("{}.json", claude.pid)),
            body,
        )
        .unwrap();
        let (kind, (line, cwd)) = resume_of(&[claude], SHELL).expect("resume");
        assert_eq!(kind, ResumeKind::Claude);
        assert_eq!(line, "claude --model opus --resume 81cfa7b0-a597");
        assert_eq!(cwd.as_deref(), Some(dir.canon("project").as_str()));
    }

    #[test]
    fn other_agents_are_left_alone() {
        let codex = job(1, SHELL, "codex", &["codex"], &[]);
        assert_eq!(resume_of(&[codex], SHELL), None);
        // Codex's npm launcher leads the group; the native `codex` is its child.
        let launcher = job(1, SHELL, "node", &["node", "/opt/homebrew/bin/codex"], &[]);
        let native = job(2, 1, "codex", &["codex"], &[]);
        assert_eq!(resume_of(&[launcher, native], SHELL), None);
    }

    #[test]
    fn a_pipeline_reruns_every_stage_but_not_their_children() {
        let npm = job(1, SHELL, "npm", &["npm", "run", "dev"], &[]);
        let vite = job(3, 1, "node", &["node", "vite"], &[]);
        let tee = job(2, SHELL, "tee", &["tee", "dev log.txt"], &[]);
        assert_eq!(
            line(resume_of(&[npm, vite, tee], SHELL)),
            Some((
                ResumeKind::Command,
                "npm run dev | tee 'dev log.txt'".into()
            ))
        );
    }

    #[test]
    fn a_leader_the_shell_did_not_fork_still_reruns() {
        // `(cd x && make)`: the subshell forks, then execs `make`, which is not the shell's child.
        let make = job(1, 7, "make", &["make", "watch"], &[]);
        assert_eq!(
            line(resume_of(&[make], SHELL)),
            Some((ResumeKind::Command, "make watch".into()))
        );
    }

    #[test]
    fn a_job_of_another_user_is_not_rerun() {
        // `sudo`: root's argv is unreadable.
        let sudo = job(1, SHELL, "sudo", &[], &[]);
        assert_eq!(resume_of(&[sudo], SHELL), None);
        let npm = job(1, SHELL, "npm", &["npm", "start"], &[]);
        let root = job(2, SHELL, "tee", &[], &[]);
        assert_eq!(resume_of(&[npm, root], SHELL), None);
    }
}

#[cfg(test)]
mod live {
    /// Not a real test: prints the Resume entry of every foreground job on the ttys of this Mac's
    /// `claude`, `node`, `uv` and `python` processes. `cargo test resume_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn resume_live() {
        let out = std::process::Command::new("pgrep")
            .args(["-x", "claude|node|uv|python3|pnpm|npm"])
            .output()
            .unwrap();
        for pid in String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.trim().parse::<i32>().ok())
        {
            // SAFETY: plain syscalls.
            let (pgid, ppid) = unsafe {
                (
                    libc::getpgid(pid),
                    super::process::short_info(pid).map_or(0, |p| p.ppid),
                )
            };
            if pgid != pid {
                continue; // not a job leader
            }
            let t = crate::model::ProbeTarget {
                session_id: 0,
                shell_pid: ppid,
                fg_pgid: Some(pgid),
            };
            let start = std::time::Instant::now();
            let e = super::entry("k", &t);
            println!("pid {pid} (shell {ppid}) in {:?}: {e:?}", start.elapsed());
        }
    }
}
