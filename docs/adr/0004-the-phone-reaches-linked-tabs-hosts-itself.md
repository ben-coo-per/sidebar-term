# 4. The phone shows linked Tabs, and reaches their Hosts itself

Status: **accepted**. Supersedes the last paragraph of 0003 ("The phone paired to the Mac does
not see linked Tabs at all"); the rest of 0003 stands.

## Context

0003 put a paired Host's Tabs into the Mac's own Groups, and left them out of what Remote serves:
a phone reaches the Mac, not the Mac's Hosts, so rows it could never open would only be noise. "A
phone that wants the Dell's Tabs pairs with the Dell", which meant a second home-screen page with
the Dell's own Groups, and none of the Mac's grouping by task.

In use that is the wrong half. The agents that run for hours run on the Dell; the phone is what
is at hand when the Mac is closed; and Manager, whose point is every agent in one place, showed
the phone only the Mac's.

Three ways for a phone paired with the Mac to reach a Session on the Dell:

1. **The Mac relays.** One pairing, one connection. But the webview holds the connections to
   Hosts, so the core would have to grow its own; every keystroke takes two hops; and nothing
   works while the Mac sleeps, which is when the phone is wanted.
2. **The Mac hands the phone its own token for the Dell.** No second pairing. But the Dell would
   list one client where there are two, and removing the phone on the Mac would not take away
   what it holds for the Dell.
3. **The phone pairs with the Dell itself**, told by the Mac where the Dell is.

## Decision

**The phone is a client of every Host its Tabs run on (3).** The page's own Host (the one it was
loaded from) gives the Groups, its own Tabs, its linked Tabs, and the Hosts those point at; the
phone connects to each of those Hosts directly, with a token that Host gave it for a pairing code,
as any client's.

**A client asks for linked Tabs.** `auth` takes `links: true`; the Host then sends its layout
whole (`Layout::snapshot`, linked Tabs included) in `hello` and `layout`, and the Hosts they
point at as `hello.hosts` and `hosts {hosts}` on change: `{id, url, name}` each, `id` being what
a linked Tab's `link.hostId` names. Without `links` a client gets what it got before
(`client_snapshot`, and no Hosts), so the Mac app as a client of another Mac is unchanged. No
command on a linked Tab is served through the Host that links it, for any client: the phone
sends the Host that owns the Tab what is that Host's to do (`attach`, `input`, `answer`,
`release`).

**The webview tells the core which Hosts there are** (`remote_hosts_set`, IPC only), at launch
and on every change, because the webview holds the pairings (`settings.json`, section `hosts`).
Never the tokens. The core keeps the list in memory; the daemon links no Tabs and has none.

**Pairing across origins.** The phone's page is the Mac's (`https://<mac>.<tailnet>.ts.net`) and
posts a pairing code to the Dell (`https://<dell>.<tailnet>.ts.net/api/pair`), which a browser
allows only if the Dell's CORS answer names the page's origin. A Host answers for the origins it
did before (the Mac app's webview) and for `https://<machine>.<its own tailnet>`: a page served
by a machine on its tailnet. What admits a client is unchanged: the code, then the token. The
WebSocket needs nothing: browsers do not apply CORS to it, and the token is its first frame.

**The phone remembers what the page's Host last said** (`localStorage`: its layout, facts and
Hosts), so after a reload with the Mac asleep the Groups are there, the Mac's own Tabs greyed,
and the Dell's Tabs open as ever.

**A Tab a Host has that no link places** (made there while the Mac was not looking) is listed in
the Group named after the Host, where the Mac will link it (`reconcile_links`), made on the phone
when the Mac has none. A link whose Tab the Host no longer has is left out.

## Consequences

- Each Host lists the phone among its paired clients and can remove it; removing it on the Mac
  does not remove it on the Dell. "Forget this Host" on the phone drops every token it holds.
- A Host must run a core that knows `links` and answers CORS for its tailnet before a phone can
  pair with it from another Host's page. An older one refuses the pairing (the browser reports
  it as unreachable); once paired by other means its socket would serve the phone as before.
- A pairing code for a Host has to come from that Host: its Settings on a Mac, `SIGUSR1` and the
  log on the daemon. Starting one from the Mac app, over the Host protocol, is the obvious next
  step, and not built.
- The phone holds one connection per Host. With the Mac asleep it cannot learn of a Host paired
  since, or of a Tab moved between the Mac's Groups.
- Manager on the phone (every Agent session across those Hosts) follows from the same mirror:
  see docs/architecture.md "The phone".
