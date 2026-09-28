# sidebar-term

A macOS terminal app whose sidebar organises terminal sessions into named groups and shows, per session, whether a coding agent is running and which repo, worktree and branch the shell is in.

## Language

**Session**:
One running shell on its own pty. A session is what a tab points at.
_Avoid_: Terminal, shell instance, pane

**Tab**:
The sidebar entry for one session. It has a title, an icon and a badge, and belongs to exactly one group.
_Avoid_: Item, row, entry

**Linked Tab**:
A Tab in the Mac's Groups whose Session runs on a paired Host: it points at that Host's Tab and has no Session on the Mac. The Host owns the Tab, its Session and its Title; the Mac only places it, and its row carries a chip with the Host's name. A Tab made on the Host from elsewhere is linked into a Group named after the Host.
_Avoid_: Remote Tab (a remote hop is an ssh Session), mirrored Tab, proxy Tab

**Group**:
A user-named, user-ordered container of tabs in the sidebar. Groups are manual; the app never reorders them, and creates one on its own only for a paired Host's Tabs made elsewhere (named after the Host), when no Group has that name.
_Avoid_: Folder, workspace, section

**Agent session**:
A session whose foreground process is a known coding agent: Claude Code, Codex CLI or Gemini CLI. An agent session shows the robot icon instead of the plain-session icon.
_Avoid_: AI tab, bot session

**Foreground process**:
The process currently in control of a session's pty. What the session "is doing" is read from it.
_Avoid_: Running command, child

**Badge**:
The compact repo / worktree / branch indicator on a tab, derived from the session's working directory.
_Avoid_: Label, tag, chip

**Worktree**:
A git worktree: one checkout of a repo at its own path, on its own branch. Sessions in different worktrees of the same repo are related, and the sidebar shows that.
_Avoid_: Checkout, clone

**Hotkey**:
A key combination bound to one app action (new Tab, go to Group 3, next Tab in Group...). Each has a default; the user can rebind or unassign it on the Settings page. One combination drives at most one action.
_Avoid_: Shortcut, keybinding, accelerator

**Title**:
The text shown on a tab. Either automatic (derived from the session) or a rename the user typed, which sticks.
_Avoid_: Name, label

**Tab colour**:
The colour that stands for a tab: its repo's badge-dot colour, or a plain light grey when the session is not in a repo. Tabs in worktrees of one repo share a colour.
_Avoid_: Accent, tint

**Panel**:
The resizable area at the bottom of the sidebar, beneath the groups. An accordion of views (Activity, Usage): each has its own header, and opening one closes the others. A closed view's header carries a one-line summary. It hides when the sidebar is too narrow.
_Avoid_: Drawer, dock, footer, pane

**Tray**:
The row of small icon buttons and indicators at the right of the window bar, in Tabs and Manager alike.
_Avoid_: Toolbar, titlebar buttons, status bar

**Caffeinate**:
The Tray toggle that keeps the Mac awake (display and system) while on, by running macOS's `caffeinate` in the background, never in a Session.
_Avoid_: Keep awake, no-sleep, Amphetamine

**Memory Guard**:
The Tray toggle that, while on, freezes the Tab using the most memory when memory use passes a limit (set in Settings), and thaws frozen Tabs one at a time once memory frees up. It never freezes the Tab in view.
_Avoid_: Load balancer, throttle, governor, auto-pause

**Frozen Tab**:
A Tab whose Session Memory Guard, or the user from the Tab's context menu, has stopped: every process in it is suspended (SIGSTOP) until it is thawed (SIGCONT). It keeps its memory but uses no CPU and does not grow. Going to it thaws it.
_Avoid_: Paused (that is flow control), suspended, sleeping

**Resume**:
Starting again, in the same Tab, what its Session was running when the app last closed (a crash, a quit, a restart to install): a Claude Code conversation (`claude --resume <id>`) or any other command, rerun from its command line. Codex and Gemini conversations are not resumed.
_Avoid_: Restore, recover, reopen (and not the flow-control resume of a paused Session)

**Resume banner**:
The dismissable bar at the bottom of the Terminal area, shown after a launch when the last run closed with Tabs still running something. It lists them, resumes every Claude Code conversation or reruns every command with one button each, or one Tab at a time.
_Avoid_: Toast, notification, crash dialog

**Unread**:
A Tab whose Title is bold because something happened in it the user hasn't looked at yet: its agent finished, stopped or asked for input while the Tab was in the background, or the user marked it unread to come back to it. Going to the Tab clears it.
_Avoid_: Highlighted, new, flagged, bookmarked

**Activity**:
The panel view showing the Mac's CPU and memory, split between sessions (each in its tab colour) and every other process (muted grey), with a list of the busiest processes. Memory is each process's footprint, as Activity Monitor's Memory column. The same samples give each tab its CPU and memory, shown on the tab when turned on in Settings.
_Avoid_: Activity monitor (that is Apple's app), stats, usage

**Usage**:
The panel view showing how much of each chosen coding agent's usage limits is spent: one bar per limit window (Claude Code's 5 hours and week, Codex's), with the time until it resets. Manager shows it too, at the bottom of Needs you. Which agents it shows is chosen on the Settings page.
_Avoid_: Quota, credits, stats, Activity (that is CPU and memory)

**Remote**:
A Host serving its Sessions, Tabs and Groups to clients: while on, a server on 127.0.0.1 that Tailscale Serve publishes to the tailnet, speaking the Host protocol, and the phone's page at `/m`. Off by default on the Mac; turned on in Settings. Always on in the daemon.
_Avoid_: Mobile app, remote access server, web UI

**Host protocol**:
The messages between a Host and a client over Remote's WebSocket: the Host's layout and each Session's facts (Agent status included), output and input, the Tab and Group commands with their replies, plus a file upload. The phone speaks it today; the Mac app will, for a remote Host.
_Avoid_: Remote protocol, API, wire format

**Pairing**:
Letting one phone in: Settings shows a code (as a QR link, or to type); the phone presents it once and gets a token it sends on every connection. A pairing code lasts ten minutes and five wrong tries.
_Avoid_: Login, sign-in, registration

**Paired phone**:
A phone that holds a token this Mac accepts; listed in Settings, where it can be removed. Removing it makes its next connection fail, and it must pair again.
_Avoid_: Device, client (except in code, where it is any connection)

**Attach**:
A client's Terminal taking a Session's output: what the Session printed before, then the live output, with its typing going to the Session's pty. A phone attaches over Remote at the Host's grid size and never resizes the pty; the Mac webview attaches in-process to the local Host's Sessions (which the Host spawns with their Tabs) and sizes them.
_Avoid_: Connect (that is the phone reaching the Mac at all), open (that is a Tab), mirror
**Host**:
A machine running the sidebar-term core, whose Sessions, Tabs and Groups can be shown and driven from a client. The Mac app is its own local Host; a second machine runs the core as a headless daemon (`sidebar-termd`). The Mac shows a paired Host's Tabs as linked Tabs in its own Groups.
_Avoid_: Server, remote machine, node, peer

**Manager**:
The mode the window opens in, beside Tabs: every Agent session as a Lane, the questions agents wait on (answered in place when the agent is Hooked), finished work not looked at, and the selected Tab's Terminal, to type into. The edges between the three drag. Its Tabs are the sidebar's; opening one goes to Tabs.
_Avoid_: Dashboard, overview, mission control

**Lane**:
One Agent session's row in Manager: its Title, then its Agent status over time (working, waiting on you, idle at prompt), then what it is doing now. Selecting one shows its Tab's Terminal in Manager; it does not open its Tab.
_Avoid_: Track, timeline, swimlane

**Hooked**:
An Agent session whose agent reports to its Host through hooks (a Claude Code started through sidebar-term's `claude`): its questions come with their answers and can be answered from Manager. Every other agent is **screen-only**: only its Agent status and its screen are known.
_Avoid_: Integrated, instrumented, connected

**Agent event**:
One thing an agent did, as its Host recorded it: a tool it used, a question it asked, an answer given, or (for a screen-only agent) a change of its Agent status. An agent's name and what its last turn changed come from them.
_Avoid_: Log, activity (that is CPU and memory), history (that is a Lane's status over time)

**Handoff**:
Moving a Tab to another Host: its Session is killed here after its Resume entry is recorded, a Tab is created on the Host at the matching checkout, and the entry is rerun there. Code moves by push and checkout, never by copying files.
_Avoid_: Migrate, transfer, sync

**Checkout root**:
The directory on a Host under which its repos are checked out, set per Host in Settings: a Tab in repo `x` lands in `<root>/x` there on a Handoff or a New Tab on that Host, unless an override names that repo's checkout elsewhere.
_Avoid_: Workspace, projects dir, base path

**Journal**:
What a Host keeps on disk about its Agent sessions, for Rewind: a span for every stretch of one Agent status in one Tab, repo, Worktree and branch. Each Host keeps its own in its data dir, and nothing in it leaves the machine.
_Avoid_: Log, history (that is a Lane's status over time), telemetry, analytics, usage log

**Agent time**:
Time Agent sessions spent in an Agent status, summed over the Journal's spans. Two agents working through the same hour are two hours of agent time.
_Avoid_: Usage (that is limit windows), uptime, runtime

**Rewind**:
The look back over a day or a week, drawn from the Journal: which repo had the most agent time, how long agents waited on you, and so on.
_Avoid_: Stats, analytics, report, recap, Usage (that is limit windows), Activity (that is CPU and memory)
