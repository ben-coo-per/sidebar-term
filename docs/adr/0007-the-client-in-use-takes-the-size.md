# 7. The client in use takes the pty's size

Status: **accepted**. Supersedes "The Host's rule is unchanged: it refuses while another client
shows the Session" and "Refused, the phone keeps the Host's grid" in 0006; the rest of 0006
stands (the phone sizes the pty to what it reads, and gives the size back).

## Context

0006 had the phone size the pty of a Session it alone shows, and pan across the Host's grid
when another client shows it too. It named what that left out: "Letting the phone take the size
from a Mac nobody is at, and the Mac take it back when it is used, needs the Mac to follow
`resized` and the Host to arbitrate; not built."

In use the phone is refused nearly always. The Mac app attaches to every Tab's Session, shown or
not, on the Mac and on every paired Host, so a Mac left awake at home refuses the phone on all of
them. What the phone shows then is 212 columns at a size it can read: a sixth of each line, and
every line dragged sideways. The sideways scroll is the thing to be rid of.

Three rules a Host could follow when two clients want different sizes:

1. **The smallest wins.** Every client says what fits it, and the pty is the smallest. The Mac's
   Terminal is phone-sized for as long as a phone is attached, used or not.
2. **The last to ask wins.** The Mac fits after every layout change, its user there or not, and
   takes the size from the phone in hand.
3. **The one in use wins**, and says so when it asks.

## Decision

**A client whose user is at it takes the size (3).** `resize` carries `take`, and with it the
Host sizes the pty though other clients show the Session. Without it the rule of 0002 stands:
only a client that alone shows the Session may size it. The Host does not judge who is in use:
the client knows, and says.

**Every other client follows.** The Host tells all of them `resized` (the Mac's own webview
`session-resized`), and each draws the pty's grid, not what fits its own screen, until it is
used again and takes the size back.

**What counts as use:**

- The phone: opening a Terminal, coming back to the page, a finger anywhere on the Terminal's
  screen, a key typed. Not a reconnect, not the keyboard sliding, not what a Terminal answers a
  program by itself.
- The Mac: its window has the focus when a Terminal is fitted (a Tab shown, the window or the
  sidebar dragged), the window gets the focus, a click or a key in a Terminal.

**The phone gives back by taking.** It remembers the size it found and takes it for the pty when
it leaves, as in 0006; a size another client took in the meantime is that client's, and is not
given back.

## Consequences

- Nothing scrolls sideways on the phone once its Host knows `take`. Under a Host that does not
  (it ignores the field), the phone is refused as before and pans; the option to shrink the text
  to the width stays for that.
- While a phone has the size, the Mac's Terminal for that Session is a phone-sized grid in the
  corner of its pane, and one with more rows than the pane has is cut off at the bottom. It is
  put right as soon as the Mac is used.
- Two clients both in use (a phone in hand beside the Mac being typed at) trade the size with
  every touch, a SIGWINCH each time. Nothing stops it; it is one person at two screens.
- A Mac app from before this ADR never takes and never follows: beside a phone that has the size
  its Terminal draws a 63-column program in a 212-column grid until the phone gives the size
  back.
- A client can take the size without its user there by setting `take` whenever it asks. The
  clients are ours, and paired.
