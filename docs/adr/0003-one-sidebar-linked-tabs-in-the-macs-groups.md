# 3. One sidebar: a paired Host's Tabs are linked into the Mac's own Groups

Status: **accepted** (issue #49, epic #24). Supersedes the sidebar part of 0002 ("Clients own
presentation": a remote Host's snapshot mirrored "under a Host section") and #29's Host sections;
the rest of 0002 stands.

## Context

#29 showed each paired Host as its own section of the Mac's sidebar, with that Host's own Groups.
In use, the user works in a handful of Groups by task, and a task spans machines: an agent on the
Dell, a shell on the Mac, both for the same repo. Two sets of Groups meant keeping two orders in
step, dragging never worked across them, and ⌘T always went to the Host of the Tab in view with
no way to see the machines side by side in one Group.

The Host still has to own its Tabs and Sessions (ADR 0002): a phone paired straight to the Dell
must see the Dell's layout, and closing a Tab there must reach the Session there.

## Decision

**The Mac's layout owns the grouping of every Tab it shows.** A Tab in the Mac's layout is either
local (it has a local Session, as before) or **linked**: it carries `link: {hostId, tabId}`, the
paired Host (this client's id for it) and the Tab's id in that Host's snapshot, and has no Session
here. The Host keeps owning that Tab, its Session, its custom Title and its cwd; its own Groups
no longer matter to the Mac, and still matter to its other clients.

**Linked Tabs persist in the core, in `layout.json`** (version 3), not in the webview. The
alternative, placements kept in the webview's `settings.json`, would have meant a second order
laid over the core's (every Group's `tabIds` interleaved with ids the core does not know), a merge
on every snapshot, and two writers for one sidebar order. In the core a linked Tab is a Tab like
any other in `model.rs`: ordered, moved, activated, closed and deleted with its Group by the same
transitions, with the same tests, so ordering among local and linked Tabs survives relaunch for
free. What makes it linked is only what it lacks: no Session is spawned for it at launch or on
reload, it has no Resume entry, and the monitor never sees it. The Tab's id in the Mac's layout is
the Mac's own (minted as any Tab's); the link holds the Host's.

**Two commands, IPC only** (not on the Host protocol, since only the Mac app links):
`tab_link {hostId, tabId, groupId?, afterTabId?}` places one Tab this Mac just made on a Host,
and `links_reconcile {hostId, tabIds, groupName}` keeps one Host's links in line with the Tabs it
has. Both are pure transitions (`link_tab`, `reconcile_links`) and never change the active Tab.
The webview decides when to call them, because the webview holds the connections to Hosts.

**Reconciling** runs on every `hello` / `layout` from a Host (and after this Mac's own layout
changes) when the links and the Host's Tabs differ (`src/lib/host/links.ts`): a Tab the Host
has with no link (made from a phone, or there before pairing) is a stray, linked at the end of the
first Group named after the Host (made at the end of the Groups if there is none) and marked
Unread; a link whose Tab the Host no longer has goes (its Session ended there). A Host that is
offline keeps its links, greyed, with its last snapshot. Removing a Host in Settings reconciles it
against no Tabs, which drops its links.

Two races are closed in the webview, because a Host's command reply and its `layout` broadcast
can arrive in either order: while this Mac is making a Tab on a Host, and until that Host's
snapshot names it (5 s at most), its links wait, so the new Tab is linked where it was asked for
and never taken for a stray; and a Tab this Mac asked a Host to close is left out of that Host's
Tabs until its snapshot drops it, so unlinking it here first never brings it back as a stray.

**Commands on a linked Tab** go to its Host over the protocol: close (`tab_close` there, then the
link goes), rename (the Title is the Host's `tab_rename`, so its other clients see it), activate
(the Host's active Tab follows, so a `tab_new` there lands next to it). Moving between the Mac's
Groups is this Mac's alone and never touches the Host; drag-and-drop and "Move to Group" work
across local and linked Tabs freely. A move never moves a Session; Handoff (#30) does.

**New Tab goes to the Host of the Tab in view**, in that Tab's Mac Group, right after it: next to
a linked Tab it is made on that Tab's Host (after its Tab there, at its cwd) and linked here; next
to a local Tab it is local. A linked Tab's context menu adds "New Tab on <Host>" and "New Local
Tab"; "New Local Tab" is also a Hotkey action, unassigned by default. "New Tab on <Host>" as a
Hotkey is Handoff's (Cmd-Shift-T), unchanged. Handoff's new Tab goes right after the local Tab it
came from (a Move takes its place); its submenu still names the Host's own Group, which is where
the Tab goes for the Host's other clients.

**Rows.** A linked Tab reads as a local one (icon, Agent status, Badge, Title, Unread, CPU and
memory, close confirmation, file drop, Terminal attach, replay and resize, all from its Host),
plus a chip with the Host's name by the Badge. The chip and the row are greyed while the Host is
not connected; closing one then says the Host is not connected rather than dropping the link.

**The phone paired to the Mac does not see linked Tabs at all.** It reaches the Mac's Remote, not
the Mac's Hosts, so it could neither attach to their Sessions nor close them; greyed rows it can
never use would only be noise. Remote serves the Mac's layout without them (`client_snapshot`),
refuses commands that name one as if it did not exist, and counts a `tab_move` index among the
Tabs it shows. A phone that wants the Dell's Tabs pairs with the Dell.

## Consequences

- The Host sections, `HostHeader.svelte` and per-Host Groups are gone from the sidebar. A Host's
  connection state shows on its Tabs' chips and in Settings > Hosts.
- Go-to-Group numbers count every Group (there are only the Mac's); next / previous Tab walk the
  one list.
- The app now creates a Group on its own, once per Host, when a stray arrives and no Group has
  the Host's name. It never reorders Groups.
- Deleting a Group closes its linked Tabs on their Hosts first. A linked Tab whose Host is not
  connected then only loses its link, and comes back as a stray when the Host does.
- `layout.json` version 3; a version-2 file reads as it is. An older app reading a version-3 file
  would take linked Tabs for local ones and spawn a shell for each.
- Linked Tabs' Titles and cwds are not kept on the Mac: after a relaunch, until the Host's `hello`
  arrives, a linked Tab shows its automatic Title without facts.
- The Host protocol is unchanged for Hosts that link nothing: `link` is absent on the wire.
