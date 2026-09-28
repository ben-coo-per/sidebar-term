//! Classification of test runners: which processes under a Session are a test suite's driver
//! (`vitest`, `pytest`, `cargo test`, ...) or one of its test binaries. Pure functions over a
//! process's `comm`, executable path and argv, as `process::classify_agent` is for agents.
//! Verified process shapes: docs/research/test-progress.md "Runner processes, as observed".

use std::fmt;

/// A test runner the app recognises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Runner {
    Vitest,
    Jest,
    Mocha,
    Playwright,
    Pytest,
    /// `cargo test`: one process per test binary, tests as threads.
    Cargo,
    /// `cargo nextest run`: one process per test.
    Nextest,
    /// `go test`: one `<pkg>.test` binary per package.
    Go,
}

impl Runner {
    /// The short name shown on the Tab and used in the history key.
    pub fn label(self) -> &'static str {
        match self {
            Runner::Vitest => "vitest",
            Runner::Jest => "jest",
            Runner::Mocha => "mocha",
            Runner::Playwright => "playwright",
            Runner::Pytest => "pytest",
            Runner::Cargo => "cargo",
            Runner::Nextest => "nextest",
            Runner::Go => "go",
        }
    }

    /// Runners that build first: their Suite is `building` until a test binary appears.
    pub fn builds_first(self) -> bool {
        matches!(self, Runner::Cargo | Runner::Nextest | Runner::Go)
    }
}

impl fmt::Display for Runner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What a classified process is to its Suite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The process the user (or agent) started: `vitest`, `pytest`, `cargo test`, `go test`.
    Driver,
    /// A test binary a driver spawned once the build was done: `target/*/deps/<crate>-<hash>`
    /// (cargo test's one process, nextest's per-test processes) or `<pkg>.test` (go).
    TestBinary,
}

/// One classified process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunnerProcess {
    pub runner: Runner,
    pub role: Role,
}

const fn driver(runner: Runner) -> Option<RunnerProcess> {
    Some(RunnerProcess {
        runner,
        role: Role::Driver,
    })
}

const fn binary(runner: Runner) -> Option<RunnerProcess> {
    Some(RunnerProcess {
        runner,
        role: Role::TestBinary,
    })
}

/// Whether `classify_runner` needs argv for a process with this `comm`: the interpreters and
/// drivers whose command line says what they run. Disjoint from `process::needs_argv`'s
/// concerns; the suite walk asks both.
pub fn needs_argv(comm: &str) -> bool {
    is_js_runtime(comm) || is_python(comm) || matches!(comm, "cargo" | "go")
}

fn is_js_runtime(comm: &str) -> bool {
    matches!(comm, "node" | "bun")
}

/// `python`, `python3`, `python3.11`, and the macOS framework build's `Python`.
fn is_python(comm: &str) -> bool {
    let rest = match comm.strip_prefix("python") {
        Some(r) => r,
        None => match comm.strip_prefix("Python") {
            Some(r) => r,
            None => return false,
        },
    };
    rest.is_empty() || rest.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

/// Classify one process. `path` is the resolved executable path ("" if unknown); `argv` may be
/// empty when it was not read.
///
/// - JavaScript runners: node/bun whose script argument's basename (extension stripped) is
///   `vitest`, `jest`, `mocha` / `_mocha`, or `playwright` followed by `test`. Workers are not
///   matched: their scripts are `processChild.js`, `forks.js` and the like.
/// - pytest: a python whose script is `pytest` / `py.test`, or `-m pytest`. Covers `.venv/bin/pytest`,
///   `python -m pytest` and `uv run pytest` (uv spawns `python …/pytest`). xdist workers
///   (`python -u -c …`) are not matched.
/// - cargo: `cargo test` / `cargo t`; `cargo nextest run` and the `cargo-nextest` binary it
///   execs. A test binary is any executable under `/target/<profile>/deps/`.
/// - go: `go test`; a test binary is a `<pkg>.test` under go's build cache.
pub fn classify_runner<S: AsRef<str>>(comm: &str, path: &str, argv: &[S]) -> Option<RunnerProcess> {
    if is_js_runtime(comm) {
        let words = positionals(argv, &NODE_VALUE_FLAGS);
        let mut words = words.iter().map(String::as_str);
        let script = words.next()?;
        return match script_name(script) {
            "vitest" => driver(Runner::Vitest),
            "jest" => driver(Runner::Jest),
            "mocha" | "_mocha" => driver(Runner::Mocha),
            "playwright" if words.next() == Some("test") => driver(Runner::Playwright),
            _ => None,
        };
    }
    if is_python(comm) {
        let words = positionals(argv, &PYTHON_VALUE_FLAGS);
        return match words.first().map(|w| script_name(w)) {
            Some("pytest" | "py.test") => driver(Runner::Pytest),
            _ if argv_has_module(argv, "pytest") => driver(Runner::Pytest),
            _ => None,
        };
    }
    match comm {
        "cargo" => {
            let words = positionals(argv, &CARGO_VALUE_FLAGS);
            let mut words = words.iter().map(String::as_str);
            return match words.next() {
                Some("test" | "t") => driver(Runner::Cargo),
                Some("nextest") if matches!(words.next(), Some("run" | "r")) => {
                    driver(Runner::Nextest)
                }
                _ => None,
            };
        }
        "cargo-nextest" => {
            let words = positionals(argv, &CARGO_VALUE_FLAGS);
            let mut words = words.iter().map(String::as_str);
            if words.next() == Some("nextest") && matches!(words.next(), Some("run" | "r")) {
                return driver(Runner::Nextest);
            }
            return None;
        }
        "go" => {
            let words = positionals(argv, &[]);
            return match words.first().map(String::as_str) {
                Some("test") => driver(Runner::Go),
                _ => None,
            };
        }
        _ => {}
    }
    if is_cargo_test_binary(path) {
        return binary(Runner::Cargo);
    }
    if is_go_test_binary(path) {
        return binary(Runner::Go);
    }
    None
}

/// `…/target/debug/deps/crate-9a1cc61f1c687a78`: a cargo test binary (`cargo run` binaries live
/// in `target/<profile>/`, build scripts in `target/<profile>/build/`).
fn is_cargo_test_binary(path: &str) -> bool {
    let Some(deps_at) = path.rfind("/deps/") else {
        return false;
    };
    let file = &path[deps_at + "/deps/".len()..];
    path[..deps_at].contains("/target/") && !file.is_empty() && !file.contains('/')
}

/// `/var/folders/…/go-build…/b001/alpha.test`: a go test binary.
fn is_go_test_binary(path: &str) -> bool {
    path.ends_with(".test") && path.contains("/go-build")
}

/// The basename of a script argument with a JavaScript extension stripped: `vitest` for
/// `node_modules/.bin/vitest` and for `node_modules/vitest/vitest.mjs`.
fn script_name(arg: &str) -> &str {
    let base = arg.rsplit('/').next().unwrap_or(arg);
    base.strip_suffix(".js")
        .or_else(|| base.strip_suffix(".mjs"))
        .or_else(|| base.strip_suffix(".cjs"))
        .unwrap_or(base)
}

/// `python -m pytest …`: `-m` before the first positional, skipping other flags' values.
fn argv_has_module<S: AsRef<str>>(argv: &[S], module: &str) -> bool {
    let mut args = argv.iter().skip(1).map(AsRef::as_ref);
    while let Some(a) = args.next() {
        if a == "-m" {
            return args.next() == Some(module);
        }
        if PYTHON_VALUE_FLAGS.contains(&a) {
            args.next();
            continue;
        }
        if !a.starts_with('-') {
            return false;
        }
    }
    false
}

/// Node flags that take a separate value argument.
const NODE_VALUE_FLAGS: [&str; 8] = [
    "-r",
    "--require",
    "--import",
    "--loader",
    "--experimental-loader",
    "-C",
    "--conditions",
    "--title",
];
/// Python flags that take a separate value argument (`-m` is handled by `argv_has_module`).
const PYTHON_VALUE_FLAGS: [&str; 4] = ["-W", "-X", "--check-hash-based-pycs", "-m"];
const CARGO_VALUE_FLAGS: [&str; 4] = ["--manifest-path", "--config", "-Z", "--color"];

/// Positional words of an invocation (argv[0] excluded), skipping `-x` / `--x` flags, the
/// separate value of any flag in `value_flags`, and cargo's `+toolchain`.
fn positionals<S: AsRef<str>>(argv: &[S], value_flags: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut args = argv.iter().skip(1).map(AsRef::as_ref);
    while let Some(arg) = args.next() {
        if arg == "--" {
            out.extend(args.map(str::to_owned));
            break;
        }
        if arg.starts_with('+') && out.is_empty() {
            continue;
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

/// The command signature a Suite's duration history is keyed by: the runner plus the
/// positional arguments that choose what runs (`pytest tests/unit` and `pytest` are different
/// suites; `-q` is not). Flag values that name a selection (`-k expr`, `-p crate`, `-run X`)
/// stay in; the runner's own verb and script do not. Paths are used as typed, minus a trailing
/// slash.
pub fn signature<S: AsRef<str>>(runner: Runner, argv: &[S]) -> String {
    let words: Vec<String> = match runner {
        Runner::Vitest | Runner::Jest | Runner::Mocha | Runner::Playwright => {
            let mut w = positionals(argv, &NODE_VALUE_FLAGS);
            if !w.is_empty() {
                w.remove(0); // the script
            }
            if runner == Runner::Playwright && w.first().map(String::as_str) == Some("test") {
                w.remove(0);
            }
            if runner == Runner::Vitest && matches!(w.first().map(String::as_str), Some("run" | "watch")) {
                w.remove(0);
            }
            w
        }
        Runner::Pytest => {
            let mut w = positionals(argv, &["-W", "-X", "-m", "-p", "-c", "-o", "-n", "--maxfail", "--rootdir"]);
            if w.first().is_some_and(|s| matches!(script_name(s), "pytest" | "py.test")) {
                w.remove(0);
            }
            w
        }
        Runner::Cargo | Runner::Nextest => {
            let mut w = positionals(
                argv,
                &["--manifest-path", "--config", "-Z", "--color", "--target", "--target-dir", "-j", "--jobs", "--profile", "--features", "-F"],
            );
            if matches!(w.first().map(String::as_str), Some("test" | "t")) {
                w.remove(0);
            } else if w.first().map(String::as_str) == Some("nextest") {
                w.remove(0);
                if matches!(w.first().map(String::as_str), Some("run" | "r")) {
                    w.remove(0);
                }
            }
            w
        }
        Runner::Go => {
            let mut w = positionals(argv, &["-count", "-timeout", "-p", "-exec", "-tags", "-cpu", "-parallel", "-o", "-coverprofile"]);
            if w.first().map(String::as_str) == Some("test") {
                w.remove(0);
            }
            w
        }
    };
    let mut out = runner.label().to_owned();
    for w in words {
        let w = w.strip_suffix('/').unwrap_or(&w);
        out.push(' ');
        out.push_str(w);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use Role::{Driver, TestBinary};

    const NO_ARGS: &[&str] = &[];

    type Case = (&'static str, &'static str, &'static [&'static str], Option<(Runner, Role)>);

    #[test]
    fn classify_runner_table() {
        let cases: &[Case] = &[
            // vitest 5: the driver, and one of its per-file workers.
            ("node", "/opt/homebrew/bin/node", &["node", "node_modules/.bin/vitest", "run"], Some((Runner::Vitest, Driver))),
            ("node", "", &["node", "/x/node_modules/vitest/vitest.mjs", "--reporter=dot"], Some((Runner::Vitest, Driver))),
            (
                "node",
                "",
                &["node", "--experimental-import-meta-resolve", "--require", "/x/node_modules/vitest/suppress-warnings.cjs", "--conditions", "node", "/x/node_modules/vitest/dist/workers/forks.js"],
                None,
            ),
            // jest 30: driver and jest-worker child.
            ("node", "", &["node", "node_modules/.bin/jest", "--ci"], Some((Runner::Jest, Driver))),
            ("node", "", &["node", "/x/node_modules/jest/bin/jest.js"], Some((Runner::Jest, Driver))),
            ("node", "", &["node", "/x/node_modules/jest-worker/build/processChild.js"], None),
            // mocha 12.
            ("node", "", &["node", "node_modules/.bin/mocha", "mocha/"], Some((Runner::Mocha, Driver))),
            ("node", "", &["node", "/x/node_modules/mocha/bin/_mocha"], Some((Runner::Mocha, Driver))),
            // Playwright: `playwright test` only; `playwright codegen` is not a suite.
            ("node", "", &["node", "node_modules/.bin/playwright", "test"], Some((Runner::Playwright, Driver))),
            ("node", "", &["node", "node_modules/.bin/playwright", "test", "--project=chromium"], Some((Runner::Playwright, Driver))),
            ("node", "", &["node", "node_modules/.bin/playwright", "codegen"], None),
            ("node", "", &["node", "/x/node_modules/playwright/lib/common/process.js"], None),
            // bun runs the same scripts.
            ("bun", "", &["bun", "node_modules/.bin/vitest"], Some((Runner::Vitest, Driver))),
            // pytest: venv script, `python -m pytest`, uv's child, the framework build's comm.
            ("python", "/r/.venv/bin/python", &["/r/.venv/bin/python", ".venv/bin/pytest", "-q"], Some((Runner::Pytest, Driver))),
            ("python3.11", "", &["python3.11", "-m", "pytest", "tests/"], Some((Runner::Pytest, Driver))),
            ("Python", "", &["/opt/homebrew/opt/python@3.12/bin/python3.12", "/opt/homebrew/bin/pytest"], Some((Runner::Pytest, Driver))),
            ("python", "", &["python", "-X", "dev", "-m", "pytest"], Some((Runner::Pytest, Driver))),
            ("python", "", &["python", "/x/.venv/bin/py.test"], Some((Runner::Pytest, Driver))),
            // xdist workers and plain scripts.
            ("python", "", &["python", "-u", "-c", "import sys;exec(eval(sys.stdin.readline()))"], None),
            ("python", "", &["python", "manage.py", "test"], None),
            ("python", "", &["python", "-m", "http.server"], None),
            ("python", "", NO_ARGS, None),
            // cargo test and its test binary; cargo run's binary is not one.
            ("cargo", "/Users/u/.cargo/bin/cargo", &["cargo", "test"], Some((Runner::Cargo, Driver))),
            ("cargo", "", &["cargo", "+nightly", "test", "--workspace"], Some((Runner::Cargo, Driver))),
            ("cargo", "", &["cargo", "--manifest-path", "x/Cargo.toml", "t"], Some((Runner::Cargo, Driver))),
            ("cargo", "", &["cargo", "build"], None),
            ("cargo", "", &["cargo", "run", "--", "test"], None),
            ("rstest-9a1cc61f", "/r/target/debug/deps/rstest-9a1cc61f1c687a78", NO_ARGS, Some((Runner::Cargo, TestBinary))),
            ("rstest", "/r/target/debug/rstest", NO_ARGS, None),
            ("build-script-bui", "/r/target/debug/build/libc-1/build-script-build", NO_ARGS, None),
            ("rustc", "/Users/u/.rustup/toolchains/x/bin/rustc", NO_ARGS, None),
            // nextest: the cargo front, the execed binary, its per-test processes.
            ("cargo", "", &["cargo", "nextest", "run"], Some((Runner::Nextest, Driver))),
            ("cargo-nextest", "/Users/u/.cargo/bin/cargo-nextest", &["cargo-nextest", "nextest", "run", "-p", "x"], Some((Runner::Nextest, Driver))),
            ("cargo-nextest", "", &["cargo-nextest", "nextest", "list"], None),
            ("rstest-9a1cc61f", "/r/target/debug/deps/rstest-9a1cc61f1c687a78", &["/r/target/debug/deps/rstest-9a1cc61f1c687a78", "--exact", "tests::a2", "--nocapture"], Some((Runner::Cargo, TestBinary))),
            // go test, its build steps, its per-package binaries.
            ("go", "/opt/homebrew/bin/go", &["go", "test", "./..."], Some((Runner::Go, Driver))),
            ("go", "", &["go", "test", "-count=1", "-run", "TestX", "./pkg"], Some((Runner::Go, Driver))),
            ("go", "", &["go", "build", "./..."], None),
            ("go", "", &["go", "vet", "./..."], None),
            ("compile", "/opt/homebrew/Cellar/go/1.26.3/libexec/pkg/tool/darwin_arm64/compile", NO_ARGS, None),
            ("alpha.test", "/var/folders/xx/T/go-build123/b001/alpha.test", NO_ARGS, Some((Runner::Go, TestBinary))),
            ("alpha.test", "/Users/u/proj/alpha.test", NO_ARGS, None),
            // Everything else.
            ("zsh", "/bin/zsh", &["/bin/zsh", "-c", "vitest run"], None),
            ("node", "", &["node", "/x/node_modules/.bin/pnpm", "test"], None),
            ("node", "", &["node", "server.js"], None),
            ("node", "", NO_ARGS, None),
            ("claude", "", NO_ARGS, None),
        ];
        for (comm, path, argv, want) in cases {
            let want = want.map(|(runner, role)| RunnerProcess { runner, role });
            assert_eq!(
                classify_runner(comm, path, argv),
                want,
                "comm={comm} path={path} argv={argv:?}"
            );
        }
    }

    #[test]
    fn needs_argv_for_interpreters_and_drivers() {
        for c in ["node", "bun", "python", "python3", "python3.11", "Python", "cargo", "go"] {
            assert!(needs_argv(c), "{c}");
        }
        for c in ["zsh", "cargo-nextest", "rustc", "pythonic", "alpha.test", "claude"] {
            assert!(!needs_argv(c), "{c}");
        }
    }

    #[test]
    fn signatures_keep_what_selects_tests_and_drop_noise() {
        let cases: &[(Runner, &[&str], &str)] = &[
            (Runner::Vitest, &["node", "node_modules/.bin/vitest", "run"], "vitest"),
            (Runner::Vitest, &["node", "node_modules/.bin/vitest", "run", "src/lib/", "--reporter=dot"], "vitest src/lib"),
            (Runner::Jest, &["node", "node_modules/.bin/jest", "--ci", "src/a.test.ts"], "jest src/a.test.ts"),
            (Runner::Playwright, &["node", "node_modules/.bin/playwright", "test", "e2e/login.spec.ts"], "playwright e2e/login.spec.ts"),
            (Runner::Mocha, &["node", "node_modules/.bin/mocha", "test/"], "mocha test"),
            (Runner::Pytest, &["python", ".venv/bin/pytest", "-q", "tests/unit"], "pytest tests/unit"),
            (Runner::Pytest, &["python", "-m", "pytest", "-k", "login", "-n", "3"], "pytest login"),
            (Runner::Pytest, &["python", "-m", "pytest"], "pytest"),
            (Runner::Cargo, &["cargo", "test"], "cargo"),
            (Runner::Cargo, &["cargo", "+nightly", "test", "-p", "core", "--", "--nocapture"], "cargo core --nocapture"),
            (Runner::Cargo, &["cargo", "test", "--features", "x", "detect::"], "cargo detect::"),
            (Runner::Nextest, &["cargo-nextest", "nextest", "run", "-p", "core"], "nextest core"),
            (Runner::Nextest, &["cargo", "nextest", "run"], "nextest"),
            (Runner::Go, &["go", "test", "-count=1", "./..."], "go ./..."),
            (Runner::Go, &["go", "test", "-run", "TestX", "./pkg/"], "go TestX ./pkg"),
        ];
        for (runner, argv, want) in cases {
            assert_eq!(signature(*runner, argv), *want, "{runner:?} {argv:?}");
        }
    }
}
