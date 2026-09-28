# 4. The Journal is append-only JSONL, a file per month, kept by each Host

Status: **accepted** (issue #63). Builds on 0002: the Host owns its Sessions and what is known
about them, and persists it in its own data dir.

## Context

Rewind (#63) looks back over a day or a week: agent time per repo, how long agents waited on the
user, how many ran at once. The core derives all of it each monitor tick and keeps five hours of
it in memory. Something has to be written down, on a Host that may be a small headless machine
running for months, by a process that can be killed at any moment, and it should cost next to
nothing in disk, in dependencies and in the daemon's size.

Measured on the Dell on 2026-09-28: 137 turns and 1,410 tool uses by hooked agents, against
125 MB of Claude Code transcripts written the same day.

## Decision

**Spans, written when they end.** One row per stretch of one Agent status in one Context (Tab,
agent, repo, Worktree, branch). Nothing is sampled on a timer, so an idle Host writes nothing but
`open.json`, and a chart of time per anything is a sum over rows.

**JSONL, appended, one file per UTC month**, `journal/<year>-<month>.jsonl` in the Host's data
dir. A Context is written once as a `ctx` row and named by number on the spans after it; numbers
hold until redefined, and each run starts again at 1, so nothing is read back at launch. A span
over the end of a month is split, so each file stands alone.

**Open spans survive a crash through `journal/open.json`**, rewritten every 30 s with the time,
and turned into spans by the next launch.

**Each Host keeps its own Journal.** The Mac reads a paired Host's over the Host protocol and
merges; Journals are never copied between machines.

## Alternatives

- **SQLite.** Queries and indexes for free, and rows of about the same size. It adds a C
  library to both binaries (the core has no database today), a file that is not compressed and
  does not shrink without a vacuum, and a schema to migrate. A month of the Journal is about
  1 MB and is read whole in milliseconds, so there is nothing for an index to speed up.
- **One JSON file, rewritten** (as `layout.json` and `resume.json` are). Every write costs the
  whole file, and a crash mid-write risks all of it rather than one line.
- **A row per tool use.** Ten times the rows of a turn-level roll-up (1,410 against 137 that
  day), for detail no chart in #63 draws. Agent events will be written a row per turn.
- **A binary format.** Smaller than JSONL before compression and no smaller after; unreadable
  with `grep` and `python3`, which is how a file like this gets debugged.
- **Local-time days as files.** A Host and its clients may be in different time zones, and the
  zone changes twice a year. UTC months only say which file; the reader cuts spans to whatever
  day it is asked for.

## Consequences

- About 37 KB a day and 14 MB a year at the rate measured (taking 4 status changes a turn), and
  1.8 MB a year gzipped. Compressing a month once it is over is slice 9 of #63, and needs a
  reader that opens both.
- A reader has to read a file from its top to know what a number means. Fine at this size.
- A span is invisible in the files until it ends; `Journal::read` adds the open ones from
  memory, so a reader must go through the Host that owns the Journal, not the files alone.
- The shape of a row is now something old files hold. New kinds of row get a new `k`, and a
  reader skips the kinds (and the lines) it does not know.
- Whether screen-derived status flaps enough to matter is not known yet (#63, "Open").
