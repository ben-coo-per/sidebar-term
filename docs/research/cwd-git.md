# Session cwd, repo, worktree and branch detection

Research for issue #4. Question: how does the app learn each Session's cwd and,
from it, the repo, Worktree and branch for the Badge, without the user installing
shell integration by hand? Machine facts below were checked on this dev box
(macOS Darwin 25.5, git 2.51.1, zsh 5.9, /bin/bash 3.2.57, no fish, rustc 1.94.1).

## Recommended pipeline

| Stage | Choice | Fallback | Cost (measured here) |
|---|---|---|---|
| cwd source | `proc_pidinfo(pid, PROC_PIDVNODEPATHINFO)` on the shell pid (and on the Foreground process pid from `tcgetpgrp(master)`) | OSC 7 from the shell when it arrives (fish 4+, or our injected zsh/bash hook) | ~1 us per `proc_pidinfo`, ~4 us per `tcgetpgrp`; no permissions needed for same-uid processes |
| git query | Read `.git` files directly in Rust: walk up to `.git`, parse `gitdir:`, read `commondir`, `HEAD`, `worktrees/*/gitdir` | `git2` (libgit2) or `gix` (pure Rust, 60+ crates) for anything beyond HEAD/worktree listing; never shell out on the hot path | file reads: microseconds; `git2`/`gix` discover+HEAD: 0.2–0.9 ms; `git rev-parse` subprocess: ~1.4–1.6 ms per call on an idle machine (mostly fork/exec) |
| refresh trigger | (a) OSC 133 `D` / `A` (prompt boundary) when shell integration is present, (b) `notify` FSEvents watch on the worktree's git dir (`HEAD` is replaced by rename) and on `<common>/worktrees/`, (c) a 2 s poll of `proc_pidinfo` cwd as the floor, because cwd changes do not touch the filesystem | poll only | (a) free, (b) one FSEvents stream per Session, (c) ~1 us per Session per tick |
| unknown cwd | If the Foreground process is `ssh`, `docker`, `mosh`, `kubectl` etc., or an OSC 7 arrives whose host is not this Mac, mark the Session "remote": Badge shows the host from OSC 7 (or nothing), never the local cwd of the `ssh` binary | | |

Why not OSC 7 as the primary source: on macOS only fish (4.0+) emits it by default;
zsh and bash emit it only via Apple's `/etc/zshrc_Apple_Terminal` /
`/etc/bashrc_Apple_Terminal`, which are gated on `TERM_PROGRAM=Apple_Terminal`. The
kernel already knows every process's cwd and hands it over in a microsecond, with no
dotfile changes, so OSC 7 becomes an accelerator (instant update, remote-host hint),
not a dependency. Auto-injection (Ghostty/iTerm2/kitty pattern) is still worth doing
for OSC 133 prompt marks, which are the best refresh trigger and the best "command
finished" signal for the agent-state work; the same injected hook can emit OSC 7.

## 1. cwd via libproc

**API.** `int proc_pidinfo(int pid, int flavor, uint64_t arg, void *buffer, int
buffersize)` in `<libproc.h>`, available since macOS 10.5. Flavor
`PROC_PIDVNODEPATHINFO` (= 9) fills `struct proc_vnodepathinfo { struct
vnode_info_path pvi_cdir; struct vnode_info_path pvi_rdir; }`; `pvi_cdir.vip_path`
is a `char[MAXPATHLEN]` holding the cwd, `pvi_rdir` the chroot root. The struct is
2352 bytes; the call returns the number of bytes filled (2352 on success) or 0 on
failure with `errno` set. The header itself says these are "private interfaces to
obtain process information. These interfaces are subject to change in future
releases." They have been stable since 10.5 and are what `lsof`, iTerm2 and
`libproc`-based crates use, but there is no App Store review guarantee.

**Related flavors.** `PROC_PIDT_SHORTBSDINFO` (= 13) gives `pbsi_comm` (16-char
process name), `pbsi_ppid`, `pbsi_pgid`, `pbsi_uid` at ~0.27 us/call;
`PROC_PIDTBSDINFO` (= 3) adds `e_tdev` (controlling tty) and `e_tpgid` (tty
foreground pgid). `proc_pidpath(pid, buf, len)` returns the executable path
(`/bin/zsh`). `proc_listchildpids(ppid, buf, len)` lists children (~70 us).

**Permissions.** xnu's `proc_security_policy()` compares the caller's uid with the
target's; a mismatch returns `EPERM` unless the caller holds
`PRIV_GLOBAL_PROC_INFO` (root). Verified: `proc_pidinfo(1, PROC_PIDVNODEPATHINFO)`
from uid 501 returns 0 with `errno=1 (EPERM)`, while `PROC_PIDT_SHORTBSDINFO` and
`proc_pidpath` on pid 1 still succeed (name and path are not uid-gated). Every
process on a Session's pty is spawned by the app as the same user, so the cwd read
always works for shells and their children; it fails only for `sudo`'d foreground
processes (then fall back to the shell's own cwd). No TCC prompt, no entitlement.
Not verified: behaviour under App Sandbox (Tauri direct-distribution builds are not
sandboxed; if the app is ever sandboxed for the App Store this needs a retest).

**Cost.** Measured with a forkpty'd `/bin/zsh`, 100 000 iterations, release build:
`PROC_PIDVNODEPATHINFO` 0.94 us/call, `PROC_PIDT_SHORTBSDINFO` 0.27 us/call,
`tcgetpgrp(master)` 4.1 us/call. The kernel does a `vn_getpath()` on the cwd vnode
per call (no process lock held during path build). Polling 50 Sessions at 2 Hz is
~100 us/s of CPU: negligible.

**Which pid.** The shell pid is what `portable-pty`'s `Child::process_id()` returns
after `spawn_command`. The Foreground process is the leader of the pty's foreground
process group: `tcgetpgrp(master_fd)` works on the master side on macOS
(`portable-pty::MasterPty::process_group_leader()` is literally `libc::tcgetpgrp`
on the master fd). Verified with bash 3.2 and zsh 5.9 on a forkpty'd pty: at the
prompt `tcgetpgrp` == shell pid; while `sleep 2` runs it returns the `sleep` pid
(`pbsi_comm=sleep`) and `PROC_PIDVNODEPATHINFO` on that pid returns its cwd.

**Path form.** libproc returns the vnode's canonical path: `cd /tmp` reads back as
`/private/tmp`, whereas the shell's `$PWD` and OSC 7
say `/tmp`. Canonicalise (`std::fs::canonicalize`) before comparing cwd to worktree
paths from `git worktree list`, or compare on device+inode.

**Rust access.** No dependency needed: declare `extern "C" { fn proc_pidinfo(...) }`
against libSystem (libproc is part of libSystem on macOS) plus the 2352-byte struct
layout, or use the `libproc` crate which wraps the same flavors. Keep it behind a
`cfg(target_os = "macos")`.

## 2. OSC 7 by shell, and how terminals inject it

**The sequence.** `ESC ] 7 ; file://HOST/PATH BEL` (or `ESC \` as terminator).
Apple's and fish's emitters percent-encode the path and put the hostname in the
authority; iTerm2 documents it as "a file URL with a hostname and a path, like
`file://example.com/usr/bin`". Ghostty and kitty emit a variant scheme,
`kitty-shell-cwd://HOST/PATH`, with the raw (unencoded) path. WezTerm's docs use
`file://HOSTNAME/CURRENT/DIR`. A parser must accept both schemes, percent-decode
`file://`, and compare HOST against the local hostname to decide local vs remote.

**Defaults on macOS (nothing installed by the user):**

- zsh 5.9: does not emit OSC 7 itself. Apple's `/etc/zshrc` ends with
  `[ -r "/etc/zshrc_$TERM_PROGRAM" ] && . "/etc/zshrc_$TERM_PROGRAM"`, and
  `/etc/zshrc_Apple_Terminal` defines `update_terminal_cwd()` that prints
  `'\e]7;%s\a' "file://$HOST$url_path"` and registers it with
  `add-zsh-hook precmd update_terminal_cwd`. So a zsh started with
  `TERM_PROGRAM=Apple_Terminal` emits OSC 7 at every prompt (observed in the probe
  above: `]7;file://MacBookPro.lan/tmp` after each command). That file also installs
  Apple's `.zsh_sessions` history save/restore, so borrowing `TERM_PROGRAM` is not
  free of side effects; treat it as a curiosity, not the plan.
- bash: `/etc/bashrc_Apple_Terminal` does the same via `PROMPT_COMMAND`, gated the
  same way. Stock bash never emits OSC 7 on its own.
- fish: since 4.0.0 (2025-02-27) "fish now reports the working directory (via OSC 7)
  unconditionally instead of only for some terminals" and "marks the prompt and
  command-output regions (via OSC 133)"; 4.3.0 re-emits OSC 7 on every fresh prompt
  so a child like `ssh` cannot leave a stale value. The emitter is
  `__fish_config_interactive.fish`: `printf \e\]7\;file://%s%s\a (string escape
  --style=url -- $host $PWD)`, disabled only for `TERM=dumb` and some Konsole /
  MSYS cases. fish is not installed on this machine, so this is from the changelog
  and source, not observed.

**Hooks the shells offer** (for our own injected hook): zsh `chpwd` ("executed
whenever the current working directory is changed") and `precmd` ("executed before
each prompt"), plus the `chpwd_functions` / `precmd_functions` / `preexec_functions`
arrays; bash `PROMPT_COMMAND` (and `PS0` from 4.4, or bash-preexec for 3.2); fish
`--on-variable PWD` and the `fish_prompt` event.

**Injection patterns (all without touching the user's dotfiles):**

- Ghostty (`src/termio/shell_integration.zig`, `src/shell-integration/README.md`).
  zsh: save the user's `ZDOTDIR` in `GHOSTTY_ZSH_ZDOTDIR`, set `ZDOTDIR` to Ghostty's
  resources `zsh/` dir whose `.zshenv` restores the real `ZDOTDIR`, sources the
  user's `$ZDOTDIR/.zshenv`, then `autoload -Uz` the `ghostty-integration` function
  for interactive shells (zsh 5.1+; a system-wide `/etc/zshenv` that sets `ZDOTDIR`
  defeats it). bash: rewrite argv to `bash --posix`, set `ENV` to
  `bash/ghostty.bash`, stash `--norc`/`--noprofile`/`--rcfile` in
  `GHOSTTY_BASH_INJECT` / `GHOSTTY_BASH_RCFILE`, bail on `-c`; the script then
  `set +o posix`, replays the normal startup files itself, and needs bash 4.4+ for
  `PROMPT_COMMAND`/`PS0` hooks (older: bundled bash-preexec). It refuses macOS
  `/bin/bash` outright: "Apple distributes their own patched version of Bash 3.2 on
  macOS that disables the ENV-based POSIX startup path". fish/elvish/nushell:
  prepend the resources dir to `XDG_DATA_DIRS` so fish's vendor loader picks up
  `fish/vendor_conf.d/*.fish`. The zsh hook then emits
  `'\e]7;kitty-shell-cwd://'"$HOST""$PWD"'\a'` from `chpwd_functions` and from
  precmd, and OSC 133 `A;cl=line` / `B` / `C` / `D;<status>`. Config:
  `shell-integration = detect|none|bash|...` and `shell-integration-features =
  cursor,sudo,title,ssh-env,ssh-terminfo`.
- iTerm2 3.5+ ("Load shell integration automatically" in Profiles > General,
  `sources/ShellIntegration/ShellIntegrationInjection.swift`). Same three tricks:
  zsh saves `ZDOTDIR` to `IT2_ORIG_ZDOTDIR`, points `ZDOTDIR` at its resources dir
  and sets `ITERM_INJECT_SHELL_INTEGRATION=1`; its `.zshenv` restores `ZDOTDIR`,
  sources the user's `.zshenv`, then autoloads `iterm2_shell_integration.zsh`.
  bash inserts `--posix` at argv[1], sets `ENV=<dir>/bash-si-loader`, preserves a
  pre-existing `ENV` in `IT2_BASH_POSIX_ENV`, handles `--rcfile`/`--init-file`,
  and the code comment calls `/bin/bash` "the un-injectable 3.2.57". fish sets
  `XDG_DATA_DIRS=<dir>` (and `IT2_FISH_XDG_DATA_DIRS`) so
  `fish/vendor_conf.d/iterm2-shell-integration-loader.fish` loads. iTerm2's script
  reports cwd with its own `OSC 1337;CurrentDir=` and `OSC 1337;RemoteHost=user@host`
  in addition to OSC 133 A/B/C/D.
- kitty (origin of the pattern the two above copied): "kitty starts bash in POSIX
  mode, using the environment variable ENV to load the shell integration script";
  zsh via `ZDOTDIR` (5.1+), fish via `XDG_DATA_DIRS` (3.3+); `shell_integration
  no-rc` opts out of the env changes while keeping the feature flags.
- WezTerm: no automatic injection on macOS. Its `assets/shell-integration/wezterm.sh`
  is sourced by hand or installed to `/etc/profile.d` by the Fedora/Debian/Arch
  packages; it emits OSC 7 `file://HOSTNAME/DIR`, OSC 133 and OSC 1337 SetUserVar
  from precmd/preexec.
- VS Code: also injects by "modifying shell arguments and environment variables at
  launch" for bash, fish, pwsh, zsh, and documents the limitation shared by all of
  them: injection "will not work for some advanced use cases like in sub-shells,
  through a regular ssh session".

**What this app should inject.** Follow the kitty/Ghostty/iTerm2 trio exactly
(they have shaken out the edge cases): zsh via `ZDOTDIR` shim, fish via
`XDG_DATA_DIRS` vendor_conf.d, bash via `--posix` + `ENV` only when the binary is
not `/bin/bash` (Homebrew bash 5 works; Apple's 3.2 gets no integration and relies
on the libproc path). Bundle the three files in the Tauri resources dir. Emit OSC 7
(`file://` with percent-encoding, matching Apple and fish) from `chpwd`/precmd and
OSC 133 A/B/C/D. Because the libproc path covers cwd regardless, an injection
failure degrades to "no OSC 133 marks", not "no Badge".

**Receiving in xterm.js.** `terminal.parser.registerOscHandler(ident: number,
callback: (data: string) => boolean): IDisposable`; return `true` to mark the
sequence handled. Register 7, 133 and 1337 in the Svelte side and forward to Rust,
or parse them in Rust before the bytes reach the webview so the sidebar state does
not depend on the renderer. Either works; doing it in Rust keeps one source of truth
with the libproc poller.

## 3. Repo, Worktree and branch from a cwd

**Layout (git docs, verified on `jack/doorstep-gifts/doorstep`, git 2.51.1).**
A linked worktree's root holds a plain-text `.git` *file* ("gitfile") containing
`gitdir: /Users/.../doorstep/.git/worktrees/review-redesign`. That private dir holds
per-worktree state: `HEAD` (`ref: refs/heads/review/resizable-views`), `index`,
`ORIG_HEAD`, `logs/`, `refs/` (only `refs/bisect`, `refs/rewritten`,
`refs/worktree`), `config.worktree`, plus two pointers: `gitdir` = absolute path of
the worktree's `.git` file (`.../.claude/worktrees/review-redesign/.git`) and
`commondir` = relative path to the shared repo (`../..`). Everything else
(`objects/`, `refs/heads/*`, `packed-refs`, `config`, `hooks/`, `worktrees/`) lives
in the common dir. The main worktree is the parent of the common dir (for
non-bare, no-`core.worktree` repos). `git rev-parse --git-dir` in the linked tree
prints the private dir; `--git-common-dir` prints the shared `.git`; they are equal
in the main worktree, which is the linked-vs-main test.

**One git call answers everything but siblings** (measured 1.4 ms per invocation
on an idle machine, dominated by process spawn; `/usr/bin/true` costs about the
same):

```
git -C <cwd> rev-parse --path-format=absolute --show-toplevel --absolute-git-dir --git-common-dir --abbrev-ref HEAD
```

prints toplevel, git dir, common dir, and the short branch, or the literal `HEAD`
when detached (`git symbolic-ref --short -q HEAD` exits 1 silently on detached;
`git branch --show-current` prints nothing). Siblings:
`git worktree list --porcelain [-z]` prints records separated by blank lines:
`worktree <path>`, `HEAD <sha>`, then `branch refs/heads/x` or `detached`, and
optional `bare`, `locked [reason]`, `prunable <reason>` (e.g. "gitdir file points
to non-existent location"). The doorstep repo shows all forms, including a
detached worktree under `/private/tmp/ci-local/pr-711` and eight linked trees.

**Reading the files directly (recommended for the hot path).** Algorithm:

1. From the canonicalised cwd, walk up until a `.git` entry exists (or a
   `ceiling`/home boundary). Directory → main worktree, `git_dir = .git`,
   `common = git_dir`. File → parse `gitdir: <path>` (may be relative to the file),
   `git_dir` = that; `common = git_dir/commondir` contents joined and normalised.
2. Branch: read `git_dir/HEAD`; `ref: refs/heads/<name>` → branch `<name>`; a 40/64
   hex string → detached at that sha. (No need to resolve the sha for the Badge.)
3. Repo identity: `common` path (or the basename of its parent). Two Sessions share
   a repo iff their `common` paths are equal after canonicalisation.
4. Siblings: for each `common/worktrees/<id>/`, read `gitdir` (strip trailing
   `/.git` for the worktree path) and `HEAD`; skip entries whose `gitdir` target no
   longer exists (that is what git calls prunable), plus the main worktree at
   `parent(common)`.

Cost is a handful of `stat`/`read` calls, microseconds, no fork. Corner cases to
keep in mind: `GIT_DIR`/`GIT_WORK_TREE` env vars in the shell (ignore; the Badge
follows the filesystem), submodules (their `.git` file points into
`<super>/.git/modules/<name>`, which has no `commondir`; treat the submodule as its
own repo), bare repos (no toplevel), and `git worktree move` done by hand (`gitdir`
stale until `git worktree repair`).

**Crates.** `git2` 0.21 (libgit2 bindings, builds libgit2 from source via
`libgit2-sys`; no cmake needed): `Repository::discover(path)`, `.workdir()`,
`.path()`, `.commondir()`, `.is_worktree()`, `.head()?.shorthand()`,
`.head_detached()`, `.worktrees()` + `.find_worktree(name).path()`. `gix` 0.87.1
(pure Rust, "60+ gix-* sub-crates"): `gix::discover(path)`, `.workdir()`,
`.git_dir()`, `.common_dir()` ("Returns the main git repository if this is a
repository on a linked work-tree, or the git_dir itself"), `.head_name()`,
`.worktrees()` ("all linked worktrees in addition to the main worktree"),
`Worktree::is_main()/id()/base()`. Note for `gix`: `default-features = false`
alone does not compile (gix-hash needs the `sha1` or `sha256` feature; with
neither, its enums are empty and 0.26.2 fails with E0004) — keep defaults or pick
the `sha1`-bearing component features. Measured discover+HEAD timings for both are
in the table below. Either crate is overkill for the Badge alone, but `git2` is the
pragmatic choice if the app later needs status/ahead-behind, and it reads the same
`.git`-file layout git does. Shelling out remains right for one-off actions
(creating a worktree) and for `git worktree list` if the direct reader ever
disagrees with git.

Measured here (release build, 200 iterations of discover + branch name, then one
worktree listing; `git2` 0.21 default features, `gix` 0.87.1 default features):

| Repo | git2 discover+head | git2 worktrees() | gix discover+head_name | gix worktrees() |
|---|---|---|---|---|
| sidebar-term main worktree (tiny, 4 linked) | 267 us | 356 us | 182 us | 260 us |
| sidebar-term linked worktree (`wt/cwd`) | 865 us | 1920 us | 299 us | 302 us |
| jack/doorstep linked worktree (4.7k loose objects, 8 linked) | 637 us | 5107 us | 571 us | 694 us |

Both crates re-read config and discover on every call, so hundreds of
microseconds per query; cache the `Repository` per Session (or per `git_dir`) and
re-query only on a trigger. Both returned the correct linked-worktree answers
(`is_worktree=true` / `kind=LinkedWorkTree`, `commondir` = shared `.git`,
`head` = `research/cwd-git`). Note `gix::Repository::common_dir()` returns the
raw `.../worktrees/cwd/../..` and needs normalising before comparison; `git2`
returns it canonical with a trailing slash. Neither lists the main worktree in
`worktrees()`; derive it from the common dir. The hand-rolled file reader is still
an order of magnitude cheaper (a few `stat`/`read` syscalls, well under 50 us)
and has no build-time cost; `git2` builds libgit2 from source (about 20 s clean
on this machine, inside the ~2 min total for the bench crate with `gix`).

## 4. Change detection

Three events change the Badge: cwd change (`cd`, or a Foreground process with a
different cwd), branch change (`checkout`/`switch`/`commit` on detached HEAD,
`rebase`), and sibling worktree add/remove.

- **cwd** does not touch the filesystem, so only the shell can announce it (OSC 7
  or OSC 133) or the app can poll. Recommended: poll `proc_pidinfo` on the shell
  pid (and `tcgetpgrp` for the Foreground process) every 1–2 s, plus immediate
  refresh on OSC 7 / OSC 133 `A` or `D` when present. Cost ~1 us per Session per
  tick; the diff is a string compare, so no churn when nothing changed.
- **branch**: git writes `HEAD` (and every ref) by creating `HEAD.lock`, writing,
  and `rename(2)` over the target ("When we want to change a file, we create a
  lockfile `<filename>.lock`, write the new file contents into it, and then rename
  the lockfile to its final destination"). A watch on the *file* `HEAD` therefore
  loses track after the first rename; watch the directory `git_dir` (per-worktree
  private dir for linked trees) non-recursively and filter for `HEAD`. `notify`
  8.2.0 on macOS defaults to the FSEvents backend (`macos_fsevent` feature;
  `macos_kqueue` is the alternative), creates the stream with
  `kFSEventStreamCreateFlagFileEvents` so events name the file, and uses
  `Config::fsevent_latency` for coalescing (Apple's FSEvents batches events for
  `latency` seconds; FSEvents are directory-scoped by design, "something in a given
  directory changed"). `Config::with_poll_interval` only affects `PollWatcher`.
  Known limits from the notify docs: some events under FSEvents' security model are
  not observable for unowned files (not our case), and very large watch sets can
  drop events. One stream per distinct `git_dir` (Sessions in the same worktree
  share it) is a few dozen streams at most. Add a watch on `common/worktrees/` for
  sibling add/remove and on `common/HEAD` when the Session is in the main tree.
- **OSC 133** (`A` prompt start, `B` command start, `C` output start, `D;<exit>`
  command finished, per iTerm2/FinalTerm and Ghostty's emitter) is the cheapest and
  most precise trigger: re-read cwd and `HEAD` on every `D` (or `A`), because
  anything the user typed has completed. It costs nothing when idle and cannot miss
  a `cd`. It exists only when injection succeeded (zsh/fish/Homebrew bash), hence
  the poll as the floor. fish 4 emits 133 A/B/C/D by default.
- **Pure polling of git** (`git rev-parse` per Session per second) is the one
  option to avoid: 1.4 ms × N Sessions × 1 Hz is still small, but fork-per-tick
  shows up in Activity Monitor and wakes the CPU for nothing; the file-read variant
  of the same poll is fine.

Debounce Badge updates to ~50–100 ms so a `rebase` that rewrites `HEAD` many times
renders once.

## 5. When cwd is unknowable (ssh, docker, remote shells)

`proc_pidinfo` always answers, but for a Session whose Foreground process is `ssh`,
`mosh`, `docker exec`, `kubectl exec`, `nix develop --impure` on a remote builder,
etc. the answer is the *local* cwd of that client binary, which is not where the
user is. Detection signals, cheapest first:

1. Foreground process name (`pbsi_comm` from `PROC_PIDT_SHORTBSDINFO`, or
   `proc_pidpath`) is in a small deny-list: `ssh`, `mosh-client`, `docker`,
   `kubectl`, `podman`, `lima`, `et`. Then the Badge stops trusting libproc.
2. OSC 7 whose host part differs from `gethostname()`: the remote shell (fish 4, or a
   remote zsh with the user's own hook) is telling us the remote cwd and host.
   iTerm2's `OSC 1337;RemoteHost=user@host` and `CurrentDir=` carry the same. This
   is how iTerm2 tracks "current working directory, host name, and more, even over
   ssh"; Ghostty and VS Code both document that injected integration is lost inside
   an ssh session unless the remote side sources it (kitty solves it by copying the
   integration over with its ssh kitten; Ghostty's `ssh-env`/`ssh-terminfo` only
   forward env and terminfo).
3. `TERM_PROGRAM`-style hints are unavailable remotely; do not rely on them.

Badge behaviour: show a "remote" glyph plus the host from OSC 7/1337 when one has
been seen, otherwise the process name (`ssh`), and clear the repo/worktree/branch
fields (or keep the last local values dimmed with the remote marker, which is
useful when the user pops back out of ssh). When the Foreground process exits and
`tcgetpgrp` returns the shell pid again, resume the libproc path. Inside a local
container run via `docker run -it` the same rule applies (the Foreground process is
the docker client). This also covers `sudo -i` and `su` (cwd read returns EPERM):
show the shell's last known cwd, no branch.

## Sources

- Apple libproc header: `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/include/libproc.h` (`proc_pidinfo`, `proc_pidpath`, `proc_listchildpids`, "private interfaces ... subject to change").
- Apple proc_info header: `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/include/sys/proc_info.h` (`PROC_PIDVNODEPATHINFO`=9, `struct proc_vnodepathinfo`, `struct vnode_info_path`, `PROC_PIDT_SHORTBSDINFO`=13, `PROC_PIDTBSDINFO` `e_tdev`/`e_tpgid`).
- xnu `proc_security_policy` uid check and `proc_pidvnodepathinfo` (`vn_getpath`): https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/proc_info.c
- Local probes (forkpty + libproc + tcgetpgrp, timings, EPERM on pid 1, `/private/tmp` canonical paths): scratch programs written for this ticket, results recorded above; not committed.
- Apple FSEvents Programming Guide (directory granularity, latency, coalescing): https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/UsingtheFSEventsFramework/UsingtheFSEventsFramework.html
- Apple Terminal zsh/bash OSC 7 hooks: `/etc/zshrc` (last line), `/etc/zshrc_Apple_Terminal` (`update_terminal_cwd`, `add-zsh-hook precmd`), `/etc/bashrc_Apple_Terminal` (`PROMPT_COMMAND`).
- zsh hook functions (`chpwd`, `precmd`, `preexec`, `*_functions`): https://zsh.sourceforge.io/Doc/Release/Functions.html
- zsh startup files and `ZDOTDIR`: https://zsh.sourceforge.io/Doc/Release/Files.html
- bash startup files, POSIX mode and `ENV`: https://www.gnu.org/software/bash/manual/html_node/Bash-Startup-Files.html
- fish configuration files, `vendor_conf.d` and `XDG_DATA_DIRS`: https://fishshell.com/docs/current/language.html#configuration-files
- fish changelog (4.0.0 unconditional OSC 7 and OSC 133; 4.3.0 OSC 7 on every prompt): https://github.com/fish-shell/fish-shell/blob/master/CHANGELOG.rst and https://fishshell.com/docs/current/relnotes.html
- fish OSC 7 emitter: https://github.com/fish-shell/fish-shell/blob/master/share/functions/__fish_config_interactive.fish
- Ghostty shell-integration README (injection per shell): https://github.com/ghostty-org/ghostty/blob/main/src/shell-integration/README.md
- Ghostty injection logic (`--posix`/`ENV`, `ZDOTDIR`, `XDG_DATA_DIRS`, macOS `/bin/bash` refusal): https://github.com/ghostty-org/ghostty/blob/main/src/termio/shell_integration.zig
- Ghostty zsh shim and hook (OSC 7 `kitty-shell-cwd://`, OSC 133): https://github.com/ghostty-org/ghostty/blob/main/src/shell-integration/zsh/.zshenv and https://github.com/ghostty-org/ghostty/blob/main/src/shell-integration/zsh/ghostty-integration
- Ghostty bash hook: https://github.com/ghostty-org/ghostty/blob/main/src/shell-integration/bash/ghostty.bash
- Ghostty config reference (`shell-integration`, `shell-integration-features`) and feature docs: https://ghostty.org/docs/config/reference and https://ghostty.org/docs/features/shell-integration
- kitty "How automatic shell integration works": https://sw.kovidgoyal.net/kitty/shell-integration/
- iTerm2 shell integration docs (auto-load in 3.5+, supported shells, ssh): https://iterm2.com/documentation-shell-integration.html
- iTerm2 injection source: https://github.com/gnachman/iTerm2/blob/master/sources/ShellIntegration/ShellIntegrationInjection.swift and https://github.com/gnachman/iTerm2/blob/master/OtherResources/.zshenv
- iTerm2 escape codes (OSC 7, OSC 1337 CurrentDir/RemoteHost, OSC 133 A/B/C/D): https://iterm2.com/documentation-escape-codes.html
- WezTerm shell integration (no macOS auto-inject; OSC 7/133/1337): https://wezterm.org/shell-integration.html and https://wezterm.org/escape-sequences.html
- VS Code shell integration (argv/env injection, ssh limitation): https://code.visualstudio.com/docs/terminal/shell-integration
- xterm.js `IParser.registerOscHandler`: https://xtermjs.org/docs/api/terminal/interfaces/iparser/
- git repository layout (gitfile, `worktrees/<id>/gitdir|commondir|locked`, shared vs per-worktree files): https://git-scm.com/docs/gitrepository-layout
- git rev-parse (`--git-dir`, `--git-common-dir`, `--show-toplevel`, `--abbrev-ref`, `--path-format`): https://git-scm.com/docs/git-rev-parse
- git worktree (`list --porcelain` format, linked-worktree details, `repair`): https://git-scm.com/docs/git-worktree
- git symbolic-ref (`--short`, `-q` exit 1 on detached): https://git-scm.com/docs/git-symbolic-ref
- git branch `--show-current`: https://git-scm.com/docs/git-branch
- git lockfile rename semantics: https://github.com/git/git/blob/master/lockfile.h
- Local verification of layout and porcelain output: `/Users/bencooper/Dev/jack/doorstep-gifts/doorstep` (read-only) and this repo's own `.git/worktrees/cwd`.
- git2 crate: https://docs.rs/git2/latest/git2/struct.Repository.html
- gix crate: https://docs.rs/gix/latest/gix/ and https://docs.rs/gix/latest/gix/struct.Repository.html
- notify crate (backends, known problems, Config): https://docs.rs/notify/latest/notify/ and https://docs.rs/notify/latest/notify/struct.Config.html ; FSEvents backend source (FileEvents flag, `fsevent_latency`): https://github.com/notify-rs/notify/blob/main/notify/src/fsevent.rs
- portable-pty (`process_id`, `process_group_leader`): https://docs.rs/portable-pty/latest/portable_pty/trait.MasterPty.html ; implementation is `libc::tcgetpgrp` on the master fd: https://github.com/wezterm/wezterm/blob/main/pty/src/unix.rs
