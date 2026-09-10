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
The collapsible, resizable area at the bottom of the sidebar, beneath the groups. It shows one view at a time, picked from a strip of view tabs in its header. It hides when the sidebar is too narrow.
_Avoid_: Drawer, dock, footer, pane

**Activity**:
The panel view showing the Mac's CPU and memory, split between sessions (each in its tab colour) and every other process (muted grey), with a list of the busiest processes.
_Avoid_: Activity monitor (that is Apple's app), stats, usage
