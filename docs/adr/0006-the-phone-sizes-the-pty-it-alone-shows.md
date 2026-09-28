# 6. The phone sizes the pty of a Session it alone shows

Status: **accepted**. Supersedes "The phone renders at the Mac's grid and never resizes the pty"
in 0002 (remote clients); the rest of 0002 stands.

## Context

0002 had the phone draw a Session at its Host's grid, the font shrunk until the Host's columns
fit the width, so that attaching disturbed nothing on the Mac. It foresaw the cost: "legible for
agent prompts and small edits, not for long work". In use it is worse than that. A Mac window is
150 to 220 columns wide; on a phone in portrait that is a 6 px font and still a grid to pan
across, and nothing on it can be read.

A font the phone can read shows about 50 columns. At the Host's grid that is a quarter of each
line, and every line of a program that fills the width has to be panned across. What reads well
is the program laid out for the phone: the pty sized to what the phone shows.

The Host protocol already has the rule that keeps this from disturbing anyone: `resize` is
honoured only for a client attached to a Session that no other client shows (0002, host daemon).

## Decision

**The phone asks its Host to size the pty** to the grid that fits its screen at the text size it
reads, on attach and whenever that grid changes (the text size, the keyboard, the phone turned).
The Host's rule is unchanged: it refuses while another client shows the Session.

**The phone gives the size back.** It remembers the size it found, and asks for it when the
Terminal is left or the page is hidden, so the pty is the phone's size only while the phone
shows it.

**Refused, the phone keeps the Host's grid** at the text size it reads and pans across it, says
that another client shows the Session, and asks again every 15 seconds: once the other client's
connection is gone (the Mac asleep, the app closed), the phone takes the size. The old fit to the
width is still there to choose, for a look at the whole screen.

## Consequences

- A program gets a SIGWINCH when the phone opens its Terminal and another when it leaves, as from
  a window resized twice.
- While the Mac app shows a Host's Sessions (it attaches to every Tab's, shown or not), the phone
  is refused on that Host, and on the Mac itself always. The phone reads best with the Mac asleep
  or the app closed, which is when it is most wanted, but not only then. Letting the phone take
  the size from a Mac nobody is at, and the Mac take it back when it is used, needs the Mac to
  follow `resized` and the Host to arbitrate; not built.
- A phone that goes away without a word (the page killed, the network gone) leaves the pty at its
  size. The Mac sizes the pty when it next attaches and shows the Session. A Mac that attached
  while the phone still showed the Session was refused, and does not ask again by itself until
  its Terminal changes size (`manager.ts` takes a refusal for done).
