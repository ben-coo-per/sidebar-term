//! Classification of the Foreground process group's members: coding agents and remote hops,
//! from a member's `comm`, executable path and argv. Pure and OS-independent; the reads those
//! facts come from are `detect::os` (libproc on macOS, `/proc` on Linux) and are re-exported
//! here for the callers. OWNER: detection agent.

use crate::model::AgentKind;

pub use super::os::{argv, argv_env, children, cwd, exe_path, group_members, short_info};

/// Whether `classify_agent` / `is_remote` can use argv for a process with this `comm`.
/// argv costs two extra syscalls, so read it only for these.
pub fn needs_argv(comm: &str) -> bool {
    is_js_runtime(comm)
        || is_version_like(comm)
        || matches!(
            comm,
            "ssh" | "docker" | "podman" | "docker-compose" | "kubectl"
        )
}

fn is_js_runtime(comm: &str) -> bool {
    matches!(comm, "node" | "bun")
}

/// `2.1.267`: the `comm` of a native Claude Code binary (named after its version).
fn is_version_like(comm: &str) -> bool {
    comm.contains('.') && comm.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

/// Classify one process as a coding agent. `path` is the resolved executable path ("" if
/// unknown); `argv` may be empty when it was not read. Rules (docs/research/agent-detection.md):
///
/// - Claude Code: comm `claude`, or path containing `/claude/versions/` or `/.claude/local/`,
///   or node/bun running `@anthropic-ai/claude-code`. The path rule is the one that fires for
///   the native installer: the kernel's `comm` is the resolved file name, i.e. the version
///   (`2.1.267`), not the `claude` symlink name that `ps -o comm` shows (verified on this
///   machine). If that file was deleted by an update, a version-like comm with argv[0]
///   `claude` still counts.
/// - Codex: comm `codex` (the native binary, also the child of the npm launcher), an executable
///   named `codex-<arch>-apple-darwin` (release tarball / Homebrew; unverified locally), or
///   node/bun running `@openai/codex`.
/// - Gemini: comm `gemini` (single-executable build), or node/bun running `@google/gemini-cli`.
/// - node/bun whose script argument's basename is `claude`, `codex` or `gemini`. This covers the
///   usual npm/Homebrew global install: the shell execs the `bin/gemini` symlink, the kernel
///   passes that unresolved path to node, so argv never contains the package name
///   (verified on this machine; Gemini's self-relaunch child reuses the same argv[1]).
pub fn classify_agent<S: AsRef<str>>(comm: &str, path: &str, argv: &[S]) -> Option<AgentKind> {
    match comm {
        "claude" => return Some(AgentKind::Claude),
        "codex" => return Some(AgentKind::Codex),
        "gemini" => return Some(AgentKind::Gemini),
        _ => {}
    }
    if path.contains("/claude/versions/") || path.contains("/.claude/local/") {
        return Some(AgentKind::Claude);
    }
    // Native Claude Code whose version file the auto-updater has since deleted: `proc_pidpath`
    // fails for a deleted executable (verified), but argv[0] still names what the shell ran.
    if is_version_like(comm) {
        let argv0 = argv.first().map(AsRef::as_ref).unwrap_or("");
        if argv0.rsplit('/').next() == Some("claude") || argv0.contains("/claude/versions/") {
            return Some(AgentKind::Claude);
        }
    }
    // Codex release binaries keep their target-triple name (`codex-aarch64-apple-darwin`),
    // which `comm` truncates to 16 bytes.
    let file = path.rsplit('/').next().unwrap_or("");
    if file.starts_with("codex-") && file.ends_with("-apple-darwin") {
        return Some(AgentKind::Codex);
    }
    if !is_js_runtime(comm) {
        return None;
    }
    const PACKAGES: [(&str, AgentKind); 3] = [
        ("@anthropic-ai/claude-code", AgentKind::Claude),
        ("@openai/codex", AgentKind::Codex),
        ("@google/gemini-cli", AgentKind::Gemini),
    ];
    for arg in argv.iter().skip(1) {
        let arg = arg.as_ref();
        for (pkg, kind) in PACKAGES {
            if mentions_package(arg, pkg) {
                return Some(kind);
            }
        }
    }
    let script = js_script_arg(comm, argv)?;
    let base = script.rsplit('/').next().unwrap_or(script);
    let base = base
        .strip_suffix(".js")
        .or_else(|| base.strip_suffix(".mjs"))
        .or_else(|| base.strip_suffix(".cjs"))
        .unwrap_or(base);
    match base {
        "claude" => Some(AgentKind::Claude),
        "codex" => Some(AgentKind::Codex),
        "gemini" => Some(AgentKind::Gemini),
        _ => None,
    }
}

/// True when `arg` is a path through npm package `pkg`: `.../@openai/codex/bin/x`, or bun's cache
/// `.../@google/gemini-cli@0.55.1@@@1/...`; not `@openai/codex-sdk`. A bare package name (as in
/// `npm i -g @openai/codex`) does not count; `npx <pkg>` is caught by its `.bin/<name>` child.
fn mentions_package(arg: &str, pkg: &str) -> bool {
    let mut from = 0;
    while let Some(i) = arg.get(from..).and_then(|s| s.find(pkg)) {
        let start = from + i;
        let end = start + pkg.len();
        let before_ok = start > 0 && arg.as_bytes().get(start - 1) == Some(&b'/');
        let after_ok = matches!(arg.as_bytes().get(end), None | Some(b'/') | Some(b'@'));
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

/// The script a node/bun invocation runs: the first positional argument after runtime flags
/// (and after bun's `run` / `x` verbs). `None` for `node -e ...` or a bare REPL.
fn js_script_arg<'a, S: AsRef<str>>(comm: &str, argv: &'a [S]) -> Option<&'a str> {
    // Node flags that take a separate value argument.
    const VALUE_FLAGS: [&str; 8] = [
        "-r",
        "--require",
        "--import",
        "--loader",
        "--experimental-loader",
        "-C",
        "--conditions",
        "--title",
    ];
    let mut args = argv.iter().skip(1).map(AsRef::as_ref);
    while let Some(arg) = args.next() {
        match arg {
            "--" => return args.next(),
            "-e" | "--eval" | "-p" | "--print" => return None,
            a if VALUE_FLAGS.contains(&a) => {
                args.next();
            }
            a if a.starts_with('-') => {}
            "run" | "x" if comm == "bun" => {}
            a => return Some(a),
        }
    }
    None
}

/// Whether a process is a hop to another machine or container, so the Session's local cwd
/// says nothing about where the user is: `ssh`, `mosh-client`, `et`, `telnet`,
/// `docker`/`podman` `exec`/`run`/`attach` (also under `container` / `compose`),
/// `kubectl exec`/`attach`.
///
/// `ssh` running as a transport for git, scp, sftp or rsync (a `git push` puts `ssh` in the
/// Foreground process group for a second) is not a hop.
pub fn is_remote<S: AsRef<str>>(comm: &str, argv: &[S]) -> bool {
    match comm {
        "ssh" => !is_ssh_transport(argv),
        "mosh-client" | "et" | "telnet" => true,
        "docker" | "podman" | "docker-compose" => {
            const VALUE_FLAGS: [&str; 16] = [
                "-c",
                "--context",
                "-H",
                "--host",
                "--config",
                "-l",
                "--log-level",
                "--connection",
                "--url",
                "-f",
                "--file",
                "-p",
                "--project-name",
                "--profile",
                "--env-file",
                "--project-directory",
            ];
            let words = positionals(argv, &VALUE_FLAGS);
            let mut words = words.iter().map(String::as_str);
            let mut verb = words.next();
            while matches!(verb, Some("container" | "compose")) {
                verb = words.next();
            }
            matches!(verb, Some("exec" | "run" | "attach"))
        }
        "kubectl" => {
            const VALUE_FLAGS: [&str; 12] = [
                "-n",
                "--namespace",
                "--context",
                "--kubeconfig",
                "--cluster",
                "--user",
                "-s",
                "--server",
                "--as",
                "--as-group",
                "--token",
                "--request-timeout",
            ];
            let words = positionals(argv, &VALUE_FLAGS);
            matches!(words.first().map(String::as_str), Some("exec" | "attach"))
        }
        _ => false,
    }
}

/// Positional words of a CLI invocation (argv[0] excluded), skipping `-x`/`--x` flags and the
/// separate value of any flag in `value_flags`. `--flag=value` needs no special casing.
fn positionals<S: AsRef<str>>(argv: &[S], value_flags: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut args = argv.iter().skip(1).map(AsRef::as_ref);
    while let Some(arg) = args.next() {
        if arg == "--" {
            out.extend(args.map(str::to_owned));
            break;
        }
        if arg.starts_with('-') && arg.len() > 1 {
            if value_flags.contains(&arg) {
                args.next();
            }
            continue;
        }
        out.push(arg.to_owned());
    }
    out
}

/// True when an `ssh` argv is a non-interactive transport: `-s` (subsystem, used by sftp and
/// modern scp) or a remote command starting with a git / scp / rsync server program.
fn is_ssh_transport<S: AsRef<str>>(argv: &[S]) -> bool {
    // OpenSSH getopt: options that take a value (attached or as the next argument).
    const VALUE_OPTS: &[u8] = b"BbcDEeFIiJLlmOopQRSWw";
    const TRANSPORT_CMDS: [&str; 7] = [
        "git-upload-pack",
        "git-receive-pack",
        "git-upload-archive",
        "git-lfs-authenticate",
        "git-lfs-transfer",
        "scp",
        "rsync",
    ];
    let mut args = argv.iter().skip(1).map(AsRef::as_ref);
    let mut host_seen = false;
    while let Some(arg) = args.next() {
        if !host_seen && arg == "--" {
            host_seen = args.next().is_some();
            continue;
        }
        if !host_seen && arg.starts_with('-') && arg.len() > 1 {
            let opts = &arg.as_bytes()[1..];
            for (i, &c) in opts.iter().enumerate() {
                if c == b's' {
                    return true;
                }
                if VALUE_OPTS.contains(&c) {
                    if i + 1 == opts.len() {
                        args.next(); // value is the next argument
                    }
                    break; // rest of this cluster is the value
                }
            }
            continue;
        }
        if !host_seen {
            host_seen = true;
            continue;
        }
        // First word of the remote command (git passes "git-upload-pack 'repo'" as one arg).
        let first = arg.split_whitespace().next().unwrap_or("");
        let first = first.trim_matches(|c| c == '\'' || c == '"');
        return TRANSPORT_CMDS.contains(&first);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentKind::{Claude, Codex, Gemini};

    const NO_ARGS: &[&str] = &[];

    #[test]
    fn classify_agent_table() {
        let cases: &[(&str, &str, &[&str], Option<AgentKind>)] = &[
            // Claude Code native installer: comm from the ~/.local/bin/claude symlink name.
            ("claude", "/Users/u/.local/share/claude/versions/2.1.267", NO_ARGS, Some(Claude)),
            // What the kernel really reports for the native install: comm is the resolved file
            // name, i.e. the version.
            ("2.1.267", "/Users/u/.local/share/claude/versions/2.1.267", NO_ARGS, Some(Claude)),
            // ... after an update deleted that file: no path, argv[0] as typed or as a path.
            ("2.1.266", "", &["claude", "--resume"], Some(Claude)),
            ("2.1.266", "", &["/Users/u/.local/bin/claude"], Some(Claude)),
            ("2.1.266", "", &["/Users/u/.local/share/claude/versions/2.1.266"], Some(Claude)),
            ("2.1.266", "", &["ugrep", "-i", "x"], None),
            ("3.12", "", NO_ARGS, None),
            // Legacy local install: by path rule, and (realistically) by node's script argument.
            ("node", "/Users/u/.claude/local/node_modules/.bin/claude", NO_ARGS, Some(Claude)),
            ("node", "/opt/homebrew/bin/node", &["node", "/Users/u/.claude/local/node_modules/.bin/claude"], Some(Claude)),
            // npm package via node.
            (
                "node",
                "/opt/homebrew/Cellar/node/24.1.0/bin/node",
                &["node", "/opt/homebrew/lib/node_modules/@anthropic-ai/claude-code/cli.js"],
                Some(Claude),
            ),
            ("node", "", &["node", "/Users/u/.nvm/versions/node/v24.13.1/bin/claude"], Some(Claude)),
            // Codex: native binary (Homebrew or the npm launcher's child).
            (
                "codex",
                "/Users/u/.nvm/versions/node/v24.13.1/lib/node_modules/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/codex/codex",
                NO_ARGS,
                Some(Codex),
            ),
            // Codex release binary (Homebrew cask / GitHub tarball), comm truncated.
            ("codex-aarch64-a", "/opt/homebrew/Caskroom/codex/0.120.0/codex-aarch64-apple-darwin", NO_ARGS, Some(Codex)),
            // Codex npm launcher, by package path and by the unresolved bin symlink.
            (
                "node",
                "",
                &["node", "/Users/u/.nvm/versions/node/v24.13.1/lib/node_modules/@openai/codex/bin/codex.js"],
                Some(Codex),
            ),
            ("node", "", &["node", "/Users/u/.nvm/versions/node/v24.13.1/bin/codex", "--full-auto"], Some(Codex)),
            // Gemini: resolved bundle path, npx cache, bin symlink, relaunch child with flags.
            (
                "node",
                "",
                &["node", "/Users/u/.nvm/versions/node/v24.13.1/lib/node_modules/@google/gemini-cli/bundle/gemini.js"],
                Some(Gemini),
            ),
            (
                "node",
                "",
                &["node", "/Users/u/.npm/_npx/6a1b/node_modules/@google/gemini-cli/bundle/gemini.js", "-p", "hi"],
                Some(Gemini),
            ),
            ("node", "", &["node", "/Users/u/.nvm/versions/node/v24.13.1/bin/gemini"], Some(Gemini)),
            (
                "node",
                "",
                &["node", "--max-old-space-size=8192", "/Users/u/.nvm/versions/node/v24.13.1/bin/gemini"],
                Some(Gemini),
            ),
            // npx: the npm launcher is not matched, the `.bin/gemini` child it spawns is.
            ("node", "", &["node", "/Users/u/.npm/_npx/6a1b/node_modules/.bin/gemini"], Some(Gemini)),
            ("node", "", &["node", "/Users/u/.nvm/versions/node/v24.13.1/bin/npx", "@google/gemini-cli"], None),
            ("node", "", &["node", "/Users/u/.nvm/versions/node/v24.13.1/bin/npm", "i", "-g", "@openai/codex"], None),
            ("bun", "", &["bun", "/Users/u/.bun/install/cache/@google/gemini-cli@0.55.1@@@1/bundle/gemini.js"], Some(Gemini)),
            ("bun", "", &["bun", "x", "gemini"], Some(Gemini)),
            ("gemini", "/opt/homebrew/bin/gemini", NO_ARGS, Some(Gemini)),
            // Not agents.
            ("zsh", "/bin/zsh", &["-zsh"], None),
            ("vim", "/usr/bin/vim", &["vim", "node_modules/@openai/codex/bin/codex.js"], None),
            ("node", "", &["node", "server.js"], None),
            ("node", "", &["node"], None),
            ("node", "", &["node", "-e", "require('gemini')"], None),
            ("node", "", &["node", "/x/node_modules/@openai/codex-sdk/dist/index.js"], None),
            ("node", "", &["node", "/x/node_modules/@google/gemini-cli-core/dist/index.js"], None),
            ("node", "", &["node", "-r", "claude", "app.js"], None),
            ("Claude", "/Applications/Claude.app/Contents/MacOS/Claude", NO_ARGS, None),
            ("claude-dev", "", NO_ARGS, None),
            // Node process whose argv was not read: unknown, not an agent.
            ("node", "/Users/u/.nvm/versions/node/v24.13.1/bin/node", NO_ARGS, None),
        ];
        for (comm, path, argv, want) in cases {
            assert_eq!(
                classify_agent(comm, path, argv),
                *want,
                "comm={comm} path={path} argv={argv:?}"
            );
        }
    }

    #[test]
    fn is_remote_table() {
        let cases: &[(&str, &[&str], bool)] = &[
            ("ssh", &["ssh", "prod"], true),
            (
                "ssh",
                &["ssh", "-p", "2222", "-i", "~/.ssh/k", "ben@host"],
                true,
            ),
            ("ssh", &["ssh", "-t", "host", "tmux", "attach"], true),
            ("ssh", &["ssh", "-J", "bastion", "host", "uptime"], true),
            ("ssh", &["ssh", "-vp22", "host"], true),
            (
                "mosh-client",
                &["mosh-client", "-#", "host", "1.2.3.4", "60001"],
                true,
            ),
            ("et", &["et", "host:8080"], true),
            ("telnet", &["telnet", "towel.blinkenlights.nl"], true),
            ("docker", &["docker", "exec", "-it", "web", "bash"], true),
            ("docker", &["docker", "run", "--rm", "-it", "ubuntu"], true),
            (
                "docker",
                &["docker", "--context", "remote", "exec", "-it", "web", "sh"],
                true,
            ),
            (
                "docker",
                &["docker", "container", "exec", "-it", "web", "sh"],
                true,
            ),
            (
                "docker",
                &["docker", "compose", "-f", "dev.yml", "exec", "web", "sh"],
                true,
            ),
            ("docker", &["docker", "attach", "web"], true),
            ("podman", &["podman", "run", "-it", "fedora"], true),
            (
                "kubectl",
                &["kubectl", "exec", "-it", "pod", "--", "sh"],
                true,
            ),
            (
                "kubectl",
                &["kubectl", "-n", "prod", "exec", "-it", "pod", "--", "sh"],
                true,
            ),
            (
                "kubectl",
                &["kubectl", "--context=prod", "attach", "pod"],
                true,
            ),
            // Not hops.
            ("zsh", &["-zsh"], false),
            ("docker", &["docker", "ps"], false),
            ("docker", &["docker", "build", "-t", "run", "."], false),
            ("docker", &["docker", "compose", "up"], false),
            ("kubectl", &["kubectl", "get", "pods"], false),
            ("kubectl", &["kubectl", "logs", "-f", "exec"], false),
            ("git", &["git", "push"], false),
            // ssh as a transport.
            (
                "ssh",
                &[
                    "ssh",
                    "-o",
                    "SendEnv=GIT_PROTOCOL",
                    "git@github.com",
                    "git-upload-pack 'ben/x.git'",
                ],
                false,
            ),
            (
                "ssh",
                &["ssh", "git@github.com", "git-receive-pack 'ben/x.git'"],
                false,
            ),
            (
                "ssh",
                &[
                    "ssh",
                    "-x",
                    "-oForwardAgent=no",
                    "-oPermitLocalCommand=no",
                    "-oClearAllForwarding=yes",
                    "-oRemoteCommand=none",
                    "-oRequestTTY=no",
                    "-oForwardX11=no",
                    "-s",
                    "--",
                    "host",
                    "sftp",
                ],
                false,
            ),
            (
                "ssh",
                &[
                    "ssh",
                    "-l",
                    "ben",
                    "host",
                    "rsync",
                    "--server",
                    "-vlogDtpre.iLsfxCIvu",
                    ".",
                    "/dst",
                ],
                false,
            ),
            ("ssh", &["ssh", "host", "scp -t /tmp"], false),
        ];
        for (comm, argv, want) in cases {
            assert_eq!(is_remote(comm, argv), *want, "comm={comm} argv={argv:?}");
        }
        // Without argv an ssh is assumed interactive.
        assert!(is_remote("ssh", NO_ARGS));
    }

    #[test]
    fn needs_argv_only_for_ambiguous_comms() {
        for c in [
            "node", "bun", "ssh", "docker", "podman", "kubectl", "2.1.267",
        ] {
            assert!(needs_argv(c), "{c}");
        }
        for c in ["zsh", "claude", "codex", "vim", "mosh-client", "7z", "..x"] {
            assert!(!needs_argv(c), "{c}");
        }
    }
}
