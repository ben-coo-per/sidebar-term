# Research: pty spawning and streaming in Tauri 2

Ticket: #2. Question: how the Tauri 2 (Rust) shell should spawn, resize, read from and write to a
pty per Session, and stream that to xterm.js with acceptable latency and throughput.
Scope: macOS only, Tauri 2 + Svelte + xterm.js, nothing custom in the terminal runtime.
Researched 2026-09-10 against crate docs/source, Tauri source and docs, xterm.js docs/source and
the XNU kernel tty sources. A scratch Rust program was built and run on the dev machine (Rust 1.94)
to confirm the API compiles and to measure raw pty behaviour; numbers below marked "measured" come
from that run.

## Recommendation

- **pty crate: `portable-pty = "=0.9.0"`** (wezterm's pty layer), used directly from our own Tauri
  commands. Not `tauri-plugin-pty` (its JS side polls a `read` command in a loop and it is a
  one-person crate with no repository link on crates.io), not `pty-process` (fine crate, but its
  tokio integration buys nothing here and it lacks `process_group_leader`/`tty_name`), and not
  `tauri-plugin-shell` (pipes, not a pty; cannot host an interactive shell).
- **IPC path: Tauri `Channel<InvokeResponseBody>` sending `InvokeResponseBody::Raw(Vec<u8>)`**, one
  channel per Session, created in the webview and passed to a `spawn_session` command. Input, resize
  and close are plain `invoke` commands. Do not use events (JSON-only, documented as not for high
  throughput, unordered with async listeners). Do not add a local WebSocket (a second listener, CSP
  changes, no Tauri integration, and the Channel path already ships bytes as `ArrayBuffer`).
- **Read loop: one dedicated `std::thread` per Session** doing blocking `read()` on the master, that
  **coalesces** chunks before `channel.send` (macOS hands the master at most ~1 KiB per `read()`, so
  unbatched sends would be one IPC message per KiB).
- **Flow control: xterm.js write-callback watermarks** (per the xterm.js flow-control guide) driving
  a `pause`/`resume` command that parks the reader thread; kernel back-pressure then stalls the child.
- **Teardown: `ChildKiller::kill()` (SIGHUP) → reader thread sees EOF → `Child::wait()` → drop master.**
  Dropping the master alone is not enough while the reader's dup'd fd is open.
- **pid / process group:** `Child::process_id()` gives the shell pid (= session id and initial pgrp,
  since the child does `setsid()`); `MasterPty::process_group_leader()` (`tcgetpgrp` on the master)
  gives the *foreground* process group at any moment. That is what agent detection should poll.

Pinned versions (all current on crates.io / npm as of 2026-09-10):

| Dep | Version | Why |
|---|---|---|
| `portable-pty` | `=0.9.0` (2025-02-11) | pty layer |
| `libc` | `=0.2.189` | `killpg`, `SIGHUP`; latest stable (`1.0.0-alpha.4` exists, do not use) |
| `nix` | `=0.31.3`, features `["process","term"]` | optional: `tcgetpgrp` without unsafe |
| `tauri` | `=2.11.5` | Channel API; `tauri-build =2.6.3` |
| `@tauri-apps/api` | `2.11.1` | `Channel` class |
| `@xterm/xterm` | `6.0.0` (2025-12-22) | terminal |
| `@xterm/addon-fit` / `@xterm/addon-webgl` | `0.11.0` / `0.19.0` | resize to container / renderer |

## Crate comparison

**portable-pty 0.9.0** (wezterm/wezterm, `pty/`). Trait-based: `native_pty_system().openpty(PtySize)`
→ `PtyPair { slave, master }`. `SlavePty::spawn_command(CommandBuilder) -> Box<dyn Child>`.
`MasterPty` has `resize`, `get_size`, `try_clone_reader` (dup of the master fd, `Read + Send`),
`take_writer` (once; `Write + Send`), and on unix `process_group_leader`, `as_raw_fd`, `tty_name`,
`get_termios`. `Child` has `process_id`, `try_wait`, `wait`, `kill`, `clone_killer` (a `ChildKiller`
that can be moved to another thread). Unix impl: `libc::openpty` with the initial `winsize`, CLOEXEC on
both fds; the child's `pre_exec` resets SIGCHLD/HUP/INT/QUIT/TERM/ALRM to `SIG_DFL`, clears the signal
mask, calls `setsid()`, then `ioctl(0, TIOCSCTTY)` (comment: without it "delivery of SIGWINCH won't
happen when we resize"), closes fds > 2, applies umask. `CommandBuilder::new_default_prog()` resolves
`$SHELL` (falls back to the passwd entry). Deps: libc, nix (`term`,`fs`), filedescriptor, anyhow.
`PtyPair` lists `slave` first "so that it is dropped first". Reader maps `EIO` to `Ok(0)` ("EIO
indicates that the slave pty has been closed"). Blocking I/O only; async is your own thread.

**pty-process 0.5.3** (doy, git.tozt.net). `open() -> (Pty, Pts)`; `Pty` is `AsyncRead + AsyncWrite`
via `tokio::io::unix::AsyncFd` (feature `async`), `into_split()` into owned halves; `Command` wraps
`tokio::process::Command`, `spawn(pts) -> tokio::process::Child`, child does `setsid` +
`ioctl_tiocsctty` in `pre_exec`. `resize` uses `rustix::termios::tcsetwinsize`. Opens the pty via
`posix_openpt`/`grantpt`/`unlockpt` (rustix). Good, small, but: no `tcgetpgrp` helper (you'd add nix or
libc anyway), no `tty_name`, no `$SHELL` resolution, `spawn_borrowed` unavailable on macOS, and the
async surface only matters if the reader lives on the tokio runtime — which we do not want (see
"Read loop"). Pid via `tokio::process::Child::id()`.

**tauri-plugin-pty 0.3.1** (Tnze/tauri-plugin-pty, JS package `tauri-pty`). Thin wrapper over
`portable-pty ^0.9.0` with `serde_support`. Commands: `spawn`, `write` (takes a `String`), `read`,
`resize`, `kill`, `exitstatus`, `get_all_pids`. Output delivery is **JS-side polling**: `readData()`
loops `await invoke("plugin:pty|read")`, each call doing one 4 KiB `read()` under a tokio `Mutex` and
returning `tauri::ipc::Response` — so one round trip per ≤4 KiB, and on macOS one per ≤1 KiB (see
measurements). `term_name`, `encoding`, `handle_flow_control` params are accepted and ignored
(`// TODO`). README: "Developing! Wellcome to contribute!". 22 stars, no repository URL in crate
metadata, last push 2026-07-08. Nothing here we cannot write in ~150 lines with a proper Channel.

**tauri-plugin-shell 2.3.6**. `Command` spawns via `std::process::Command` with
`Stdio::piped()` for all three streams (`process/mod.rs`: `pipe()` three times); no `openpty`/`forkpty`
anywhere; output events are line-split by default. A shell under pipes has no controlling terminal, no
job control, no SIGWINCH, no `tcgetpgrp`. Not applicable.

## IPC path: Channel vs events vs WebSocket

Tauri docs (Calling the Frontend): events are "not designed for low latency or high throughput
situations", payloads are JSON, and "if a listener is async and the event emitter sends multiple events
in rapid succession, the listeners may process events out of order". `Emitter::emit` requires
`S: Serialize + Clone`, i.e. JSON only. Channels "are designed to be fast and deliver ordered data" and
are the documented tool for "subprocess output".

How `Channel` actually moves bytes (`crates/tauri/src/ipc/channel.rs`, 2.11.x):
- `Channel<TSend = InvokeResponseBody>`; `send(data)` where `TSend: IpcResponse`. The blanket impl
  `impl<T: Serialize> IpcResponse for T` serialises to JSON — so **`Channel<Vec<u8>>` or the docs'
  `Channel<&[u8]>` example would ship a JSON array of numbers**. To ship raw bytes use
  `Channel<InvokeResponseBody>` and `send(InvokeResponseBody::Raw(buf))` (`From<Vec<u8>>` maps to
  `Raw`), or `Channel<tauri::ipc::Response>` with `Response::new(buf)`.
- Transport is chosen per message by size: JSON < 8192 B and Raw < 1024 B are pushed with
  `webview.eval(...)` (`MAX_RAW_DIRECT_EXECUTE_THRESHOLD = 1024`, comment: "1024 byte payload runs
  roughly 30% faster through eval than through fetch on macOS"; raw bytes are inlined as
  `new Uint8Array([..]).buffer`). Larger payloads are parked in `ChannelDataIpcQueue` and the webview is
  told to `invoke('plugin:__TAURI_CHANNEL__|fetch')`, which on macOS is a `fetch()` POST to the
  `ipc://localhost` custom scheme (`core.js convertFileSrc`: non-Windows/Android →
  `${protocol}://localhost/`); a `Raw` response is served as `application/octet-stream`
  (`ipc/protocol.rs`) and read with `response.arrayBuffer()` (`ipc-protocol.js`).
- Every message carries an `index`; the JS `Channel` class buffers out-of-order arrivals in
  `#pendingMessages` and delivers strictly in order. Dropping the Rust `Channel` sends `{end: true}`.
- Either path delivers an `ArrayBuffer` to `onmessage`; `term.write(new Uint8Array(buf))` decodes it
  as UTF-8 ("Raw bytes will always be treated as UTF-8 encoded").
- `Channel::send` does not queue or apply back-pressure: it evals immediately and returns `Err` only if
  the webview is gone. Back-pressure must be ours (below).

Neither the Tauri docs nor source publish a MB/s figure for channels; the only primary numbers are the
threshold comments above. The measured pty side is far below what a `fetch()` of a 64 KiB body per
message costs, so the IPC will not be the bottleneck once chunks are coalesced. (Not measured
end-to-end in a Tauri app; see "Unverified".)

WebSocket: nothing in Tauri supports it; it needs a listener, a port, an auth token, a
`connect-src ws://localhost:*` CSP entry, and its own framing — for no gain over Raw channels.

## Spawn / read / write / resize loop

Session state, kept in `tauri::State<Sessions>` (a `Mutex<HashMap<SessionId, SessionHandle>>`):

```rust
struct SessionHandle {
    master: Box<dyn MasterPty + Send>,          // resize, process_group_leader, tty_name
    writer: Box<dyn Write + Send>,              // take_writer(), once
    killer: Box<dyn ChildKiller + Send + Sync>, // clone_killer(), SIGHUP
    child_pid: u32,                             // Child::process_id()
    paused: Arc<(Mutex<bool>, Condvar)>,        // flow control gate for the reader thread
}
```

Spawn (`#[tauri::command] fn spawn_session(cwd, cols, rows, on_output: Channel<InvokeResponseBody>,
state, app) -> Result<SessionId, String>`):
1. `native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })`.
2. `let mut cmd = CommandBuilder::new_default_prog(); cmd.cwd(cwd); cmd.env("TERM","xterm-256color");
   cmd.env("COLORTERM","truecolor");` (set `LANG` if unset; login shell = `cmd.arg("-l")`).
3. `let child = pair.slave.spawn_command(cmd)?; drop(pair.slave);` — drop the slave in the parent so
   the master sees EOF when the child's last slave fd closes.
4. `let reader = pair.master.try_clone_reader()?; let writer = pair.master.take_writer()?;`
5. `std::thread::spawn` the read loop (below) moving `reader`, `on_output`, `paused`, and a
   `Child` handle for `wait()`.
6. Store `SessionHandle`, return the id.

Measured on macOS 26 (XNU): a single `read()` on the master returns **at most 1023 bytes** in cooked or
raw mode regardless of the buffer size passed (8 KiB vs 64 KiB made no difference; 19,532 reads for
20,000,000 bytes). This is the tty output queue (`TTYHOG 1024` in `bsd/sys/tty.h`). Throughput of a
tight blocking read loop: `head -c 20000000 /dev/zero` → ~100 MB/s cooked, ~250 MB/s with `stty raw`;
`yes | head -c 20000000` → 3.9 MB/s because macOS `yes` writes per line and the master wakes per write
(avg 6 bytes/read, 4.7 M reads). Consequences: (a) never forward one `read()` per IPC message;
(b) `yes` is a syscall-rate test, not a bandwidth test; (c) `cat bigfile` is the realistic burst case
and runs at ~100 MB/s into the reader.

Read loop (blocking thread, not tokio; `Read` on portable-pty is blocking and `spawn_blocking` would
pin a runtime worker for the life of the Session):

```rust
let mut buf = vec![0u8; 64 * 1024];
let mut acc: Vec<u8> = Vec::with_capacity(64 * 1024);
loop {
    // flow control gate: block here while the webview said "pause"
    wait_while_paused(&paused);
    match reader.read(&mut buf) {
        Ok(0) | Err(_) => break,                 // EIO is mapped to Ok(0) by portable-pty
        Ok(n) => {
            acc.extend_from_slice(&buf[..n]);
            // coalesce: keep draining while more is immediately available, up to a cap or ~4 ms
            while acc.len() < 64 * 1024 && poll_readable(master_fd, 0 /*ms*/) { /* read again */ }
            on_output.send(InvokeResponseBody::Raw(std::mem::take(&mut acc))).ok();
        }
    }
}
// child gone or webview gone: reap and notify
let status = child.wait();  on_exit.send(...)
```

`poll_readable` is one `libc::poll` with a zero timeout (or a 1–4 ms timeout to batch bursts — this is
the same "frame-sized batches" idea xterm.js relies on internally with its 12 ms `WRITE_TIMEOUT_MS`).
With ~1 KiB per kernel read, batching to 16–64 KiB turns a 100 MB/s burst into ~2–6 k channel
messages/s, all on the fetch path (> 1024 B), each an `ArrayBuffer` straight into `term.write`.
Interactive typing echoes are a few bytes and go the eval path in a single message (< 1024 B).

Write (`#[tauri::command] fn write_session(id, data: Vec<u8>)` — pass a `Uint8Array` from JS so
`invoke` uses the raw body, or a `String` from `term.onData` and `write_all(s.as_bytes())`): lock the
`SessionHandle`, `writer.write_all(&data)`. `term.onData` yields UTF-16 strings; `term.onBinary` (for
non-UTF-8 mouse/legacy data) yields a "binary" string — encode with `charCodeAt` bytes. A write to the
master blocks when the tty input queue is full (`TTYHOG - 2`, `ptcwrite`), which only happens for
pastes into a non-reading child; do the write on the blocking pool or a short `spawn_blocking`.

Resize (`#[tauri::command] fn resize_session(id, cols, rows)`): `master.resize(PtySize{..})` →
`ioctl(TIOCSWINSZ)`. XNU `ttioctl` on `TIOCSWINSZ`: if the size changed, `tty_pgsignal_locked(tp,
SIGWINCH, 1)` — the kernel sends SIGWINCH to the **foreground process group**; nothing to do in
userland beyond the ioctl, and it only works because the child did `TIOCSCTTY`. On the JS side: the
`FitAddon` computes cols/rows from the container; xterm docs say "It's best practice to debounce calls
to resize"; `term.onResize(({cols, rows}) => invoke('resize_session', …))`.

Flow control (xterm.js guide, `WriteBuffer.ts`): xterm parses asynchronously at "5–35 MB/s" and
throws `write data discarded, use flow control to avoid losing data` past `DISCARD_WATERMARK =
50_000_000` pending bytes. Use the guide's watermark pattern with the write callback:

```ts
const HIGH = 200_000, LOW = 50_000; let pending = 0;
chan.onmessage = (buf: ArrayBuffer) => {
  const bytes = new Uint8Array(buf); pending += bytes.length;
  term.write(bytes, () => { pending -= bytes.length; if (pending < LOW && paused) resume(); });
  if (pending > HIGH && !paused) pause();
};
```

`pause()`/`resume()` are two tiny `invoke`s flipping the `paused` flag + `Condvar` the reader thread
waits on. While paused the reader stops draining the master; the tty output queue fills; the child's
`write()` blocks in the kernel (`ptcwrite`/`ttyoutput` back-pressure) — no data is lost and no
buffering grows on the Rust side. This is exactly what the xterm guide models as `pty.pause()`.

## Teardown when a Tab closes

Kernel facts (XNU `bsd/kern/tty_dev.c`, `tty.c`): closing the **last** master fd runs `ptcclose` →
`l_modem(tp, 0)` → `ttymodem` → `psignal(tp->t_session->s_leader, SIGHUP)` and flushes queues; closing
the last slave fd makes master reads return EOF/`EIO` and master writes fail with `EIO`
(`TS_ISOPEN`/`TS_CONNECTED` checks in `ptcwrite`). "Last" matters: `try_clone_reader` and
`take_writer` are `dup()`s of the master, so the tty is not closed until the reader thread's fd is gone.

Order that works (measured: exit status observed, reader thread ended with `Ok(0)`):
1. `killer.kill()` — portable-pty's unix `ChildKiller` sends **SIGHUP to the child pid** (comment:
   "we send the SIGHUP signal instead of trying to kill the process"). The child is the session leader,
   so the login shell hangs up and, for zsh/bash default settings, forwards HUP to its jobs.
   For an unresponsive foreground job, escalate with `libc::killpg(pgrp, SIGKILL)` where
   `pgrp = master.process_group_leader()` or the shell pid.
2. Child exits → its slave fds close → reader `read()` returns `Ok(0)` → thread does `child.wait()`
   (reaps the zombie), sends the exit through the channel, drops `reader` and the `Channel`
   (JS sees `{end: true}`).
3. Main side removes the `SessionHandle`; dropping `writer` (portable-pty's writer `Drop` writes
   `\n` + VEOF if the child is still alive; harmless after exit) and `master` closes the last fds.
4. JS: `term.dispose()`.

App quit: iterate sessions and do step 1 for each; do not block quit on `wait()` for more than a short
timeout — on process exit all master fds close and the kernel SIGHUPs every session leader anyway.

Do not rely on `Drop` of `PtyPair` alone for teardown: with the reader thread alive nothing is closed,
and portable-pty's `Child::kill` (SIGHUP) is explicitly not guaranteed to terminate a process that
handles HUP.

## Child pid and process group (for agent detection / cwd research)

- **Shell pid:** `child.process_id()` (`Some(std::process::Child::id())`). Because the child ran
  `setsid()`, shell pid == session id == the shell's own pgrp. Store it on the `SessionHandle`.
- **Foreground process group:** `master.process_group_leader()` → `libc::tcgetpgrp(master_fd)`; returns
  `Some(pgid)` (> 0) or `None`. Measured: idle zsh → equals the shell pid; while `sleep 2` runs →
  the `sleep`'s pid (its own job pgrp). This is the pid the agent-detection research should resolve to a
  process name (e.g. `claude`, `codex`, `gemini`) and the cwd research should resolve to a directory.
  Equivalent safe wrapper: `nix::unistd::tcgetpgrp(fd)` (features `process` + `term`).
- **tty name:** `master.tty_name()` → e.g. `/dev/ttys002`; useful to cross-check `ps -t`.
- The master fd is available via `master.as_raw_fd()` for any extra `ioctl`/`poll` we need.
- Polling cadence: `tcgetpgrp` is one cheap ioctl; polling it on a 250–500 ms timer per Session (or on
  each output burst, edge-triggered) is negligible.

## Unverified / open

- End-to-end throughput and latency through a real Tauri webview were not measured (only the pty side
  and Tauri's own threshold comments). Expect the fetch path to dominate at ~2–6 k messages/s under
  `cat`; if that proves too chatty, raise the coalescing cap toward 256 KiB — xterm's own parser is the
  next limit (5–35 MB/s per its guide).
- The 1023-byte-per-read ceiling was observed on this machine (macOS 26.x); it matches `TTYHOG` in the
  XNU headers but was not traced through every code path.
- `pause`/`resume` round-trip latency (two invokes) was not measured; the HIGH/LOW watermarks above
  are the xterm guide's shape, not tuned numbers.
- portable-pty 0.9.0 is from 2025-02; wezterm's tree has moved on but no newer crate release exists.

## Sources

- portable-pty crate: https://docs.rs/portable-pty/0.9.0/portable_pty/ and https://crates.io/crates/portable-pty
- portable-pty traits (`MasterPty`, `SlavePty`, `Child`, `ChildKiller`, `PtyPair` drop order, `Child::kill` SIGHUP): https://github.com/wezterm/wezterm/blob/main/pty/src/lib.rs
- portable-pty unix impl (`openpty`, `TIOCSWINSZ`, `setsid`/`TIOCSCTTY`, `tcgetpgrp`, EIO→EOF, writer Drop): https://github.com/wezterm/wezterm/blob/main/pty/src/unix.rs
- portable-pty `CommandBuilder` (`new_default_prog`, `$SHELL`, `set_controlling_tty`): https://github.com/wezterm/wezterm/blob/main/pty/src/cmdbuilder.rs
- portable-pty Cargo.toml (deps/features): https://github.com/wezterm/wezterm/blob/main/pty/Cargo.toml
- pty-process 0.5.3 docs and source (crate docs, `open`, `session_leader`, `spawn_borrowed` not on macOS): https://docs.rs/pty-process/0.5.3/pty_process/ and https://static.crates.io/crates/pty-process/pty-process-0.5.3.crate
- tauri-plugin-pty 0.3.1: https://crates.io/crates/tauri-plugin-pty , source https://github.com/Tnze/tauri-plugin-pty/blob/main/src/lib.rs , JS poll loop https://github.com/Tnze/tauri-plugin-pty/blob/main/api/index.ts
- tauri-plugin-shell process spawning (pipes): https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/shell/src/process/mod.rs and https://v2.tauri.app/plugin/shell/
- Tauri "Calling the Frontend" (events vs channels, ordering caveat, throughput statement): https://v2.tauri.app/develop/calling-frontend/
- Tauri "Calling Rust" (async commands, `State`, `tauri::ipc::Response`, `Channel` argument example): https://v2.tauri.app/develop/calling-rust/
- `tauri::ipc::Channel` docs: https://docs.rs/tauri/2.11.5/tauri/ipc/struct.Channel.html
- `InvokeResponseBody` (`Json`/`Raw`): https://docs.rs/tauri/2.11.5/tauri/ipc/enum.InvokeResponseBody.html
- Channel implementation (eval vs fetch thresholds, `ChannelDataIpcQueue`, `end` on drop): https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/src/ipc/channel.rs
- `IpcResponse` blanket impl over `Serialize`, `From<Vec<u8>> → Raw`, `Response::new`: https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/src/ipc/mod.rs
- IPC protocol response content types (`application/octet-stream` for Raw): https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/src/ipc/protocol.rs
- IPC transport selection (custom scheme fetch vs postMessage, `arrayBuffer()`): https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/scripts/ipc-protocol.js
- `convertFileSrc` scheme form on macOS (`ipc://localhost`): https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/scripts/core.js
- JS `Channel` class (index-based ordering, `__CHANNEL__:` serialisation): https://github.com/tauri-apps/tauri/blob/dev/packages/api/src/core.ts
- `tauri::Emitter::emit` bound `Serialize + Clone`: https://docs.rs/tauri/2.11.5/tauri/trait.Emitter.html
- `tauri::async_runtime` (`spawn_blocking`): https://docs.rs/tauri/2.11.5/tauri/async_runtime/index.html
- Tauri CSP (`connect-src`, `ipc:` / `http://ipc.localhost`): https://v2.tauri.app/security/csp/
- xterm.js flow control guide (write callback, watermarks, 5–35 MB/s, 50 MB discard): https://xtermjs.org/docs/guides/flowcontrol/
- xterm.js `WriteBuffer` constants (`DISCARD_WATERMARK`, `WRITE_TIMEOUT_MS`, throw on overflow): https://github.com/xtermjs/xterm.js/blob/master/src/common/input/WriteBuffer.ts
- xterm.js `Terminal` API (`write` UTF-8 for `Uint8Array`, `onData`, `onBinary`, `onResize`, debounce `resize`): https://xtermjs.org/docs/api/terminal/classes/terminal/
- xterm.js 6.0.0 release (2025-12-22): https://github.com/xtermjs/xterm.js/releases/tag/6.0.0
- `nix::unistd::tcgetpgrp` (features `process`,`term`): https://docs.rs/nix/0.31.3/nix/unistd/fn.tcgetpgrp.html
- XNU `TIOCSWINSZ` → `SIGWINCH` to foreground pgrp (`ttioctl`): https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty.c
- XNU `ttymodem` → `SIGHUP` to session leader on carrier loss: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty.c
- XNU `ptcclose` (`l_modem(tp, 0)`), `ptcread` (blocks until replica open; error when lost), `ptcwrite` (`EIO` when not open/connected, `TTYHOG - 2` input limit): https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty_dev.c
- XNU `TTYHOG 1024`, `TTMAXHIWAT`: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/tty.h
- Versions: https://crates.io/crates/tauri , https://crates.io/crates/libc (max stable 0.2.189), https://crates.io/crates/nix , https://crates.io/crates/tauri-plugin-shell , https://www.npmjs.com/package/@xterm/xterm , https://www.npmjs.com/package/@tauri-apps/api
