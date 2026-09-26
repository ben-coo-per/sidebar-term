//! The environment a Session gets so the app's reporters load into the runners that offer an
//! environment channel (docs/research/test-progress.md "Three injection channels"). Pure over a
//! `BTreeMap` so it is unit-tested without spawning anything.
//!
//! What is set, and what happens when the user already set the same variable:
//! - `SIDEBAR_TERM_PROGRESS_DIR`: the Session's progress directory. Always set.
//! - `SIDEBAR_TERM_REPORTERS`: where the reporters are, for a repo's own `vitest` / `jest`
//!   `reporters` entry. Always set.
//! - `PYTEST_ADDOPTS`: `-p sidebar_progress` appended; `PYTHONPATH`: the reporters dir appended.
//! - `PW_TEST_REPORTER`, `MOCHA_OPTIONS`, `CARGO_TARGET_<TRIPLE>_RUNNER`: set only when unset,
//!   since each holds a single value and the user's must win.
//! - `GOFLAGS`: `-exec=<wrapper>` appended, unless it already carries an `-exec`.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

pub const PROGRESS_DIR: &str = "SIDEBAR_TERM_PROGRESS_DIR";
pub const REPORTERS: &str = "SIDEBAR_TERM_REPORTERS";

/// Every variable `inject` may touch (the Settings page names the same list).
#[cfg(test)]
const VARIABLES: [&str; 8] = [
    PROGRESS_DIR,
    REPORTERS,
    "PYTEST_ADDOPTS",
    "PYTHONPATH",
    "PW_TEST_REPORTER",
    "MOCHA_OPTIONS",
    "CARGO_TARGET_<triple>_RUNNER",
    "GOFLAGS",
];

/// The Rust target triple of this Mac, as cargo names it in `CARGO_TARGET_<triple>_RUNNER`.
pub fn host_triple() -> String {
    let os = if cfg!(target_os = "macos") {
        "apple-darwin"
    } else {
        "unknown-linux-gnu"
    };
    format!("{}-{os}", std::env::consts::ARCH)
}

/// Add the progress and reporter variables to `env`. `reporters` is the directory holding the
/// shipped reporters and wrappers; `None` sets only the two `SIDEBAR_TERM_*` variables.
pub fn inject(
    env: &mut BTreeMap<OsString, OsString>,
    progress_dir: &Path,
    reporters: Option<&Path>,
    triple: &str,
) {
    env.insert(PROGRESS_DIR.into(), progress_dir.as_os_str().to_owned());
    let Some(dir) = reporters else {
        return;
    };
    env.insert(REPORTERS.into(), dir.as_os_str().to_owned());
    let path = |file: &str| dir.join(file).into_os_string();

    append(env, "PYTEST_ADDOPTS", " ", OsStr::new("-p sidebar_progress"));
    append(env, "PYTHONPATH", ":", dir.as_os_str());
    set_if_unset(env, "PW_TEST_REPORTER", path("playwright.cjs"));
    let mut mocha = OsString::from("--reporter ");
    mocha.push(path("mocha.cjs"));
    set_if_unset(env, "MOCHA_OPTIONS", mocha);
    let runner_var = format!(
        "CARGO_TARGET_{}_RUNNER",
        triple.to_ascii_uppercase().replace('-', "_")
    );
    let mut runner = path("cargo-runner.sh");
    runner.push(" ");
    runner.push(triple);
    set_if_unset(env, &runner_var, runner);
    let has_exec = env
        .get(OsStr::new("GOFLAGS"))
        .is_some_and(|v| v.to_string_lossy().split_whitespace().any(|f| f.starts_with("-exec")));
    if !has_exec {
        let mut exec = OsString::from("-exec=");
        exec.push(path("go-exec.sh"));
        append(env, "GOFLAGS", " ", &exec);
    }
}

fn set_if_unset(env: &mut BTreeMap<OsString, OsString>, key: &str, value: OsString) {
    let key = OsString::from(key);
    if env.get(&key).is_none_or(|v| v.is_empty()) {
        env.insert(key, value);
    }
}

fn append(env: &mut BTreeMap<OsString, OsString>, key: &str, sep: &str, value: &OsStr) {
    let key = OsString::from(key);
    match env.get(&key).filter(|v| !v.is_empty()) {
        Some(existing) if existing.to_string_lossy().contains(&*value.to_string_lossy()) => {}
        Some(existing) => {
            let mut joined = existing.clone();
            joined.push(sep);
            joined.push(value);
            env.insert(key, joined);
        }
        None => {
            env.insert(key, value.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(env: &'a BTreeMap<OsString, OsString>, k: &str) -> Option<&'a str> {
        env.get(OsStr::new(k)).and_then(|v| v.to_str())
    }

    #[test]
    fn sets_every_channel_on_a_clean_environment() {
        let mut env = BTreeMap::new();
        inject(&mut env, Path::new("/tmp/p/3"), Some(Path::new("/App/Resources/reporters")), "aarch64-apple-darwin");
        assert_eq!(get(&env, PROGRESS_DIR), Some("/tmp/p/3"));
        assert_eq!(get(&env, REPORTERS), Some("/App/Resources/reporters"));
        assert_eq!(get(&env, "PYTEST_ADDOPTS"), Some("-p sidebar_progress"));
        assert_eq!(get(&env, "PYTHONPATH"), Some("/App/Resources/reporters"));
        assert_eq!(get(&env, "PW_TEST_REPORTER"), Some("/App/Resources/reporters/playwright.cjs"));
        assert_eq!(get(&env, "MOCHA_OPTIONS"), Some("--reporter /App/Resources/reporters/mocha.cjs"));
        assert_eq!(
            get(&env, "CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER"),
            Some("/App/Resources/reporters/cargo-runner.sh aarch64-apple-darwin")
        );
        assert_eq!(get(&env, "GOFLAGS"), Some("-exec=/App/Resources/reporters/go-exec.sh"));
        assert_eq!(env.len(), VARIABLES.len());
    }

    #[test]
    fn appends_to_lists_and_leaves_single_values_the_user_set() {
        let mut env: BTreeMap<OsString, OsString> = [
            ("PYTEST_ADDOPTS", "-q --tb=short"),
            ("PYTHONPATH", "/me/lib"),
            ("PW_TEST_REPORTER", "/me/reporter.js"),
            ("MOCHA_OPTIONS", "--reporter dot"),
            ("CARGO_TARGET_X86_64_APPLE_DARWIN_RUNNER", "/me/runner"),
            ("GOFLAGS", "-mod=vendor"),
        ]
        .map(|(k, v)| (k.into(), v.into()))
        .into();
        inject(&mut env, Path::new("/p"), Some(Path::new("/r")), "x86_64-apple-darwin");
        assert_eq!(get(&env, "PYTEST_ADDOPTS"), Some("-q --tb=short -p sidebar_progress"));
        assert_eq!(get(&env, "PYTHONPATH"), Some("/me/lib:/r"));
        assert_eq!(get(&env, "PW_TEST_REPORTER"), Some("/me/reporter.js"));
        assert_eq!(get(&env, "MOCHA_OPTIONS"), Some("--reporter dot"));
        assert_eq!(get(&env, "CARGO_TARGET_X86_64_APPLE_DARWIN_RUNNER"), Some("/me/runner"));
        assert_eq!(get(&env, "GOFLAGS"), Some("-mod=vendor -exec=/r/go-exec.sh"));

        // Injecting twice (a nested launch) changes nothing.
        let before = env.clone();
        inject(&mut env, Path::new("/p"), Some(Path::new("/r")), "x86_64-apple-darwin");
        assert_eq!(env, before);

        // An existing -exec in GOFLAGS is the user's.
        let mut env: BTreeMap<OsString, OsString> = [("GOFLAGS", "-exec=/me/x")].map(|(k, v)| (k.into(), v.into())).into();
        inject(&mut env, Path::new("/p"), Some(Path::new("/r")), "x86_64-apple-darwin");
        assert_eq!(get(&env, "GOFLAGS"), Some("-exec=/me/x"));
    }

    #[test]
    fn without_reporters_only_the_directory_is_named() {
        let mut env = BTreeMap::new();
        inject(&mut env, Path::new("/p"), None, "aarch64-apple-darwin");
        assert_eq!(env.len(), 1);
        assert_eq!(get(&env, PROGRESS_DIR), Some("/p"));
    }

    #[test]
    fn host_triple_is_cargos() {
        let t = host_triple();
        assert!(t == "aarch64-apple-darwin" || t == "x86_64-apple-darwin" || t.ends_with("-unknown-linux-gnu"), "{t}");
    }
}
