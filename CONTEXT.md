# sidebar-term

A macOS terminal app whose sidebar organises terminal sessions into named groups and shows, per session, whether a coding agent is running and which repo, worktree and branch the shell is in.

## Language

**Session**:
One running shell on its own pty. A session is what a tab points at.
_Avoid_: Terminal, shell instance, pane

**Tab**:
The sidebar entry for one session. It has a title, an icon and a badge, and belongs to exactly one group.
_Avoid_: Item, row, entry

**Group**:
A user-named, user-ordered container of tabs in the sidebar. Groups are manual; the app never creates or reorders them on its own.
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
The row of small icon buttons and indicators at the top of the sidebar, beside the traffic lights. Shown whenever the sidebar is.
_Avoid_: Toolbar, titlebar buttons, status bar

**Caffeinate**:
The Tray toggle that keeps the Mac awake (display and system) while on, by running macOS's `caffeinate` in the background, never in a Session.
_Avoid_: Keep awake, no-sleep, Amphetamine

**Resume**:
Starting again, in the same Tab, what its Session was running when the app last closed (a crash, a quit, a restart to install): a Claude Code conversation (`claude --resume <id>`) or any other command, rerun from its command line. Codex and Gemini conversations are not resumed.
_Avoid_: Restore, recover, reopen (and not the flow-control resume of a paused Session)

**Resume banner**:
The dismissable bar at the bottom of the Terminal area, shown after a launch when the last run closed with Tabs still running something. It lists them, resumes every Claude Code conversation or reruns every command with one button each, or one Tab at a time.
_Avoid_: Toast, notification, crash dialog

**Activity**:
The panel view showing the Mac's CPU and memory, split between sessions (each in its tab colour) and every other process (muted grey), with a list of the busiest processes.
_Avoid_: Activity monitor (that is Apple's app), stats, usage

**Usage**:
The panel view showing how much of each chosen coding agent's usage limits is spent: one bar per limit window (Claude Code's 5 hours and week, Codex's), with the time until it resets. Which agents it shows is chosen on the Settings page.
_Avoid_: Quota, credits, stats, Activity (that is CPU and memory)
