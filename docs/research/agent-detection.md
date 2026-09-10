# Detecting Claude Code, Codex CLI and Gemini CLI as the Foreground process

Research for issue #3. Vocabulary per `CONTEXT.md`: a **Session** is one shell on its own pty; its
**Foreground process** is whatever holds the pty's foreground process group; an **Agent session** is a
Session whose Foreground process is one of the three agents.

Verified on this machine (macOS 26.5, Xcode 26.6 SDK) against: Claude Code 2.1.267 (native binary),
`@openai/codex` 0.120.0 npm launcher (the native `codex` binary it spawns was missing from this
install, so its behaviour comes from source), `@google/gemini-cli` 0.55.1 (Node bundle), plus the
docs and source trees named in Sources. Anything not verified is marked **unverified**.

## Recommendation

**Mechanism: poll the pty for its foreground process group and classify the processes in that group;
use the terminal's own OSC parser (xterm.js) for state; do not depend on hooks for presence.**

1. Every **1 s** per Session (and once ~150 ms after output resumes following silence, to catch a
   fresh `claude`/`codex`/`gemini` launch quickly), from the Rust side that owns the pty master:
   - `pgid = tcgetpgrp(master_fd)` (or `ioctl(master_fd, TIOCGPGRP)`; same kernel path).
   - If `pgid` is unchanged since last tick and the classification was "agent", skip the rest.
   - `proc_listpids(PROC_PGRP_ONLY, pgid, ...)` to get every pid in the foreground group. This
     matters because the agent is **not always the group leader** (Codex from npm runs under a
     `node` launcher that stays the leader; Gemini may relaunch itself as a child `node`).
   - For each pid: `proc_pidinfo(pid, PROC_PIDT_SHORTBSDINFO)` gives `pbsi_comm` (16 chars) and
     `pbsi_ppid`; `proc_pidpath(pid)` gives the resolved executable path.
   - Classify (first match wins):
     - `comm == "claude"` **or** path matches `*/claude/versions/*` or `*/.claude/local/*` -> Claude Code.
     - `comm == "codex"` -> Codex CLI (native Rust binary, any install method).
     - `comm == "node"` (or `bun`) -> read argv with `sysctl KERN_PROCARGS2` and match
       `@google/gemini-cli/bundle/gemini.js` -> Gemini; `@openai/codex/bin/codex.js` -> Codex
       launcher (the native child will also be in the group; either is enough);
       `@anthropic-ai/claude-code/cli.js` -> Claude Code via npm.
   - Otherwise: plain Session.
2. Cost: three to five syscalls per Session per tick, each in the microsecond range on libproc
   (this is how `ps`, `lsof` and node-pty's `pty.process` work); tens of Sessions at 1 Hz are
   negligible. **Unverified** on this machine: the exact per-iteration microseconds (test program
   `ptytest.c` is written but could not be executed in this run; see "Could not verify").
3. There is no macOS event for "foreground pgrp changed", so polling is the only presence source.
   Keep the tick at 1 s; the UI never needs sub-second presence.
4. State (working / waiting / finished) comes from what the agents already write to the pty:
   - **Title** (OSC 0/2): Codex and Gemini encode state in the title by default; Claude Code writes
     a conversation-derived title (not a state signal).
   - **Notifications** (OSC 9 / OSC 777 / BEL): all three can emit "turn complete" and "needs
     approval" as terminal notifications; xterm.js can intercept these. Some require user settings.
   - **Hooks** are the richest state source but are user-installed config, so treat as an optional
     "integration" layer, not the baseline (the map says the app only observes).

## Reading the Foreground process on macOS (primary sources on this machine)

| Fact | Source |
| --- | --- |
| `pid_t tcgetpgrp(int fd)` returns the foreground process group of the terminal; `-1`/`ENOTTY` if `fd` is not a controlling terminal of the caller. | `man 3 tcgetpgrp` (macOS 26.5) |
| `TIOCGPGRP` is "the underlying call" for the pgrp query; it is `_IOR('t', 119, int)`. | `man 4 tty`; `<sys/ttycom.h>:121` |
| `proc_listpids(uint32_t type, uint32_t typeinfo, void*, int)`; types `PROC_ALL_PIDS 1`, `PROC_PGRP_ONLY 2`, `PROC_TTY_ONLY 3`, `PROC_PPID_ONLY 6`. | `<libproc.h>:92`; `<sys/proc_info.h>:51-56` |
| `proc_pidinfo(pid, flavor, arg, buf, size)`; flavor `PROC_PIDT_SHORTBSDINFO` (13) fills `struct proc_bsdshortinfo { pbsi_pid, pbsi_ppid, pbsi_pgid, pbsi_status, char pbsi_comm[MAXCOMLEN] /* up to 16 chars */, ... }`. | `<libproc.h>:96`; `<sys/proc_info.h>:85-99,754` |
| `proc_pidpath(pid, buf, size)`; buffer `PROC_PIDPATHINFO_MAXSIZE` = `4*MAXPATHLEN`. | `<libproc.h>:102`; `<sys/proc_info.h>:749` |
| `proc_name(pid, buf, size)` exists for the long name. | `<libproc.h>:99` |
| `MAXCOMLEN 16`. | `<sys/param.h>:95` |
| `sysctl` `KERN_PROC` (14) with `KERN_PROC_PID 1` / `KERN_PROC_PGRP 2`; `KERN_PROCARGS2` (49) returns argv+env of a process. | `<sys/sysctl.h>:222,261,433-434` |
| `ps -E` shows another same-uid process's environment ("does not reflect changes after launch"), i.e. `KERN_PROCARGS2` works across processes of the same user without root. | `man ps`; verified with `ps -Ewww -p <claude pid>` |
| `PROC_PIDVNODEPATHINFO` (9) gives `pvi_cdir` — the process cwd — useful for the badge later. | `<sys/proc_info.h>:342-345,741` |

Observed process facts (this machine, `ps -o pid,ppid,pgid,tpgid,tty,stat,comm`):

- A `claude` launched from zsh in Apple Terminal: `PID 33832 PPID 33453 PGID 33832 TPGID 33832
  TTY ttys009 STAT S+ COMM claude`. It is its own group leader and the tty's foreground group.
  Its shell parent zsh keeps `PGID 33453` (background). Tool subprocesses it spawns (e.g. the
  `zsh -c` that ran these commands) have `TTY ??` and their own pgid — they never take the pty
  foreground, so presence stays stable while tools run.
- `comm` is derived from the exec path the shell used (`~/.local/bin/claude`, a symlink), so
  `pbsi_comm == "claude"` while `proc_pidpath` resolves to the target
  `~/.local/share/claude/versions/2.1.267` (a Mach-O arm64 executable). **Match on `comm` or on the
  `/claude/versions/` path segment, not on the path's basename.**
- Node-based agents show `comm == "node"`; the identifying string is only in argv.

Reference implementations (not run here, cited for the call pattern): node-pty's macOS
`pty.process` uses `tcgetpgrp(master)` then `sysctl KERN_PROC_PID` and returns `kp_proc.p_comm`
(`src/unix/pty.cc`); WezTerm's `procinfo` crate walks `proc_listpids`/`proc_pidinfo`/`proc_pidpath`
(`procinfo/src/macos.rs`). Both are evidence that the master fd is accepted by `tcgetpgrp` on
macOS; **verify** with `ptytest.c` before relying on it (see Could not verify).

## Detection table: Claude Code

| Signal | Detail | Reliability | Cost |
| --- | --- | --- | --- |
| Process name (native install, the default) | `~/.local/bin/claude` -> symlink to `~/.local/share/claude/versions/<ver>`; `comm = claude`; pgid == pid; foreground of the tty. | High | 3 syscalls/tick |
| Process name (npm `@anthropic-ai/claude-code`) | Runs under `node` with `cli.js` in argv. **Unverified** whether this package is still the current distribution; the docs and this machine use the native installer. | Medium | +1 sysctl for argv |
| Env vars exported to children | `CLAUDECODE=1`, `CLAUDE_CODE_CHILD_SESSION=1`, `CLAUDE_CODE_SESSION_ID=<uuid>`, `CLAUDE_PID=<pid>`, `CLAUDE_CODE_ENTRYPOINT=cli`, `CLAUDE_CODE_EXECPATH=<binary>`, `CLAUDE_CODE_MESSAGING_SOCKET=/tmp/cc-socks/<pid>.sock` (observed). Only present in *children* of claude, not in claude's own env, so useless for identifying the foreground pid itself; useful to confirm a `zsh`/`node` in the group is a claude child. | Low for presence | sysctl |
| Terminal title | Claude Code sets the tab title, generated from the conversation, or the `/rename` name (`terminalTitleFromRename`). Disable with `CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1`. Not a state signal; not a stable "I am Claude" marker. OSC code number **unverified** (escape bytes not recoverable from the compiled binary). | Low | free (xterm.js title event) |
| Progress bar | `terminalProgressBarEnabled` (default true): "reports it only in terminals that support the indicator: ConEmu, Ghostty 1.2.0+, iTerm2 3.6.6+". This is the ConEmu-style progress sequence; emitted only when the terminal is detected as one of those. **Unverified** which detection key (likely `TERM_PROGRAM`). | Medium if the app advertises support | free |
| Desktop notification / bell | `preferredNotifChannel`: `auto` (desktop notification in iTerm2, Ghostty, Kitty; bell in Terminal.app only if its audible bell is off; nothing elsewhere), `iterm2`, `iterm2_with_bell`, `kitty`, `ghostty`, `terminal_bell`, `notifications_disabled`. Binary confirms the same list and an "(OSC 777)" channel label; the auto path switches on `TERM_PROGRAM` (`Apple_Terminal`, `iTerm.app`, `kitty`, `ghostty`). | Medium (user/terminal dependent) | free |
| Hooks | Events include `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `Notification`, `Stop`, `StopFailure`, `SessionEnd`, `CwdChanged`. Hook types: `command`, `http` (POST JSON to a URL), `mcp_tool`, `prompt`, `agent`. Input JSON always has `session_id`, `cwd`, `hook_event_name`, `transcript_path`, `permission_mode`. Configured in `~/.claude/settings.json` etc. | High once installed | one process or HTTP call per event |
| Status line | `statusLine.command` runs with JSON on stdin (`session_id`, `cwd`, `model`, `workspace.current_dir`, `context_window`, `cost`...) on session start, each new assistant message, `/compact`, permission-mode change, optional `refreshInterval`; debounced 300 ms. Only one status line per user, so the app should not claim it. | High once installed | one process per update |

## Detection table: Codex CLI

| Signal | Detail | Reliability | Cost |
| --- | --- | --- | --- |
| Process name (npm `@openai/codex`) | `bin/codex.js` (`#!/usr/bin/env node`) resolves `@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/codex/codex` and `spawn`s it with `stdio: "inherit"`, forwarding SIGINT/SIGTERM/SIGHUP. Result: `node` is the pgrp leader and the tty foreground; the native `codex` child is in the same pgrp with `comm = codex`. Launcher also sets `CODEX_MANAGED_BY_NPM=1` (or `_BUN`) in the child's env. | High (enumerate the group) | 3 syscalls + per-member lookups |
| Process name (Homebrew / direct binary) | Native `codex` binary directly; `comm = codex`, group leader. **Unverified** locally (not installed). | High | 3 syscalls |
| Env vars for children | `CODEX_SANDBOX=1`-style markers: `CODEX_SANDBOX_NETWORK_DISABLED` and `CODEX_SANDBOX` constants in `core/src/spawn.rs`. Children only. | Low for presence | sysctl |
| Terminal title | Written via OSC 0 with BEL terminator: `"\x1b]0;{title}\x07"` (`tui/src/terminal_title.rs`), sanitised and capped at 240 visible chars. Default items `["activity", "thread-name", "project-name"]` (`tui.terminal_title` overrides). "activity" is a braille dot spinner (`⠋⠙⠹...`, 100 ms frames) while working; when blocked on the user the title is prefixed `"[ ! ] Action Required"`, blinking with `"[ . ] Action Required"` at 1 s. Title is cleared (empty OSC 0) on exit. | High for state | free |
| Desktop notification | `tui.notifications` (bool or list of `"agent-turn-complete"`, `"approval-requested"`, `"plan-mode-prompt"`); `tui.notification_method` `auto | osc9 | bel` (default auto); focus filter defaults to "unfocused" only. OSC 9 form: `"\x1b]9;{message}\x07"`, wrapped in tmux DCS passthrough when under tmux (`tui/src/notifications/osc9.rs`). Notification types: `AgentTurnComplete`, `ExecApprovalRequested`, `EditApprovalRequested`, `ElicitationRequested`, `PlanModePrompt` (`chatwidget/notifications.rs`). | Medium (needs setting; focus-gated) | free |
| External `notify` program | `notify = ["cmd", ...]` in `config.toml`: Codex runs the command with a JSON payload; verified fields `type: "agent-turn-complete"`, `input-messages`, `last-assistant-message` (`core/tests/suite/user_notification.rs`). Other fields **unverified**. | High once configured | one process per turn |
| Hooks | `features.hooks` gate; events `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`, `SessionStart`, `SessionEnd`, `SubagentStart`, `SubagentStop`, `UserPromptSubmit`, `Stop`, `Interrupt`; configured in `~/.codex/hooks.json`, `<repo>/.codex/hooks.json` or inline `[hooks]` in `config.toml`; command and MCP-tool handlers. Input fields (schema.rs): `session_id`, `turn_id`, `transcript_path`, `cwd`, `hook_event_name`, `model`, `permission_mode`, plus event fields (`stop_hook_active`, `last_assistant_message` on Stop). | High once installed | one process per event |

## Detection table: Gemini CLI

| Signal | Detail | Reliability | Cost |
| --- | --- | --- | --- |
| Process name (npm/npx) | `bundle/gemini.js` (`#!/usr/bin/env node`, ESM); `comm = node`; argv[1] ends in `@google/gemini-cli/bundle/gemini.js` (under npx it lives in the npx cache but the tail is the same). The bundle contains a self-relaunch path guarded by `GEMINI_CLI_NO_RELAUNCH: "true"` (`RELAUNCH_EXIT_CODE` in the entry file), so a second `node` child may exist in the group; match any member. No native binary. | High (argv match) | 3 syscalls + argv sysctl |
| Env vars for children | `GEMINI_CLI=1` (`GEMINI_CLI_IDENTIFICATION_ENV_VAR = "GEMINI_CLI"`, value `"1"`) applied to processes the CLI spawns. Children only. | Low for presence | sysctl |
| Terminal title | OSC 0 with BEL: `` `\x1B]0;${windowTitle}\x07` ``; cleared with `\x1B]0;\x07` on exit. `ui.dynamicWindowTitle` (default **true**) "Update the terminal window title with current status icons (Ready: ◇, Action Required: ✋, Working: ✦)"; `ui.showStatusInTitle` (default false) puts model thoughts in the title while working; `CLI_TITLE` env overrides the folder-name part (`process.env["CLI_TITLE"] || folderName`); `ui.hideWindowTitle` suppresses. | High for state (on by default) | free |
| Desktop notification | `general.enableNotifications` (default **false**); `general.notificationMethod` `auto | osc9 | osc777 | bell`. Prefixes in the bundle: `"\x1B]9;"` and `"\x1B]777;notify;"`; falls back to BEL. Fires for "Action required" (waiting for input / tool approval) and "Session complete". | Medium (off by default) | free |
| Hooks | Events `SessionStart` (`source`: startup/resume/clear), `SessionEnd` (`reason`: exit/clear/logout/prompt_input_exit/other), `BeforeAgent`, `AfterAgent` (agent loop ended), `BeforeModel`, `AfterModel`, `BeforeToolSelection`, `BeforeTool`, `AfterTool`, `PreCompress`, `Notification` (`notification_type: "ToolPermission"`, `message`, `details`). Stdin JSON: `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `timestamp`. Configured under `hooks` in `~/.gemini/settings.json` / `.gemini/settings.json`; `hooksConfig.enabled` gate. | High once installed | one process per event |

## What reveals state (working / waiting for input / finished)

Baseline (no user configuration), observable by xterm.js's OSC parser on the Session's output:

| Agent | Working | Waiting for input | Finished / idle |
| --- | --- | --- | --- |
| Codex | Title starts with a braille spinner frame and then the thread/project name. | Title starts with `[ ! ] Action Required` / `[ . ] Action Required` (alternating 1 s). | Title without spinner or prefix; empty title once the process exits. |
| Gemini | Title contains `✦` (Working). | Title contains `✋` (Action Required). | Title contains `◇` (Ready); empty title on exit. |
| Claude Code | Progress-bar OSC only if the app is detected as Ghostty/iTerm2/ConEmu (**unverified** detection key). Title is conversation text, not state. | Nothing by default in an unknown terminal. | Nothing by default. Process exit is the only free "finished" signal. |

With one settings entry from the user (opt-in integration the app could offer to write):

- Claude Code `preferredNotifChannel: "terminal_bell"` -> BEL (`xterm.js` `onBell`) on `permission_prompt`
  (about 6 s after the prompt appears, keystrokes defer it) and `idle_prompt` (about 60 s after the
  last response, only if the user has not typed). This is the cheapest cross-terminal Claude state
  signal but is delayed by design.
- Claude Code `http` hook on `UserPromptSubmit`, `PermissionRequest`, `Stop`, `SessionStart`,
  `SessionEnd` posting to a localhost port the app owns: immediate and exact working/waiting/finished
  with `session_id` and `cwd`. `Notification` hooks fire even with `notifications_disabled`.
- Codex `tui.notifications = true` + `tui.notification_method = "osc9"`: OSC 9 messages
  "Approval requested: ..." / turn-complete preview, but only when Codex believes the terminal is
  unfocused (focus reporting); `notify = [...]` gives an exact turn-complete event regardless of focus.
- Gemini `general.enableNotifications = true` with `notificationMethod = "osc777"` or `"osc9"`:
  OSC notifications on action-required and session-complete. Hooks `AfterAgent` / `Notification` for
  exact events.

Presence ends when the pgrp no longer contains an agent process; that is "finished" for the icon.
Do not infer "waiting" from output silence alone: Claude Code idles silently while waiting for input
and also while a long tool runs.

## Poll interval and cost

- Presence: 1 Hz timer per Session plus an early re-check ~150 ms after output resumes following
  >1 s of silence. Each tick is `tcgetpgrp` + `proc_listpids(PROC_PGRP_ONLY)` + per-member
  `proc_pidinfo`/`proc_pidpath` (2-4 members). Skip member lookups when the pgid is unchanged and
  the last classification was an agent. Expected cost: microseconds per Session per tick
  (**unverified** on this machine; run `ptytest.c`).
- State: zero polling; parse OSC 0/2 (title), OSC 9, OSC 777, OSC 9;4 (progress) and BEL in the
  xterm.js data path. `xterm.js` exposes `onTitleChange`, `onBell` and `parser.registerOscHandler`.
- Event-driven presence is not available on macOS (no kqueue/ notification for `tcsetpgrp`).
  `proc_listpids(PROC_TTY_ONLY, dev)` is an alternative enumeration but no cheaper.

## Could not verify (needs one run outside this session)

- Whether `tcgetpgrp()` on the pty **master** fd returns the pgid on macOS 26.5 or fails with
  `ENOTTY`. node-pty and WezTerm rely on it; `ptytest.c` (scratchpad) opens a pty with `openpty`,
  spawns a shell with `TIOCSCTTY`, and prints `tcgetpgrp(master)`, `ioctl(TIOCGPGRP)`, the group
  members via `proc_listpids(PROC_PGRP_ONLY)`, and per-iteration timing. If it fails, fall back to
  `proc_listpids(PROC_TTY_ONLY, st_rdev of the slave)` and pick the member whose `pbsi_pgid`
  matches `TIOCGPGRP` on the slave, or simply the newest member.
- The exact escape Claude Code uses for the title and the progress bar, and the terminal detection
  key; docs only say which terminals get them.
- Extra fields in Codex's `notify` payload beyond `type`, `input-messages`, `last-assistant-message`.
- Homebrew Codex process name and whether the npm `@anthropic-ai/claude-code` package is still current.

## Sources

Apple headers and man pages on this machine (`/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/include/`):
- `libproc.h` lines 92-111 (`proc_listpids`, `proc_listchildpids`, `proc_pidinfo`, `proc_name`, `proc_pidpath`).
- `sys/proc_info.h` lines 51-56 (`PROC_*_ONLY`), 85-99 (`proc_bsdshortinfo`), 342-345 (`proc_vnodepathinfo`), 741-755 (flavour numbers, `PROC_PIDPATHINFO_MAXSIZE`).
- `sys/param.h:95` (`MAXCOMLEN 16`); `sys/sysctl.h` 222, 261, 433-434 (`KERN_PROC`, `KERN_PROCARGS2`, `KERN_PROC_PID/PGRP`); `sys/ttycom.h:121` (`TIOCGPGRP`).
- `man 3 tcgetpgrp`, `man 3 tcsetpgrp`, `man 4 tty` (TIOCGPGRP), `man 1 ps` (`-E`).

Local observation:
- `which claude` -> `~/.local/bin/claude` -> `~/.local/share/claude/versions/2.1.267` (Mach-O arm64); `claude --version` = 2.1.267.
- `ps -o pid,ppid,pgid,tpgid,tty,stat,comm -p <claude>` and `ps -Ewww` on the same pid; `env` inside a Claude Code Bash tool (CLAUDECODE, CLAUDE_CODE_* values above); `/tmp/cc-socks/<pid>.sock`.
- `~/.nvm/versions/node/v24.13.1/lib/node_modules/@openai/codex/bin/codex.js` and `package.json` (0.120.0).
- `~/.nvm/versions/node/v24.13.1/lib/node_modules/@google/gemini-cli/bundle/*.js` (0.55.1): strings `\x1B]0;${windowTitle}\x07`, `OSC777_PREFIX = "\x1B]777;notify;"`, `OSC9_PREFIX = "\x1B]9;"`, `GEMINI_CLI_IDENTIFICATION_ENV_VAR = "GEMINI_CLI"`, `GEMINI_CLI_NO_RELAUNCH`, `process.env["CLI_TITLE"] || folderName`.
- Strings in the Claude Code binary: notification channel list `["auto","iterm2","terminal_bell","iterm2_with_bell","kitty","ghostty","notifications_disabled"]`, "(OSC 777)", `TERM_PROGRAM` switch (`Apple_Terminal`, `iTerm.app`, `kitty`, `ghostty`), `CLAUDE_CODE_DISABLE_TERMINAL_TITLE`, `sendIdleNotification`, `idleNudge`.

Claude Code docs (code.claude.com):
- https://code.claude.com/docs/en/hooks (events table; Notification matchers and timing: `permission_prompt` ~6 s, `idle_prompt` ~60 s; common input fields; hook types incl. `http`; Stop fields).
- https://code.claude.com/docs/en/statusline ("When it updates", 300 ms debounce, `refreshInterval`, JSON fields).
- https://code.claude.com/docs/en/settings-reference (`preferredNotifChannel` values, `terminalProgressBarEnabled`, `terminalTitleFromRename`).
- https://code.claude.com/docs/en/env-vars (`CLAUDECODE`, `CLAUDE_CODE_CHILD_SESSION`, `CLAUDE_CODE_SESSION_ID`, `CLAUDE_PID`, `CLAUDE_CODE_DISABLE_TERMINAL_TITLE`).
- https://code.claude.com/docs/en/terminal-config ("Get a terminal bell or notification"; tmux `allow-passthrough`).

Codex (github.com/openai/codex, `main`, and developers.openai.com/codex):
- `codex-rs/tui/src/terminal_title.rs` (OSC 0 + BEL, sanitising, 240-char cap, clear on exit).
- `codex-rs/tui/src/chatwidget/status_surfaces.rs` (`DEFAULT_TERMINAL_TITLE_ITEMS`, spinner frames/100 ms, `[ ! ] Action Required` blink 1 s, `tui.terminal_title`).
- `codex-rs/tui/src/notifications/osc9.rs` (OSC 9 format, tmux DCS passthrough).
- `codex-rs/tui/src/chatwidget/notifications.rs` (notification enum and type names `agent-turn-complete`, `approval-requested`, `plan-mode-prompt`).
- `codex-rs/core/src/spawn.rs` (`CODEX_SANDBOX`, `CODEX_SANDBOX_NETWORK_DISABLED`); `codex-rs/core/tests/suite/user_notification.rs` (notify payload fields).
- `codex-rs/hooks/src/schema.rs`, `codex-rs/hooks/src/events/stop.rs` (hook event names and input fields).
- https://developers.openai.com/codex/config-reference (`notify`, `tui.notifications`, `tui.notification_method`, focus default, `features.hooks`, `hooks.<Event>`); https://developers.openai.com/codex/hooks (hooks.json locations).
- `codex-cli/bin/codex.js` as installed (launcher behaviour).

Gemini CLI (github.com/google-gemini/gemini-cli, `main`):
- `docs/reference/configuration.md` (`general.enableNotifications`, `general.notificationMethod`, `ui.dynamicWindowTitle` with ◇ ✋ ✦, `ui.showStatusInTitle`, `ui.hideWindowTitle`, `hooks.*`, `hooksConfig`, `CLI_TITLE`).
- `docs/cli/notifications.md` (OSC 9 with BEL fallback; action-required and session-complete events).
- `docs/hooks/index.md`, `docs/hooks/reference.md` (event table, base input schema, `Notification` `ToolPermission`, `SessionStart`/`SessionEnd` fields).

Reference implementations (call pattern only):
- node-pty `src/unix/pty.cc` (`PtyGetProc`: `tcgetpgrp` + `sysctl KERN_PROC_PID` -> `p_comm`).
- WezTerm `procinfo/src/macos.rs` (`proc_listpids` / `proc_pidinfo` / `proc_pidpath` walk).
