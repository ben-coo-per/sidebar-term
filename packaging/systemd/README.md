# sidebar-termd under systemd

`sidebar-termd.service` runs the Host daemon as a systemd user service on a Linux box with
cgroup v2. Install it as the main README's "Host daemon" section says. This page covers how it
uses memory, the limits on its Sessions, and how to move a running box to this layout.

## The cgroup layout

Each Session's processes (agents and everything they start: `tsc`, `vitest`, local CI) used to
share one cgroup with the daemon, and the memory limits sat on that cgroup. When the Sessions
reached `MemoryHigh`, the kernel throttled the whole group and pushed the daemon's own pages
into swap. The daemon then took seconds to answer, or never did, and every client showed every
Session greyed out although all of them were still running.

Now the unit delegates its cgroup (`Delegate=yes`) and the daemon splits it (`--cgroups`,
`src-tauri/core/src/cgroup.rs`):

```text
sidebar-termd.service/      delegated to the daemon; no processes of its own
├── daemon/                 the daemon alone (DelegateSubgroup=daemon); memory.low 256M
└── sessions/               every Session together; the limits are here
    ├── <run>-<id>/         one Session: its shell and everything it starts
    └── ...
```

- **The limits are on `sessions/`, never above the daemon.** Sessions at their limit slow down
  themselves and each other. The daemon is not in that group, so it is never throttled with
  them: it keeps answering clients in under a millisecond. `cpu` is enabled too, so the daemon
  gets half the CPU under contention, however many processes the Sessions run.
- **One shared budget, one cgroup per Session.** The limits cover all the Sessions together, so
  one big build may use most of it. Each Session gets its own leaf so the daemon can say which
  Session uses the memory. Every 2 seconds it sends clients a `pressure` sample: how much the
  Sessions use against their limits, how much of the time they wait on memory, and the 3
  Sessions using the most. The Mac app colours the Host's chip on its linked Tabs and says why
  on hover and in Settings > Hosts. The phone shows a banner. The daemon logs each change of
  level, naming the biggest Session.
- `<run>` is the daemon's pid. A leaf a previous run left populated (a process that outlived its
  hangup) is never reused, and is removed once it empties.
- `OOMPolicy=continue` stays: when the Sessions hit `memory.max`, the kernel kills the biggest
  process in `sessions/`, and the service keeps running.

Never put `MemoryHigh=` / `MemoryMax=` / `MemorySwapMax=` on the service (in a drop-in, or with
`systemctl set-property`). They apply to the whole tree, the daemon included, and bring the old
problem back. Put the limits on the daemon's command line (`memory.conf.example`):

| flag | on `sessions/` | what happens there |
|---|---|---|
| `--sessions-memory-high 9G` | `memory.high` | the kernel throttles the Sessions; nothing is killed |
| `--sessions-memory-max 10G` | `memory.max` | the kernel OOM-kills the biggest Session process |
| `--sessions-swap-max 2G` | `memory.swap.max` | how much the Sessions may swap |

Size them so `system.slice` (Tailscale, SSH, Docker: about 3.5 GB on bennet) and the daemon
always fit beside `max`.

Without `--close-idle-after`, Tabs whose Sessions have been idle for 24 hours close. Idle means an agent at its prompt, or a shell with nothing running, with no input
or output all that time. The daemon logs each one's Resume entry first (`closing Tab …; resume
with `claude --resume …` in …`). `--close-idle-after 0` turns this off.

Without `--cgroups`, or when the split fails (no `Delegate=yes`, cgroup v1, other processes in
the cgroup), the daemon logs why and runs as before. Its Sessions then share its cgroup.

## Deploying to bennet

**This restarts the daemon, which hangs up every Session on it.** Whatever runs in them is killed:
agents mid-turn, dev servers, builds. Their shells come back in the same Tabs at the same cwd, but
nothing reruns by itself. The daemon records a Resume entry for each Session before it stops, and
step 1 prints them so you can start each one again by hand. Pick a moment when no agent is
mid-task.

1. Note what is running, to resume by hand afterwards:

   ```sh
   python3 -c 'import json; [print(e["cwd"], "\t", e["line"]) for e in json.load(open("'$HOME'/.local/share/sidebar-term/resume.json"))["running"]]'
   ```

2. Build the new daemon and phone page in the checkout, capped so the build itself cannot
   starve the running Sessions:

   ```sh
   cd ~/repos/sidebar-term && git pull
   systemd-run --user --scope -p MemoryMax=3G nice cargo build --release --bin sidebar-termd --manifest-path src-tauri/Cargo.toml
   pnpm install && pnpm build
   ```

3. Keep the old setup for a rollback, then install the new unit and drop-in. The new drop-in
   replaces `memory.conf`, which put the limits on the service:

   ```sh
   mkdir -p ~/sidebar-termd-rollback && cp -a ~/.local/bin/sidebar-termd ~/.config/systemd/user/sidebar-termd.service ~/.config/systemd/user/sidebar-termd.service.d ~/sidebar-termd-rollback/
   cp packaging/systemd/sidebar-termd.service ~/.config/systemd/user/
   cp packaging/systemd/memory.conf.example ~/.config/systemd/user/sidebar-termd.service.d/memory.conf
   ```

4. Remove the limits `systemctl set-property` put on the service at runtime. They live apart from
   the drop-in, and they would cap the daemon again:

   ```sh
   ls /run/user/$(id -u)/systemd/user.control/sidebar-termd.service.d/
   rm -f /run/user/$(id -u)/systemd/user.control/sidebar-termd.service.d/50-Memory*.conf
   systemctl --user daemon-reload
   systemctl --user show sidebar-termd -p MemoryHigh -p MemoryMax -p MemorySwapMax -p Delegate -p DelegateSubgroup -p ExecStart
   ```

   It should show `MemoryHigh=infinity`, `MemoryMax=infinity`, `MemorySwapMax=infinity`,
   `Delegate=yes`, `DelegateSubgroup=daemon`, and `--cgroups --sessions-memory-high 9G …` in
   `ExecStart`. This step changes nothing yet: the running daemon keeps its old cgroup settings
   until the restart.

5. Install, then restart. From here to the end of the restart, every Session is down:

   ```sh
   install -m 755 src-tauri/target/release/sidebar-termd ~/.local/bin/
   rm -rf ~/.local/share/sidebar-term/web && cp -r build ~/.local/share/sidebar-term/web
   systemctl --user restart sidebar-termd
   ```

6. Check:

   ```sh
   journalctl --user -u sidebar-termd -n 20 --no-pager   # "cgroups: daemon apart from the Sessions in …/sessions; their limits: high 9.0 GB, max 10.0 GB, swap 2.0 GB"
   S=/sys/fs/cgroup$(systemctl --user show sidebar-termd -p ControlGroup --value)
   cat $S/daemon/cgroup.procs                            # the daemon's pid only
   ls $S/sessions/                                       # one <pid>-<id> per Session
   cat $S/sessions/memory.high $S/sessions/memory.max    # 9663676416, 10737418240
   curl -s -o /dev/null -w '%{time_total}\n' http://127.0.0.1:47611/
   ```

   The Mac app reconnects by itself. Rerun what step 1 listed in the Tabs where it was.

Rollback: copy the three files from `~/sidebar-termd-rollback/` back to where they came from,
then run `systemctl --user daemon-reload && systemctl --user restart sidebar-termd`. The old
binary ignores the new layout and runs in the service's cgroup as before. The old `memory.conf`
puts the limits back on the service.

## Tested

A stress test in a cgroup v2 container (Docker Desktop's Linux VM: 3 GB, 4 cores) ran the
daemon with Sessions limited to 1 GB high, 1.2 GB max and 256 MB swap. A script on the Mac
paired over the Host protocol, opened 5 Tabs and ran `stress-ng --vm 1 --vm-bytes 450M
--vm-keep` in each (2.25 GB wanted against a 1 GB budget). Meanwhile it timed ping/pong every
250 ms, and a loop in the container timed `curl 127.0.0.1:<port>/` every 200 ms:

| layout | curl p50 / p99 / max | over 50 ms | ping/pong max |
|---|---|---|---|
| before: daemon and Sessions in one limited cgroup | 0.5 / 1125 / 1140 ms | 29 of 159 | 995 ms |
| `--cgroups` | 0.5 / 1.3 / 1.7 ms | 0 of 239 | 30 ms |

With `--cgroups`, the Sessions were held at their limit throughout (`memory.events` high: 10 759).
The client got 26 `pressure` samples (`ok`, then `throttled`), each naming the Session using the
most memory.
