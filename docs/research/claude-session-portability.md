# Claude Code conversation portability across machines

Research for issue #31 (part of #24, decides how #30 Handoff treats Claude Code
conversations). Question: when a Tab's Session is a Claude Code conversation and the
Tab is handed off to a Host where the same repo is checked out at a different absolute
path, can the Host run `claude --resume <id>` on a copy of the conversation and carry
on? Machine facts below were checked on this dev box (macOS Darwin 25.6.0 arm64,
Claude Code 2.1.282, Codex CLI 0.154.0, Gemini CLI 0.55.1) on 2026-09-25; the docs
quoted are the current ones at code.claude.com (docs.claude.com redirects there).

## Recommendation

Copy files: **yes, one file, and not "only when paths match"**. A Claude Code
conversation is exactly one JSONL transcript, `~/.claude/projects/<project>/<id>.jsonl`,
where `<project>` is the *working directory path* with every non-alphanumeric character
replaced by `-`; nothing in the file or its directory name has to match the Host's
path. `claude --resume <id>` (2.1.223 and later) searches the current project directory
and its worktrees first and then every other project directory, so the copy resumes
whether it sits under the old Mac-derived name or under the Host's derived name; and
`claude --resume /absolute/path/to/<id>.jsonl` resumes a transcript kept anywhere,
with nothing under `projects/` at all. The one thing that breaks it is a *duplicate*:
two copies of the same id in two project directories, neither of them the current one,
make Claude Code report `No conversation found` on purpose. So Handoff should ship the
single `.jsonl` (optionally the project's `memory/` directory beside it), put it under
`<config dir>/projects/<key derived from the Host's checkout path>/`, delete it on the
Mac side once the Host has it (Handoff kills the Session here anyway), and record the
Resume entry as `claude --resume <id>` to run from the matching checkout. The resumed
conversation behaves: every transcript line carries the old `cwd`, but the new process
puts the new cwd in the system context, and in the experiment the agent noticed the move
unprompted and wrote to the new path. Two things the Host must have on its own and that
no file copy provides: a Claude login (credentials are per machine and per config
directory) and, for an interactive resume at a path Claude Code has not seen, one
"Yes, I trust this folder" answer; `-p` runs skip that dialog. The transcript format is
internal and "changes between versions", so keep the Mac and the Host on the same
Claude Code version. Codex CLI and Gemini CLI both have resumable, on-disk sessions
too (Gemini even has `--session-file` import), so `CONTEXT.md`'s "not resumed" for them
is a choice, not a limit.

## 1. Where a conversation lives, and what `--resume` needs

**Path and key.** Docs: "Claude Code stores transcripts as JSONL at
`~/.claude/projects/<project>/<session-id>.jsonl`, where `<project>` is your working
directory path with non-alphanumeric characters replaced by `-`. For a working directory
whose converted name exceeds 200 characters, Claude Code truncates the name to 200
characters and appends a hash of the full path". Observed: a `claude -p` run in
`/private/tmp/claude-501/-Users-bencooper-Dev-sidebar-term/13b2…/scratchpad/srcparent/portability-lab`
wrote
`~/.claude/projects/-private-tmp-claude-501--Users-bencooper-Dev-sidebar-term-13b2899c-a981-49f4-8f3b-49312d5a09f2-scratchpad-srcparent-portability-lab/e4a3ccd1-89b2-4b2d-b752-1cfe0b6a02ae.jsonl`
(note `/-` becomes `--`). The key is the cwd, not a project id, not the git root; two
worktrees of one repo get two project directories. `CLAUDE_CONFIG_DIR` moves the whole
tree ("All settings, session history, and plugins are stored under this path"), and
since 2.1.234 `CLAUDE_CODE_PROJECT_DIR_NAME`, set together with `CLAUDE_CONFIG_DIR`,
names `<project>` yourself "whatever the working directory is" (1–64 letters, digits,
`-`, `_`), which a Host daemon could use to sidestep the path key entirely.

**What is in the file.** One JSON object per line. Line types seen in the scratch
transcript: `queue-operation`, `user`, `assistant`, `attachment`, `atis-latch`,
`last-prompt`, `cost-state`. Every `user`/`assistant`/`attachment` line carries `cwd`,
`gitBranch`, `version` (`2.1.282`), `sessionId`, `entrypoint`, `uuid`, `parentUuid`,
`timestamp`. The `cwd` is the absolute Mac path and is not rewritten on resume. Docs:
"The entry format is internal to Claude Code and changes between versions, so scripts
that parse these files directly can break on any release."

**What else exists, and whether `--resume` needs it.**

| Path | What | Needed to resume? |
|---|---|---|
| `projects/<project>/<id>.jsonl` | the conversation | yes, the only thing (experiment C4) |
| `projects/<project>/<id>/subagents/`, `…/tool-results/` | subagent transcripts, large tool outputs spilled to files (docs) | not for the main conversation; a spilled tool result referenced by path would be missing |
| `projects/<project>/memory/` | auto memory (`MEMORY.md` + notes); keyed by the git repo root, "so all worktrees and subdirectories within the same repo share one auto memory directory" | no; it is loaded fresh from the *new* cwd's project directory, so it does not follow the conversation unless copied (see 3) |
| `session-env/<id>/` | "Per-session environment metadata" (empty directory here) | no |
| `file-history/<id>/` | pre-edit snapshots for checkpoint rewind | no; rewind of edits made on the Mac would be unavailable on the Host |
| `~/.claude.json` → `projects["<absolute cwd>"]` | `allowedTools`, `mcpServers`, `hasTrustDialogAccepted`, `enabledMcpjsonServers`, `lastSessionId`… (118 entries on this box; none was written for the scratch repo by `-p` runs) | no, but see 3: per-path MCP servers and the trust answer do not travel |
| `history.jsonl` | prompts typed interactively, with project path | no |
| `settings.json`, `.claude/settings*.json`, `CLAUDE.md` | docs: "re-read at launch, so configuration that lives in them doesn't need to be passed again" | read from the Host's own copies |

Retention: transcripts are deleted after `cleanupPeriodDays` (default 30) by the sweep
in whichever `~/.claude` they sit.

## 2. Copied to a different absolute path: does `--resume <id>` find it?

Docs (Manage sessions): "You can run `claude --resume <session-id>` from any directory:
Claude Code looks for the ID in the current project directory and its git worktrees
first, then in every other project on this machine, so it finds a session that started
elsewhere or moved with `/cd`. The cross-project search resolves the ID only when
exactly one other project holds a transcript with messages for it, so a hand-copied
duplicate makes Claude Code report not-found rather than resume an arbitrary copy. […]
Before v2.1.223, the lookup stopped at the current project directory and its git
worktrees, so you had to resume from the directory the session last worked in." And
(CLI reference, `--resume`): "In place of an ID, you can pass the absolute path to a
session's `.jsonl` transcript file".

Experiment. The scratch repo was moved from `…/scratchpad/srcparent/portability-lab`
(the source path was then renamed away, so old absolute paths dangle) to
`…/scratchpad/dstparent/portability-lab`; call the two derived keys OLDKEY and NEWKEY.
Each run was `claude -p --resume <id> --output-format json "What word did I ask you to
remember, and what absolute path did you read README.md from?"` from the new path, with
a pristine copy of the transcript placed as described. All under the default `~/.claude`.

| Case | Transcript placed at | Result |
|---|---|---|
| A | `projects/OLDKEY/<id>.jsonl` only (nothing renamed) | resumed; answered "pelican" and the old `srcparent` path, and added on its own: "the working directory has since moved to a sibling folder named dstparent" |
| C2 | `projects/NEWKEY/<id>.jsonl` only (renamed to the target's key) | resumed, same answer; continuation appended to that file |
| C3 | both `projects/NEWKEY/` and `projects/OLDKEY/` | resumed; the *current* project's copy grew (201 855 → 211 934 bytes), the other stayed pristine |
| C3b | `projects/OLDKEY/` and `projects/-home-ben-Dev-portability-lab/` (two *other* projects, none current) | `No conversation found with session ID: e4a3ccd1-…`, exit 1 |
| C4 | `…/scratchpad/elsewhere/<id>.jsonl`, nothing under `projects/`; passed the absolute path to `--resume` | resumed; the continuation was appended to *that* file (201 855 → 211 925 bytes) and nothing was created under `projects/` |
| C5 | nowhere | `No conversation found with session ID`, exit 1 |
| I | `projects/NEWKEY/` only, interactive `claude --resume <id>` on a pty via `expect` | first the workspace-trust dialog for the new path ("Is this a project you created or one you trust?"); after "Yes, I trust this folder" the prior turns rendered, ending with `The word you asked me to remember is "pelican".` |

So renaming the directory to the target's key works, and is the tidy choice (the
session then appears in that directory's `/resume` picker, and its `memory/` sits
beside it), but it is not required. The rule to enforce is "one copy of an id per
config directory".

**Different home.** With `CLAUDE_CONFIG_DIR=…/otherhome/.claude` (or `HOME=…/otherhome2`)
and the transcript copied under either key, `claude -p --resume <id>` found the
transcript but returned `Not logged in · Please run /login` (exit 1). Credentials are
per config directory (docs: macOS Keychain, or `~/.claude/.credentials.json` mode 0600
on Linux) and this experiment did not move them, so the lookup-in-a-fresh-config-dir
case rests on the docs quote plus cases A–C5, which exercise the same lookup in the
default directory. On the Dell the login is a one-time `claude` setup step already listed
in #24.

## 3. Does the resumed conversation behave?

Probe A2: with the transcript under NEWKEY and the old path gone, `claude -p --resume
<id> --allowedTools Read,Edit,Write,Bash "Append the line 'moved' to README.md. Then
tell me the absolute path you wrote to and whether any command failed."` The agent
wrote to `…/dstparent/portability-lab/README.md`, reported "No command failed", and
added: "The old path under `srcparent` no longer exists, so I wrote to the new location
under `dstparent`, which is the current working directory. The old memory directory path
also no longer applies, so the pelican memory index is no longer loaded in this
session." That is the expected shape: the system context of the new process names the
new cwd, and the model reconciles the stale paths in old tool results against it.

What does and does not carry over, from the docs' "What a resumed session restores"
plus the experiment:

- Restored from the file: full history with tool calls and results, the model, the
  agent (`--agent`), permission mode for interactive terminal resumes (a `-p` resume
  starts in the mode a new `-p` run would), an active goal, unexpired scheduled tasks.
- Not restored, pass again: `--mcp-config`, `--settings`, `--plugin-dir`,
  `--fallback-model`, `--add-dir` directories (and `/add-dir` ones). Project MCP servers
  in the repo's `.mcp.json` come with the checkout; servers stored per path in
  `~/.claude.json` (`projects["<mac path>"].mcpServers`) do not, and neither do
  `allowedTools` grants made for that path. "Allow for this session" grants never
  survive a new process anyway.
- A tool that was mid-flight when the Mac Session was killed: "Claude sees the call
  marked as cut off before its result was recorded and is told to check whether it
  took effect before running it again" (2.1.281+). Handoff kills the Session on the Mac,
  so this is the normal case, and it is handled.
- Misleading remnants: every old tool result quotes Mac paths (`/Users/bencooper/…`),
  which the Host's Linux paths will never match; large tool results spilled to
  `projects/<project>/<id>/tool-results/` are referenced by absolute path from the
  transcript and will be missing unless that directory is copied too; auto memory is
  read from `projects/<Host key>/memory/`, so copy `memory/` next to the transcript if
  the memory notes should follow (they are keyed by repo root, so one copy serves every
  worktree of the repo on the Host); checkpoint rewind for Mac-side edits is gone.
- Worktrees: sessions from another worktree of the same repo are found first-class
  ("current project directory and its git worktrees"), so a Tab whose Session sat in a
  worktree resumes from the Host's main checkout too, provided the Host has the same
  worktree or the transcript is placed under the checkout's key.
- Trust: the first interactive `claude` at a path not in `~/.claude.json` shows the trust
  dialog before anything else (case I); `-p` does not. For the Resume banner on the Host,
  the user answers once per checkout path.

## 4. Anything cleaner than copying files?

Checked in the current CLI reference, Manage sessions, Non-interactive mode and
Environment variables pages:

- `--session-id <uuid>`: "Use a specific session ID for the conversation (must be a
  valid UUID)". It names a *new* conversation; there is no import side. Useful for
  Handoff in the other direction: New Tab on a Host can choose the id up front, so the
  Resume entry is known before the first turn.
- `--resume <absolute path to .jsonl>`: the documented "import" (case C4). Handoff can
  drop the file anywhere and resume by path; Claude Code then keeps appending there.
- `--fork-session`: "When resuming, create a new session ID instead of reusing the
  original". Use it if the Mac copy is kept and both sides may continue.
- `--continue`: most recent conversation *in the current directory*; skips `-p`
  sessions unless `-p --continue`. Not useful across machines.
- `/export`: "copy the current conversation to your clipboard or save it as a plain-text
  file, with messages and tool outputs rendered as readable text". Human-readable, not
  re-importable.
- `--teleport`: "Resume a cloud session in your local terminal" (claude.ai/code only).
  Remote Control keeps a copy of the transcript on Anthropic servers "to sync the
  conversation across devices" but exposes no export. Neither moves a local session
  between two of your machines.
- `CLAUDE_CONFIG_DIR` + `CLAUDE_CODE_PROJECT_DIR_NAME` (2.1.234+): a Host that runs
  each Session with its own config directory can pin the project directory name and
  stop deriving it from the path; then the transcript's location is fully under the
  daemon's control on both machines. Costs a login per config directory.
- `CLAUDE_CODE_SKIP_PROMPT_HISTORY` / `--no-session-persistence`: the opposite knob;
  a Session started with either cannot be resumed anywhere.

So there is no supported export/import; copying the `.jsonl` is the mechanism, and
`--resume <path>` makes it explicit.

## 5. Codex CLI and Gemini CLI, briefly

Both are installed here, so `--help` output was checked alongside their docs.

**Codex CLI 0.154.0.** `codex resume [SESSION_ID|name] [PROMPT]` ("picker by default;
use `--last` to continue the most recent"), `--all` ("disables cwd filtering and shows
CWD column"), plus `codex fork`, `archive`, `delete`, `migrate-rollouts`. Sessions are
`~/.codex/sessions/YYYY/MM/DD/rollout-<timestamp>-<uuid>.jsonl`; the first line is
`session_meta` with `id`, `cwd`, `cli_version`, `git.{commit_hash,branch,repository_url}`.
There is also an index, `~/.codex/state_5.sqlite`, table `threads(id, rollout_path, cwd,
title, …)`, so a copied rollout is not visible to the picker until indexed
(`migrate-rollouts` exists for "legacy local sessions"). Docs: when the current and
saved directories differ, "Codex asks which directory to use", settable with
`tui.resume_cwd` = `current` | `session`, and `--cd` overrides. Resumable, path-aware,
and already prompts for the moved-cwd case; the index is the extra step.

**Gemini CLI 0.55.1.** `-r, --resume` ("latest", an index, or the session UUID),
`--list-sessions`, `--delete-session`, `--session-id` ("Start a new session with a
manually provided UUID") and `--session-file` ("Load a session from a JSON file"), the
last two only in `--help`, not yet in the docs. Docs: "Sessions are stored in
`~/.gemini/tmp/<project_hash>/chats/`, where `<project_hash>` is a unique identifier
based on your project's root directory" and "Sessions are project-specific". Observed:
`~/.gemini/projects.json` maps absolute path → short name, chats live at
`~/.gemini/tmp/<name>/chats/session-<timestamp>-<shortid>.json`, and each file carries
`sessionId` and a `projectHash` (a SHA-256, not of the bare path). `--session-file` is
a real import: point it at the copied JSON and the path key does not matter.

So `CONTEXT.md`'s "Codex and Gemini conversations are not resumed" is a scope decision
that could be revisited once #30 lands for Claude Code: Gemini needs one file and one
flag; Codex needs the rollout file plus a `codex resume <uuid>` that will ask about the
cwd.

## Could not verify

- A resume inside a fresh config directory (`CLAUDE_CONFIG_DIR` / other `HOME`): the
  lookup found the file but the run stopped at `Not logged in`. Credentials were not
  moved for this experiment. The Dell will have its own login.
- Linux specifically: this box is macOS. The docs describe one layout for both; only
  credential storage differs (Keychain vs `.credentials.json`).
- A long conversation (>100k tokens, idle >1 h) gets the "Resume from summary" dialog
  on Pro/Max; not exercised.
- Whether an interactive resume at the new path accepted a *new* prompt: the scripted
  keystrokes after the history rendered did not register within the timeout, so case I
  proves lookup and rendering, and `-p` cases prove continuation.

## Sources

- Manage sessions: https://code.claude.com/docs/en/sessions (resume lookup order,
  duplicate rule, `--resume <transcript-path>`, what a resumed session restores,
  "Where transcripts are stored", `CLAUDE_CODE_PROJECT_DIR_NAME`, `/export`)
- CLI reference: https://code.claude.com/docs/en/cli-reference (`--resume`,
  `--continue`, `--session-id`, `--fork-session`, `--name`, `--no-session-persistence`,
  `--teleport`)
- Non-interactive mode: https://code.claude.com/docs/en/headless ("Continue
  conversations", resume by path)
- Environment variables: https://code.claude.com/docs/en/env-vars (`CLAUDE_CONFIG_DIR`,
  `CLAUDE_CODE_PROJECT_DIR_NAME`, `CLAUDE_CODE_SKIP_PROMPT_HISTORY`,
  `CLAUDE_CODE_RESUME_INTERRUPTED_TURN`)
- The `~/.claude` directory: https://code.claude.com/docs/en/claude-directory
  (application-data table, retention sweep)
- Data usage: https://code.claude.com/docs/en/data-usage (local transcripts under
  `~/.claude/projects/` for 30 days, Remote Control sync)
- Memory: https://code.claude.com/docs/en/memory (auto memory at
  `~/.claude/projects/<project>/memory/`, keyed by git repo)
- Authentication: https://code.claude.com/docs/en/authentication (credential storage
  per OS)
- Codex CLI command reference: https://developers.openai.com/codex/cli/reference
  (redirects to learn.chatgpt.com/docs/developer-commands?surface=cli): `resume`,
  `fork`, `--all`, `tui.resume_cwd`
- Gemini CLI session management: https://geminicli.com/docs/cli/session-management/
- Local: `claude --version` = 2.1.282; `codex --version` = codex-cli 0.154.0;
  `gemini --version` = 0.55.1; experiment scripts in the session scratchpad
  (`exp.sh`, `interactive.exp`), session id `e4a3ccd1-89b2-4b2d-b752-1cfe0b6a02ae`.
