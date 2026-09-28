//! What gives a Claude Code started in a Session sidebar-term's hooks, without touching the
//! user's own settings: three files the Host writes under its data dir at every launch,
//!
//! - `claude-hooks/bin/claude`: a wrapper first on every Session's `PATH`. It finds the real
//!   `claude` further down `PATH` and runs it with `--settings <settings.json>` (Claude Code
//!   merges those hooks with the user's), except for subcommands, which take no settings.
//! - `claude-hooks/settings.json`: the hooks, each running the hook script.
//! - `claude-hooks/hook`: posts the hook's payload to the Host's hook endpoint (`server.rs`)
//!   with `curl` and prints the reply. With no Host to reach it prints nothing, which leaves
//!   Claude Code to do what it does without hooks.
//!
//! A Session learns where the Host is from its environment ([`session_env`]): the endpoint's
//! URL, its token, and the Session's id (`SIDEBAR_TERM_SESSION_ID`, set by `session.rs`).
//! A `claude` that is not the wrapper (an alias, a PATH the shell's startup files rebuilt) runs
//! without the hooks: the Session is then screen-only, as Codex and Gemini are.

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The directory under the Host's data dir.
pub const DIR: &str = "claude-hooks";
pub const ENV_URL: &str = "SIDEBAR_TERM_HOOK_URL";
pub const ENV_TOKEN: &str = "SIDEBAR_TERM_HOOK_TOKEN";
pub const ENV_SETTINGS: &str = "SIDEBAR_TERM_CLAUDE_SETTINGS";

/// Seconds a hook that waits on the user may take: a question can wait all day.
const WAIT_TIMEOUT: u32 = 86_400;
/// Seconds any other hook may take.
const QUICK_TIMEOUT: u32 = 10;

/// Where the files are, once written.
#[derive(Clone, Debug)]
pub struct Installed {
    pub bin: PathBuf,
    pub settings: PathBuf,
}

/// `s` quoted for a POSIX shell.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

const WRAPPER: &str = r#"#!/bin/sh
# sidebar-term: runs the real claude with sidebar-term's hooks added (see its
# src-tauri/core/src/agents/install.rs). Your own settings are left as they are.
self_dir=$(cd "$(dirname "$0")" && pwd -P)
real=""
old_ifs=$IFS
IFS=:
for d in $PATH; do
  [ -n "$d" ] || continue
  [ "$(cd "$d" 2>/dev/null && pwd -P)" = "$self_dir" ] && continue
  if [ -x "$d/claude" ] && [ ! -d "$d/claude" ]; then real="$d/claude"; break; fi
done
IFS=$old_ifs
if [ -z "$real" ]; then
  echo "claude: command not found" >&2
  exit 127
fi
case "${1-}" in
  mcp|config|update|upgrade|doctor|install|migrate-installer|setup-token|plugin|plugins|-v|--version|-h|--help)
    exec "$real" "$@" ;;
esac
if [ -n "${SIDEBAR_TERM_HOOK_URL-}" ] && [ -r "${SIDEBAR_TERM_CLAUDE_SETTINGS-}" ]; then
  exec "$real" --settings "$SIDEBAR_TERM_CLAUDE_SETTINGS" "$@"
fi
exec "$real" "$@"
"#;

const HOOK: &str = r#"#!/bin/sh
# sidebar-term: hands a Claude Code hook's payload to the Host and prints its reply. Without
# a Host to reach it prints nothing, and Claude Code carries on as it would without the hook.
[ -n "${SIDEBAR_TERM_HOOK_URL-}" ] && [ -n "${SIDEBAR_TERM_SESSION_ID-}" ] || exit 0
curl -sS --fail --max-time 86400 -X POST \
  -H 'content-type: application/json' \
  -H "x-sidebar-term-token: ${SIDEBAR_TERM_HOOK_TOKEN-}" \
  -H "x-sidebar-term-session: $SIDEBAR_TERM_SESSION_ID" \
  --data-binary @- "$SIDEBAR_TERM_HOOK_URL/hook/$1" 2>/dev/null
exit 0
"#;

/// The hooks, as Claude Code's `--settings` reads them, each running `hook` with its event.
pub fn settings(hook: &Path) -> serde_json::Value {
    let run = |event: &str, timeout: u32| {
        serde_json::json!({
            "type": "command",
            "command": format!("{} {event}", sh_quote(&hook.to_string_lossy())),
            "timeout": timeout,
        })
    };
    let every = |event: &str, timeout: u32| serde_json::json!([{ "hooks": [run(event, timeout)] }]);
    let matching = |event: &str, matcher: &str, timeout: u32| {
        serde_json::json!([{ "matcher": matcher, "hooks": [run(event, timeout)] }])
    };
    serde_json::json!({
        "hooks": {
            "SessionStart": every("SessionStart", QUICK_TIMEOUT),
            "SessionEnd": every("SessionEnd", QUICK_TIMEOUT),
            "UserPromptSubmit": every("UserPromptSubmit", QUICK_TIMEOUT),
            "PreToolUse": matching("PreToolUse", super::hooks::ASK_TOOL, WAIT_TIMEOUT),
            "PermissionRequest": matching("PermissionRequest", "*", WAIT_TIMEOUT),
            "PostToolUse": matching("PostToolUse", "*", QUICK_TIMEOUT),
            "Stop": every("Stop", QUICK_TIMEOUT),
        }
    })
}

fn write_executable(path: &Path, content: &str) -> Result<(), String> {
    fs::write(path, content).map_err(|e| format!("{}: {e}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write the three files under `data_dir`, replacing what an older version wrote.
pub fn install(data_dir: &Path) -> Result<Installed, String> {
    let dir = data_dir.join(DIR);
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    let hook = dir.join("hook");
    write_executable(&hook, HOOK)?;
    write_executable(&bin.join("claude"), WRAPPER)?;
    let settings = dir.join("settings.json");
    let text = serde_json::to_string_pretty(&self::settings(&hook)).map_err(|e| e.to_string())?;
    fs::write(&settings, text).map_err(|e| format!("{}: {e}", settings.display()))?;
    Ok(Installed { bin, settings })
}

/// What every Session's environment gets: where the Host's hook endpoint is, its token, the
/// settings, and `bin` first on `PATH` (`path` is the Session's `PATH` so far; an older copy of
/// `bin` in it is dropped, so a sidebar-term run inside another one does not stack them).
pub fn session_env(installed: &Installed, url: &str, token: &str, path: Option<&OsString>) -> Vec<(OsString, OsString)> {
    let bin = installed.bin.as_os_str();
    let mut parts: Vec<OsString> = vec![bin.to_owned()];
    if let Some(path) = path {
        parts.extend(std::env::split_paths(path).map(PathBuf::into_os_string).filter(|p| p.as_os_str() != bin));
    }
    let path = std::env::join_paths(parts).unwrap_or_else(|_| bin.to_owned());
    vec![
        ("PATH".into(), path),
        (ENV_URL.into(), url.into()),
        (ENV_TOKEN.into(), token.into()),
        (ENV_SETTINGS.into(), installed.settings.clone().into_os_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sidebar-term-hooks-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_settings_run_the_hook_quoted() {
        let s = settings(Path::new("/Users/me/Library/Application Support/x/hook"));
        let cmd = s["hooks"]["PermissionRequest"][0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "'/Users/me/Library/Application Support/x/hook' PermissionRequest");
        assert_eq!(s["hooks"]["PreToolUse"][0]["matcher"], "AskUserQuestion");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn the_path_puts_bin_first_once() {
        let installed = Installed { bin: "/d/bin".into(), settings: "/d/s.json".into() };
        let env = session_env(&installed, "http://127.0.0.1:1", "t", Some(&OsString::from("/d/bin:/usr/bin:/bin")));
        assert_eq!(env[0], ("PATH".into(), "/d/bin:/usr/bin:/bin".into()));
        assert_eq!(env[1].1, "http://127.0.0.1:1");
    }

    #[test]
    fn the_wrapper_runs_the_real_claude_with_the_settings() {
        let data = tmp("wrapper");
        let installed = install(&data).unwrap();
        // A fake `claude` further down PATH that prints its arguments.
        let real = data.join("real");
        fs::create_dir_all(&real).unwrap();
        write_executable(&real.join("claude"), "#!/bin/sh\necho \"$@\"\n").unwrap();
        let path = format!("{}:{}:/usr/bin:/bin", installed.bin.display(), real.display());
        let run = |args: &[&str], url: &str| {
            let out = Command::new(installed.bin.join("claude"))
                .args(args)
                .env("PATH", &path)
                .env(ENV_URL, url)
                .env(ENV_SETTINGS, &installed.settings)
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap().trim().to_owned()
        };
        let settings = installed.settings.display().to_string();
        assert_eq!(run(&["-c"], "http://x"), format!("--settings {settings} -c"));
        assert_eq!(run(&["mcp", "list"], "http://x"), "mcp list", "subcommands take no settings");
        assert_eq!(run(&["-p", "hi"], ""), "-p hi", "no Host, no hooks");
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn the_hook_prints_nothing_without_a_host() {
        let data = tmp("hook");
        install(&data).unwrap();
        let out = Command::new(data.join(DIR).join("hook"))
            .arg("Stop")
            .env_remove(ENV_URL)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(out.stdout.is_empty());
        let _ = fs::remove_dir_all(&data);
    }
}
