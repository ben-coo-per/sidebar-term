# Showing a running test suite's progress and ETA on the Tab

Research for issue #33. Vocabulary per `CONTEXT.md`: a **Session** is one shell on its own pty, a
**Tab** points at it, an **Agent session** is one whose Foreground process is Claude Code, Codex or
Gemini, the **Panel** is the accordion at the bottom of the sidebar and **Activity** its CPU /
memory view. A **runner** below is the process that runs a test suite (`vitest`, `pytest`, ...).

Verified on this Mac (macOS 26.6.2, arm64) on 2026-09-25 against: Claude Code 2.1.282 (native
binary, running in a sidebar-term Session; every process tree below was read from it or from
scratch runs under `bash -c`), vitest 5.0.2, jest 30.5.2, mocha 12.0.2, `@playwright/test` 1.63.0
(node 24.13.1), pytest 9.1.1 with pytest-json-report 1.5.0 and pytest-xdist 3.8.0 (Python 3.11.7),
cargo 1.94.1 with cargo-nextest 0.9.146, go 1.26.3. Codex 0.120.0 and Gemini CLI 0.55.1 are
installed but were not driven; their shell tools come from source. The issue says "try it on
Linux"; it was tried on the dev Mac, which is where the app runs (Linux differs only in the `ps`
flags and in `/proc` replacing libproc). Anything not verified is marked **unverified**.
`rspec`, `phpunit`, `dotnet test` and `gradle test` are not installed here; they are covered from
their documentation only.

## Recommendation

**Detect the suite from the process tree, get progress from a small "progress file" protocol that
the app injects through the environment, fall back to a duration history, and show it as a thin
bar under the Tab's Title with the count and ETA in the Tab's stats slot.** Detection: on each
monitor tick, walk the Agent session's descendants with `proc_listpids(PROC_PPID_ONLY)` and
classify each by executable path and argv (`vitest`, `jest`, `pytest`, `cargo test`, nextest's
per-test processes, `go test` and its `*.test` binaries, `mocha`, `playwright test`); a run shows
within one 500 ms tick, needs nothing from the user, and the same classifier on the Foreground
process group covers suites the user runs by hand. Progress: the app names a per-Session directory
in `SIDEBAR_TERM_PROGRESS_DIR`; every process under the Session inherits it (verified: variables
the app sets reach Claude Code's Bash tool), and the app's own reporters append one JSON line per
test there. They are injected by environment alone, leaving the agent's output untouched:
`PYTEST_ADDOPTS=-p sidebar_progress` + `PYTHONPATH` for pytest, `PW_TEST_REPORTER` for Playwright,
`MOCHA_OPTIONS=--reporter …` for mocha (a wrapper around Spec), `CARGO_TARGET_<triple>_RUNNER` for
cargo test (test count) and nextest (one process per test: count, start, end and pass/fail), and
`GOFLAGS=-exec=…` for go test (per package). vitest and jest have no environment channel and get a
one-line `reporters` entry in the repo's config (a `NODE_OPTIONS --import` shim also works but
taxes every node process the user starts by ~45 ms; not recommended). The runners' own
machine-readable outputs (vitest / jest / pytest-json-report JSON, JUnit XML, nextest's
libtest-json, `.pytest_cache`) are all written when the run ends, so they are useless for progress;
`go test -json` and libtest's JSON do stream but replace what the agent reads. Without a stream, a
history of the last ten durations per repo, runner and command gives an ETA: the bar fills against
the median ("1m 30s of ~2m 10s"), turns amber past the longest run seen, and with no history shows
elapsed time with an indeterminate bar. The Tab row gets a 2 px bar under the Title / Badge lines
and `12/48 · ~1m` (red count once a test fails) in the stats slot where CPU / memory show; the
Agent status icon and Unread are untouched; the Activity view lists every running suite above its
process list; nothing is added to the Terminal area. It appears within a tick of the runner
starting and lingers 5 s after it exits showing the result. Claude Code hooks are not needed:
they carry the command and its end but nothing while it runs, and need an install.

## 1. Detection

### Where the runner sits under Claude Code

Claude Code's Bash tool does not run commands in the Session's pty. Read from this Mac while a
tool call ran (`ps -o pid,ppid,pgid,tpgid,tty,stat,comm,args`):

```
30711  1      30711  0      ??      S   sidebar-term                      (the app)
30724  30711  30724  81790  ttys004 Ss  /bin/zsh -l                       (the Session's shell)
81790  30724  81790  81790  ttys004 R+  claude --resume 13b2…              (Foreground process group)
91191  81790  91191  0      ??      Ss  /bin/zsh -c source ~/.claude/shell-snapshots/snapshot-zsh-<id>.sh
                                        2>/dev/null || true && setopt NO_EXTENDED_GLOB NO_BARE_GLOB_QUAL
                                        2>/dev/null || true && { \builtin unalias -- 'unsetenv'; \builtin
                                        unset -f -- 'unsetenv'; } >/dev/null 2>&1 || true && eval 'sleep 25'
                                        < /dev/null && pwd -P >| /tmp/claude-ec87-cwd
91193  91191  91191  0      ??      S   sleep 25                          (the command)
88462  81790  81790  81790  ttys004 S   caffeinate -i -t 300              (Claude Code keeps the Mac awake)
```

- The tool's shell is the user's `$SHELL` (`/bin/zsh -c`, not `bash -c`), it is a **new session
  leader with no controlling tty** (`Ss`, `tty ??`, `pgid == pid`), so neither it nor the runner is
  ever in the pty's Foreground process group. `detect::probe` cannot see them; only a walk down
  from the agent by ppid can, which is what `activity.rs`'s `attribute()` already does over `ps`.
- The shell always stays as an intermediate process, because the command is `eval '…' < /dev/null
  && pwd -P >| …` (zsh execs the last command of a plain `-c` string, verified with `zsh -c "true
  && sleep 2"`, but not here). So the shape is **claude → zsh → runner → workers**, three levels
  before workers. A `run_in_background: true` tool call has the same shape (the `sleep 25` above
  was one).
- A pipeline or `bash script.sh` adds levels (`zsh -c` → `bash script.sh` → `cargo test`); the walk
  must be recursive, not "children of the agent".
- The command's cwd is the Session's (Claude Code `cd`s in the shell and writes `pwd -P` back), so
  the runner's `PROC_PIDVNODEPATHINFO` cwd is the repo, the same key the Badge uses.

Codex and Gemini (from source, **unverified** live): Codex builds `[shell, "-lc" | "-c", cmd]` with
the user's default shell (`codex-rs/core/src/shell.rs`, `derive_exec_args`), stdin null and
stdout / stderr piped (`spawn.rs`, `StdioPolicy::RedirectForShellTool`), so the same shape with
`zsh -lc`. Gemini uses `bash` with `['-c']` on POSIX (`packages/core/src/utils/shell-utils.ts`,
`getShellConfiguration`) under node-pty when it can, else `child_process.spawn` with `detached:
true` (`services/shellExecutionService.ts`); under node-pty the runner *has* a tty (Gemini's own,
not the Session's), still a descendant of `gemini`'s node process.

### Runner processes, as observed

Each was run as `bash -c "<command>"` from a scratch project (24 tests, 300–600 ms each, one
failing) and the tree read 1–5 s in with `ps -axo pid,ppid,pgid,comm,args`.

| Runner | Command as typed | What appears (comm / exe path / argv) | Workers | Identifiable from |
|---|---|---|---|---|
| vitest 5.0.2 | `vitest run` (`node_modules/.bin/vitest`) | `node node_modules/.bin/vitest run`; exe `…/bin/node` | one `node --experimental-import-meta-resolve --require …/vitest/suppress-warnings.cjs --conditions node …` per file (6 here), env `VITEST=true VITEST_MODE=RUN` | argv[1] basename `vitest` (or a path through `/vitest/`) |
| jest 30.5.2 | `jest` | `node node_modules/.bin/jest` | one `node …/jest-worker/build/processChild.js` per file (7 here), env `JEST_WORKER_ID=n`; `ps` shows `(node)` for a worker in its first moments, `KERN_PROCARGS2` reads it fine | argv[1] basename `jest`; workers by `/jest-worker/` |
| mocha 12.0.2 | `mocha mocha/` | `node node_modules/.bin/mocha mocha/`, no workers by default | – | argv[1] basename `mocha` |
| Playwright 1.63.0 | `playwright test` | `node node_modules/.bin/playwright test`; workers `node …/playwright/lib/…` (2 with `workers: 2`) | yes | argv[1] basename `playwright` and `test` next |
| pytest 9.1.1 | `.venv/bin/pytest -q` / `python -m pytest` / `uv run pytest` | `…/.venv/bin/python .venv/bin/pytest -q`, or `python -m pytest`, or `uv run …` → `python …/pytest` | with `-n 3`: three `python -u -c import sys;exec(eval(sys.stdin.readline()))` (execnet); identifiable only as children | comm is the venv's `python`; argv[1] basename `pytest`, or `-m pytest` |
| cargo test 1.94.1 | `cargo test` | `…/bin/cargo test` → after the build, one `target/debug/deps/rstest-9a1cc61f1c687a78` running every test as **threads** (no per-test process); `rustc` children during the build | – | argv `cargo test`; test binary by path `/target/*/deps/` |
| cargo-nextest 0.9.146 | `cargo nextest run` | `…/cargo-nextest nextest run` → **one process per test**: `target/debug/deps/rstest-9a1c… --exact tests::a2 --nocapture`, each in its own process group (8 at once = cores) | yes | argv `nextest run`; test processes by path and `--exact <name>` |
| go 1.26.3 | `go test ./...` | `go test -count=1 ./...` → `compile` / `link` children while building (~4 s here) → one `/var/folders/…/go-build…/b001/alpha.test -test.paniconexit0 -test.timeout=10m0s` per package | per package | argv `go test`; binaries by `.test` suffix under `go-build` |

Not installed here, from documentation (**unverified**): `rspec` is a Ruby script (`ruby …/bin/rspec`:
match argv[1] basename), `phpunit` is `php …/phpunit` (same rule), `dotnet test` is `dotnet test`
then `dotnet exec …/testhost.dll` children, `gradle test` is a `java` JVM whose argv mentions
`gradle-wrapper` / `org.gradle`, with a `Gradle Test Executor` worker JVM. The classifier shape
is the same: exe basename for native runners, argv[1] (or `-m`) basename for interpreted ones.

### How early, and what it costs

- A runner is recognisable **at exec** (its argv says what it is), so a 500 ms monitor tick sees it
  within 0.5 s of the agent starting it. Two-phase runners (`cargo test`, `go test`) are
  recognisable from their driver at once but only *testing* once the test binary appears: `cargo
  test` after the build, `go test` here after ~4 s of `compile` + `link`.
- A descendant walk with libproc from the live `claude` pid (32 descendants: nine tool shells,
  `cargo`, `rustc`, `docker`, …) costs **931 µs per walk in Python ctypes**
  (`proc_listpids(PROC_PPID_ONLY)` per node plus `proc_pidpath` per pid; scratch `ppidwalk.py`,
  2 000 iterations); Rust will be well under that. Read argv (`KERN_PROCARGS2`, two syscalls) only
  for `node`, `python*`, `ruby`, `php`, `java`, `cargo`, `go`, `dotnet` and the shells, as
  `process::needs_argv` already does for agents.
- Activity's `ps` pass (20 ms every 2 s) attributes every process to a Session too, but only while
  the Panel, Tab stats or Memory Guard want it. Detection belongs in the monitor tick, next to
  `probe`, so it is always on and lands in the same 500 ms cadence as Agent status.

## 2. Progress sources

### Nothing streams from the runners' own files

Each was watched every 0.5 s (`stat -f %z`, `wc -l`) while the run was in progress:

| Source | Behaviour | Verdict |
|---|---|---|
| vitest `--reporter=json --outputFile=x.json` | file does not exist until the end; 6 519 bytes appear at 2.9 s of a 2.9 s run | end-only |
| vitest `--reporter=junit --outputFile=x.xml` | created empty at start, 4 044 bytes at the end | end-only |
| jest `--json --outputFile=x.json` | 9 573 bytes at 2.6 s, run ends at 2.6 s | end-only (`testResultsProcessor` likewise runs on the aggregated result) |
| mocha `--reporter json` | 8 074 bytes, all at the end | end-only |
| Playwright `--reporter=json` with `PLAYWRIGHT_JSON_OUTPUT_NAME` | 17 388 bytes at the end | end-only |
| pytest-json-report `--json-report-file` | nothing for the whole 10 s run, 12 806 bytes at the end | end-only |
| `.pytest_cache` (`nodeids`, `lastfailed`) | directory appears at ~9 s of an 11 s run (session finish) | end-only; `nodeids` does give last run's total for free |
| cargo `target/` | nothing written during a run; nextest writes nothing per test | none |
| nextest `--message-format libtest-json` (needs `NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1`) | 0 lines at 2.5 s and 5 s of an 8 s single-threaded run, 26 lines at the end | end-only, and it replaces the human output |
| libtest JSON: `RUSTC_BOOTSTRAP=1 cargo test -- -Zunstable-options --format json` | **streams**: 9 lines at 0.6 s, 21 at 1.2 s (`{"type":"suite","event":"started","test_count":12}` first, then `started` / `ok` per test) | streams, on stable via the bootstrap var, but needs argv and replaces stdout |
| `go test -json` (also `GOFLAGS=-json`, verified to apply by env) | **streams**: `start` per package, `run` / `pass` / `fail` per test with timestamps; no total up front | streams, but replaces what the agent reads |
| nextest and cargo test human output to a pipe | both stream one line per test (checked to a file) | the agent's pipe, not ours |

So the only progress source worth building on is **a reporter of our own writing to a file**, and
the question becomes how to load it without the user's help.

### Three injection channels

1. **Environment.** Rust spawns each Session as `$SHELL -l` with an environment of its choosing.
   Verified that variables the app sets reach the Bash tool: `claude`'s environment (read with
   `ps -Eww -p <claude>`) has `TERM_PROGRAM=sidebar-term` and
   `__CFBundleIdentifier=com.bencooper.sidebarterm` from the app, and a Bash tool call sees the
   same two, plus Claude Code's own `CLAUDECODE=1`, `CLAUDE_CODE_ENTRYPOINT=cli`. The shell
   snapshot Claude Code sources restores functions, aliases and options, not the environment.
   Codex's `zsh -lc` and Gemini's `bash -c` inherit the same way. Everything below marked
   *env-only* was run with the variable set on the `bash -c` line and no change to the project.
2. **Claude Code hooks** (docs at https://code.claude.com/docs/en/hooks, read 2026-09-25).
   `PreToolUse` for `Bash` receives `tool_name: "Bash"`, `tool_input: {command, description,
   timeout, run_in_background}` and `tool_use_id`, plus the common `session_id`, `cwd`,
   `transcript_path`, `permission_mode`, `hook_event_name`; `PostToolUse` receives the same input
   again with `tool_response` (the page's Bash `tool_response` shape was not in the fetched
   excerpt: **unverified** whether it carries a duration). The lifecycle is `PreToolUse` →
   `PostToolUse` / `PostToolUseFailure` → `PostToolBatch`; **no event fires while a tool runs**, so
   hooks cannot carry progress. Command hooks run as `sh -c` in Claude Code's environment with
   `CLAUDE_PROJECT_DIR`; `async: true` runs one in the background. They live in
   `~/.claude/settings.json`, `.claude/settings.json`, `.claude/settings.local.json`, managed
   settings, or a plugin's `hooks/hooks.json` (the only way to add hooks without editing the user's
   settings, and the user must enable the plugin). What a hook would add over the process tree is
   the tool's `description` string and `tool_use_id`; not worth an install for v1.
3. **Repo configuration**: a `reporters` line in `vitest.config.*` / `jest.config.*`. One line,
   but per repo and per user.

### Per runner

`SIDEBAR_PROGRESS_FILE` below stands for the file the app names; in the product it is a directory
(`SIDEBAR_TERM_PROGRESS_DIR`, per Session) and each writer appends to `<pid>.jsonl` in it.

| Runner | Cheapest streaming source | Channel | Evidence |
|---|---|---|---|
| pytest | our `-p` plugin: `pytest_collection_finish` gives the total, `pytest_runtest_logreport` one line per test with outcome and duration | **env-only**: `PYTEST_ADDOPTS='-p sidebar_progress' PYTHONPATH=<dir with the plugin>` | 27 lines streamed over the 7.9 s run (`collected total=24` at 47 ms, a `case` every ~310 ms, the failure at 4.7 s); the agent's output unchanged (`1 failed, 23 passed in 7.89s`). With `-n 3` the controller still reports every test; the plugin must stay quiet in workers (`PYTEST_XDIST_WORKER` set), else each case is logged twice. Wall time 9.4–10.4 s without, 9.9–10.2 s with: no measurable cost |
| vitest | custom reporter (`onTestRunStart` → files, `onTestModuleCollected` → per-file totals, `onTestCaseResult` per test, reporter API v3, vitest ≥ 3) | repo config `reporters: ['default', '<path>']`, or `--reporter=` on the command line; **no env channel** (vitest reads only `VITEST_WORKER_ID`, `VITEST_MAX_WORKERS`, `NODE_OPTIONS`, …) | first `case` at 0.8 s, all 24 by 1.8 s; totals known at 0.5 s; agent output unchanged; 3.2–4.3 s vs 3.2–3.8 s wall time (noise) |
| jest | custom reporter (`onRunStart` gives `numTotalTestSuites` and an `estimatedTime`, `onTestCaseResult` per test since jest 26.2, `onTestFileResult`) | repo config `reporters: ['default', '<path>']` or `--reporters=`; **no env channel** (`JEST_WORKER_ID` only) | `run-start suites=6 estimatedTime=2` at 0 ms, cases stream from 2.2 s (jest's start-up), done at 3.4 s; agent output unchanged |
| vitest / jest, env-only alternative | `NODE_OPTIONS=--import=<shim.mjs>` that appends `--reporter=…` / `--reporters=…` to `process.argv` when `argv[1]` is `vitest` / `jest` and not in a worker | env-only, but every node process pays: 100 × `node -e 0` took 12.1 s with the shim vs 7.6 s without (**~45 ms each**) | 44 JSONL lines for each, agent output unchanged; works, not recommended |
| mocha | our reporter extending `Spec`, logging `start` (with `runner.total`), `pass`, `fail`, `end` | **env-only**: `MOCHA_OPTIONS='--reporter <path>'` (mocha allows one reporter, hence the wrapper; a repo `.mocharc` reporter is overridden) | streams (8 lines by 3.2 s), agent sees `11 passing (4s) 1 failing` as before |
| Playwright Test | our reporter: `onBegin` gives `suite.allTests().length` and workers, `onTestEnd` per test with status and duration | **env-only**: `PW_TEST_REPORTER=<path>` (`playwright/lib/runner/index.js:5221`) | streams (1 line at 2.6 s, 8 by 6.7 s), agent sees `11 passed (6.2s)` as before; the reporter is added to, not instead of, the configured one |
| cargo test | runner wrapper: `CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER=<wrapper>` runs `<binary> --list --format terse` (12 tests, ms) then `exec`s the binary unchanged | **env-only** | one `binary-start … tests=12` line; agent sees `test result: FAILED. 11 passed; 1 failed` as before. libtest runs tests as threads, so no per-test progress without `--format json` (which needs argv and replaces stdout). Only the total and the moment the build ended |
| cargo-nextest | the same wrapper: nextest launches the binary twice with `--list` and then **once per test** (`--exact tests::b3 --nocapture`), so each launch is a test start, its exit a test end; a wrapper that waits instead of `exec`ing sees the exit code (pass / fail) | **env-only** (nextest honours cargo's target runner) | 14 launches logged for 12 tests; agent sees `Summary [1.687s] 12 tests run: 11 passed, 1 failed` as before |
| go test | `-exec` wrapper: `GOFLAGS=-exec=<wrapper>` runs `<pkg>.test -test.list '.*'` (4 tests, ms) then `exec`s it; one line per package, package count from `go list` | **env-only** (`GOFLAGS` applies to every `go` command; `-json` also works this way but changes stdout) | three `binary-start … tests=4` lines (cwd is the package dir, so the path in the env must be absolute); agent output unchanged. Per test only with `-json` |
| rspec / phpunit / dotnet / gradle | rspec `--format` + a formatter class via `SPEC_OPTS` (env, documented); phpunit `--extensions` / `TestRunner` event API (config); `dotnet test --logger` (argv); gradle `--console`/listeners (config) | **unverified** | not installed |

Counting worker processes or their CPU (the issue's other idea) gives neither N nor M: vitest and
jest keep a worker pool alive for the whole run, pytest is one process, cargo test one binary.
nextest is the exception (one process per test) and the wrapper covers it better.

### The progress file protocol

One JSON object per line, appended, in `$SIDEBAR_TERM_PROGRESS_DIR/<writer pid>.jsonl`:

```
{"t":1790365752402,"ev":"start","runner":"vitest","pid":91300,"total":null}
{"t":1790365752867,"ev":"total","total":24}          # once known (pytest collection, vitest collect, playwright onBegin)
{"t":1790365753183,"ev":"case","status":"passed"}    # one per test; "failed" | "skipped"; name optional
{"t":1790365754191,"ev":"end","passed":23,"failed":1}
```

Rust matches a file to a runner by the writer's pid being the runner or one of its descendants
(the pytest plugin's pid is the runner; a cargo wrapper's pid is a child of `cargo`), tails the
directory on the monitor tick (a `stat` per file; kqueue later if it matters), and deletes files
whose writer is gone and whose run has been shown. Names are optional and never shown on the Tab.

## 3. ETA without progress

The app keeps `history.json` in its data dir: for each key `(repo commonDir, runner, command
signature)` the last ten `(started_at, duration_ms, outcome?)`. The signature is the runner plus
its positional arguments with paths normalised (`pytest tests/unit` and `pytest` are different
suites; `-q` is not), so the estimate matches what is being run. A run is recorded when its
runner exits; a run during which the Session was frozen is not.

- **History known**: bar = `min(elapsed / median, 1)`; text `1m 30s of ~2m 10s`; ETA is the
  median minus elapsed, rounded to 5 s under a minute and 15 s above, never negative.
- **Past the median**: bar stays full, text `2m 20s · usually ~2m 10s`.
- **Past the longest of the ten**: bar and text amber, `4m 05s · longer than usual` (the hang
  case); nothing more, the agent's own status will change when it notices.
- **One run in history**: same as above but `~` reads "about"; the tooltip says "from 1 run".
- **Empty**: elapsed only (`testing · 1m 30s`) with an indeterminate bar.
- Two-phase runners: the build phase counts in the duration (that is what the user waits for),
  but the row says `building` until the test binary appears, so a slow build is not read as a
  hung suite. Cache-warm and cache-cold builds will pull the median around; ten samples and a
  median cope.

What it buys: for the common case (the same `pnpm test` / `cargo test` / `pytest` every few
minutes in the same repo) the second run onwards has an ETA within a few seconds, which is what
the user asked for ("is it halfway or hung"). With a stream, the stream wins and history still
records, so the bar is never worse than the estimate.

## 4. Presentation

Tab row today (`src/lib/sidebar/TabRow.svelte`): icon slot (Agent status), Title, Badge on a
second line, the stats slot on the right (`35% · 1.21 GB`, hidden on hover behind the close
button), 28 px rows. The suite adds a **2 px bar under the Badge line** (full text width, in
`--status-running` blue, failures in red, history-only estimate in the same blue at half opacity,
indeterminate as a 30 % segment sliding) and **replaces the stats text** while a suite runs. The
icon slot, Title weight (Unread) and Agent status are untouched: the spinner keeps meaning "the
agent is working", the bar means "and here is the suite it is waiting on".

```
State 1, progress known (12 of 48 done, 2 failed, ETA ~1 min):

 ┌────────────────────────────────────────────────────────┐
 │ ◠  Claude Code                        12/48 · 2✗ · ~1m │
 │    ● sidebar-term · main                               │
 │    ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ │
 │    ▓▓ red    ▓▓ blue                                   │
 └────────────────────────────────────────────────────────┘

State 2, history only (1m 30s elapsed, usually ~2m 10s):

 ┌────────────────────────────────────────────────────────┐
 │ ◠  Claude Code                     1m 30s of ~2m 10s   │
 │    ● sidebar-term · main                               │
 │    ▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░ │
 │    ▒▒ blue at half opacity: an estimate                │
 └────────────────────────────────────────────────────────┘

State 3, nothing known (elapsed only):

 ┌────────────────────────────────────────────────────────┐
 │ ◠  Claude Code                       testing · 1m 30s  │
 │    ● sidebar-term · main                               │
 │    ░░░░░░░░░░░░▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░░░░ │
 │    a short segment sliding left to right               │
 └────────────────────────────────────────────────────────┘

Overrun (past the longest run in history), text and bar amber:

 │ ◠  Claude Code               4m 05s · longer than usual│
 │    ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓ │

Just finished (5 s), then the stats return:

 │ ◠  Claude Code                          48/48 · 2 failed│
 │    ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓ │
```

- Narrow sidebar: the stats text shrinks to the count alone (`12/48`), then disappears; the bar
  needs no width. The row's tooltip carries the full line (`vitest · 12 of 48 · 2 failed · 42 s,
  about 1 min left`).
- Hover: as today, the close button takes the stats slot; the bar stays.
- Frozen Tab: the snowflake and `frozen · 1.2 GB` win; the bar stays where it was (the runner is
  stopped) and elapsed stops counting until thawed.
- A plain Session running a suite by hand gets the same bar and text (the terminal icon stays).
- Two suites at once in one Tab (parallel Bash tool calls, a background run): the Tab shows the
  newest; the Panel lists both.

**Panel.** No new view: a suite is a fact about a Session's activity. The Activity view gets a
**"Tests" block above the process list**, one line per running suite, in the Tab's colour, which
goes to the Tab on click; the closed header keeps CPU and Memory Used (a suite is short-lived and
already on the Tab). Hidden when no suite runs.

```
 ┌ Activity ─────────────────────────────────────────────┐
 │ CPU  ▓▓▓▓▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  38% │
 │ Mem  ▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░  21.4 GB   │
 │                                                        │
 │ Tests                                                  │
 │ ● Claude Code   vitest   12/48 · 2 failed   42 s ~1m   │
 │   ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ │
 │ ● api           pytest   1m 30s of ~2m 10s             │
 │   ▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░ │
 │ ● Codex         cargo    building · 20 s               │
 │   ░░░░░░░░▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ │
 │                                                        │
 │ Process              CPU ▾       Mem                   │
 │ node                 212.0     1.02 GB                 │
 │ …                                                      │
 └────────────────────────────────────────────────────────┘
```

**Terminal area: nothing.** When the Tab is in view its row is on screen; a strip under the
Terminal would cover the agent's own output and compete with the Resume banner's slot.

**Lifecycle.** Appears on the first monitor tick that sees a runner under the Session (≤ 0.5 s
after the agent starts it, before its first test). Updates every tick from the progress file or
the clock. When the runner process exits it holds the final state (`48/48`, or `3 failed` in red)
for 5 s, then the stats return and history is written. Disappears at once when the Tab closes or
the Session is killed. It does not care whether the agent has moved on: a background run keeps its
bar until it ends; when the agent exits with a run still going, the bar stays on the now-plain
Session. A new run in the same Tab replaces the old one.

## 5. Failure signal

With a stream, yes, as it happens: pytest, vitest, jest, mocha and Playwright report each test's
outcome the moment it ends (the failing case landed at 4.7 s of the 7.9 s pytest run, 1.2 s of
the 1.8 s vitest run, 2.7 s of 3.4 s for jest); nextest's wrapper sees each test process's exit
code; go's `-exec` wrapper sees each package's. The bar grows a red segment at its left edge
sized by failed / total and the count turns red (`12/48 · 2✗`); the Panel line says `2 failed`.
cargo test (one binary, threads) has no per-test outcome without libtest JSON, and neither does
any suite without a stream: the runner's exit status is invisible to a non-parent (libproc reports
no exit code), so those runs end as `done` and the agent's own status carries the news.

## 6. Beyond tests

`cargo build`, `pnpm install` / `npm ci`, `docker build`, `go build`, `tsc`, `webpack` are the
same shape for detection (a recognisable process under the agent, in a repo), for elapsed time,
for the duration history and for the Tab row bar; only the classifier table and the text
(`building`, `installing`) differ. Test-specific are the reporters, the `N of M`, the failure
segment and the Panel's "Tests" heading. `cargo build` could even count `rustc` children against
`cargo metadata`'s crate count, and `docker build` has `--progress=rawjson` (argv, changes
stdout), but none of them streams to a file without changing what the agent reads. The follow-up
should model a `Job { kind: Test(runner) | Build(tool) | Install(tool), … }` so the second kind is a
table entry, but ship tests first and the history keyed by the same signature.

## Could not verify

- The `tool_response` shape `PostToolUse` receives for Bash (the fetched docs excerpt did not
  include it); irrelevant to the recommendation, which does not use hooks.
- Codex's and Gemini's shell tool trees live; both were read from source only.
- rspec, phpunit, `dotnet test`, `gradle test`: not installed, process shapes from docs.
- Rust-side cost of the descendant walk; 931 µs is the Python ctypes number for 32 descendants.
- Whether nextest's libtest-json streams on runs longer than 8 s (it wrote nothing until the end
  here); moot, since it replaces the human output.

## Sources

Read from this Mac:
- Process trees: `ps -o pid,ppid,pgid,tpgid,tty,stat,comm,args` on the running Claude Code
  (2.1.282) and on scratch runs under `bash -c` (scratch scripts `run.sh`, `tree.sh`,
  `watch.sh`, `exp-*.sh`, `procargs.py`, `ppidwalk.py`, since deleted); `ps -Eww` on `claude` for
  its environment; `env` in a Bash tool call.
- vitest 5.0.2 reporter API: `node_modules/vitest/dist/chunks/plugin.d.*.d.ts` (`onTestCaseResult`,
  `onTestModuleCollected`, `onTestRunStart`); env reads: `grep process.env node_modules/vitest/dist`.
- jest 30.5.2: `@jest/reporters/build/index.d.ts` (`onTestCaseResult`), `jest-config` defaults
  (`cacheDirectory`), `jest-worker/build/processChild.js`.
- mocha 12.0.2: `lib/` reads `MOCHA_OPTIONS`, `MOCHA_COLORS`.
- Playwright 1.63.0: `playwright/lib/runner/index.js:5221` (`process.env.PW_TEST_REPORTER`),
  `types/testReporter.d.ts` (`onBegin`, `onTestEnd`), `PLAYWRIGHT_JSON_OUTPUT_NAME`.
- pytest 9.1.1: `PYTEST_ADDOPTS`, `-p`, `pytest_collection_finish`, `pytest_runtest_logreport`;
  pytest-xdist 3.8.0 `PYTEST_XDIST_WORKER`; pytest-json-report 1.5.0.
- cargo 1.94.1: `target.<triple>.runner` via `CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER`;
  libtest `--format json` under `RUSTC_BOOTSTRAP=1`; `cargo test -- --list`.
- cargo-nextest 0.9.146: `cargo nextest run --help` (`[env: NEXTEST_MESSAGE_FORMAT]`,
  `NEXTEST_TEST_THREADS`, …), `NEXTEST_EXPERIMENTAL_LIBTEST_JSON`, per-test `--exact` processes.
- go 1.26.3: `go test -json`, `-exec`, `-list`, `GOFLAGS`, `go list`.
- Apple headers: `<sys/proc_info.h>` `PROC_PPID_ONLY 6`, `<libproc.h>` `proc_listpids`,
  `proc_pidpath` (as in `docs/research/agent-detection.md`).

Documentation and source online (2026-09-25):
- https://code.claude.com/docs/en/hooks: lifecycle table, common input fields, `PreToolUse` /
  `PostToolUse` input for Bash (`tool_input.command`, `description`, `timeout`,
  `run_in_background`, `tool_use_id`), matchers, hook types, `async` / `asyncRewake`, timeouts,
  hook locations incl. plugin `hooks/hooks.json`, "Shell form runs when `args` is absent. The
  `command` string is passed to a shell: `sh -c` on macOS and Linux".
- github.com/openai/codex `codex-rs/core/src/shell.rs` (`derive_exec_args`: `-lc` / `-c`),
  `codex-rs/core/src/spawn.rs` (`StdioPolicy::RedirectForShellTool`: stdin null, stdout / stderr
  piped; `kill_on_drop`), `codex-rs/core/src/exec.rs`.
- github.com/google-gemini/gemini-cli `packages/core/src/utils/shell-utils.ts`
  (`getShellConfiguration`: `bash` + `['-c']`), `packages/core/src/services/shellExecutionService.ts`
  (node-pty first, `child_process.spawn` with `detached: true` as fallback).

## Proposed follow-up issue

Title: **Show a running test suite's progress and ETA on the Tab**

Labels: `enhancement`, `ready-for-agent`

---

Follow-up to #33 (research in `docs/research/test-progress.md`). Claude Code, Codex and Gemini run
test suites from shells they spawn inside an Agent session; the output goes to the agent, so the
sidebar shows "Running" for minutes with no sign of whether the suite is halfway or hung.

## Prompt for the agent

Work in a fresh worktree off `main`. Read `CLAUDE.md`, `CONTEXT.md`, `docs/architecture.md`
("Agent status", "Panel", "Activity"), `docs/research/test-progress.md` and `docs/agents/*.md`
first. Use the glossary's vocabulary; add **Suite** (a test run the app has recognised under a
Session) to `CONTEXT.md`.

### Goal

While a test suite runs under a Session, its Tab shows how far along it is and how much longer to
expect, and the Activity view lists it; with no configuration for the common case, better numbers
where the runner lets the app inject a reporter through the environment.

### Why

The research found that no runner streams progress to a file on its own, but that every process
under a Session inherits the app's environment, Claude Code's Bash tool included, and that
pytest, mocha, Playwright, cargo test / nextest and go test each have an environment variable
that loads a reporter or a wrapper without touching the repo or the agent's output. Detection
needs only a descendant walk from the agent, which the app already knows how to do.

### What must hold

- **Detection (Rust, `detect/`)**: a `classify_runner(comm, path, argv) -> Option<Runner>` table
  for vitest, jest, mocha, Playwright, pytest (`pytest`, `python -m pytest`, `uv run pytest`),
  cargo test and its `target/*/deps/` test binary, cargo-nextest and its per-test processes, go
  test and its `*.test` binaries, with a unit-test table like `classify_agent_table`. On every
  monitor tick, walk the descendants of each Session's Foreground process group with
  `proc_listpids(PROC_PPID_ONLY)` (recursive; the runner is a grandchild of Claude Code, under a
  tty-less `zsh -c`) and of the group itself, so suites run by hand count too. Read argv only for
  interpreters and drivers (`needs_argv`). A Suite starts when its runner is first seen and ends
  when that pid is gone. Cost: microseconds per Session per tick; no `ps`.
- **Progress file protocol**: `session_spawn` sets `SIDEBAR_TERM_PROGRESS_DIR` to a per-Session
  directory under the app's temp dir; writers append `<pid>.jsonl` with `start` / `total` /
  `case` / `end` lines as the research specifies. Rust tails the directory on the monitor tick,
  matches a file to a Suite by the writer pid being the runner or a descendant, and removes files
  once their Suite is over. Malformed lines are skipped, never fatal.
- **Reporters shipped in the app bundle** and injected by environment at `session_spawn` when the
  "Suite progress" setting is on (default on): pytest plugin via `PYTEST_ADDOPTS` (appended, not
  replaced) and `PYTHONPATH` (appended), quiet under `PYTEST_XDIST_WORKER`; Playwright via
  `PW_TEST_REPORTER`; mocha via `MOCHA_OPTIONS` with a reporter that extends the configured or
  default `Spec`; cargo via `CARGO_TARGET_<triple>_RUNNER` with a wrapper that logs
  `--list`'s count, delegates to a runner set in the repo's `.cargo/config.toml` if there is one,
  and for nextest waits (not `exec`s) so it can report the exit code; go via `GOFLAGS=-exec=…`
  (appended). Each wrapper `exec`s or waits on the real binary with the same argv and exit code;
  the agent's stdout and stderr are byte-for-byte what they were. A vitest and a jest reporter
  ship too, documented as a one-line `reporters` entry pointing at `$SIDEBAR_TERM_REPORTERS` (an
  env var the app also sets), not injected.
- **History**: `history.json` in the app data dir, last ten durations per `(commonDir, runner,
  command signature)`, written when a Suite ends unless the Session was frozen during it.
  Estimate rules as in the research (median; amber past the longest; elapsed only when empty).
- **Event and types**: a `suite` event with `SuiteSnapshot { sessionId, runner, phase:
  building | testing | done, startedAt, elapsedMs, done, total, failed, etaMs, source: stream |
  history | none }` for every Session with a Suite, emitted on change each tick; mirrored in
  `src/lib/types.ts` and `src/lib/mock.ts` (`pnpm dev` shows a fake suite).
- **Tab row**: a 2 px bar under the Badge line and the count / ETA text in the stats slot, the
  three states and the overrun, finished-for-5-s, narrow, hover and frozen behaviours exactly as
  the research's mockups; Agent status icon and Unread untouched. Pure formatting in a
  `src/lib/suite/model.ts` with vitest tests.
- **Panel**: a "Tests" block above the Activity view's process list, one line per running Suite in
  the Tab's colour, click goes to the Tab; hidden when none. Closed header unchanged.
- **Settings**: a "Suite progress" toggle (env injection on / off; detection and history always on)
  with one sentence naming the variables it sets.
- **Docs**: `docs/architecture.md` gains a "Suites" section (detection, protocol, injected
  variables, history) and the IPC table the new event; `CONTEXT.md` gains Suite.
- Existing vitest and cargo tests keep passing; `pnpm check`, `pnpm test`, `cargo test` and
  `cargo clippy` clean.

### Out of scope

Build and install jobs (`cargo build`, `pnpm install`, `docker build`), Claude Code hooks, a strip
in the Terminal area, rspec / phpunit / dotnet / gradle, per-test progress for plain `cargo test`
(libtest runs tests as threads), reading the agent's transcript, and the Host daemon (the Rust
side of this lands in the core and carries over).

### Deliverable

One PR. In its description: the runner table, the injected variables and how each wrapper keeps
the agent's output unchanged, a screenshot of the three states, and what happens when the same
environment variable is already set by the user.
