# 4. The Journal is a tally per hour, in append-only JSONL, kept by each Host

Status: **accepted** (issue #63). Builds on 0002: the Host owns its Sessions and what is known
about them, and persists it in its own data dir.

## Context

Rewind (#63) looks back over a day or a week: agent time per repo, how long agents waited on the
user, what it cost. The core derives all of it each monitor tick and keeps five hours of it in
memory. Something has to be written down, on a laptop and on a small headless machine running
for months, by a process that can be killed at any moment. It is kept locally and for good, so
it must not grow into something the user has to clean up.

The first version wrote a row when something ended: a span for every stretch of one Agent
status in one Tab, a row of counts for every turn of a hooked agent, a row whenever Claude
Code's totals moved. At the rate measured on the Dell on 2026-09-28 (137 turns) that came to
about 58 KB a day, and it grew with what agents did: a status that flickers, or a day of many
short turns, made a larger file. How much screen-derived status flickers was not known.

## Decision

**One tally per hour and Context, written when the hour is over.** A Context is the agent, the
repo and the branch. A tally is seconds worked, seconds waited on the user, prompts given,
lines added and removed, and output tokens and cost per model. It is kept in memory through the
hour and written as one row. The Journal's size is bounded by hours and repos, not by events.

**Counts only.** No prompt, no command, no path but the repo's, and not the Tab or the
Worktree.

**JSONL, appended, one file per UTC month**, `journal/<year>-<month>.jsonl` in the Host's data
dir. A Context is written once as a `ctx` row and named by number on the rows after it; numbers
hold until redefined, and each run starts again at 1, so nothing is read back at launch. Rows
of one hour and Context add up, so a Host that stops within the hour writes what it has.

**Tallies not yet written survive a crash through `journal/open.json`**, rewritten every
minute, and written as rows by the next launch.

**Each Host keeps its own Journal.** The Mac reads a paired Host's over the Host protocol and
merges; Journals are never copied between machines.

## Alternatives

- **A row per span, turn and reading of the totals** (the first version). Everything a tally
  has and more: a day as a timeline, the most agents at once, how long one question waited.
  Six times the size at the rate measured, and unbounded.
- **A tally per day.** A sixth of the rows again. It cannot say at what hours agents worked or
  the user kept them waiting, which is half of what a Rewind shows, and a day has to be
  someone's time zone's.
- **The Tab and the Worktree in the Context.** More rows for what Rewind has no use for: a Tab's
  id means nothing once the Tab is closed, and a Worktree is nearly always a branch.
- **Every token count per model** (input, cache reads and writes, thinking). Cost already
  weighs them; output tokens are the one count a person reads.
- **SQLite.** Queries and indexes for free. It adds a C library to both binaries (the core has
  no database today), and a schema to migrate. A year of the Journal is a few megabytes and is
  read whole in milliseconds, so there is nothing for an index to speed up.
- **One JSON file, rewritten** (as `layout.json` and `resume.json` are). Every write costs the
  whole file, and a crash mid-write risks all of it rather than one line.
- **A binary format.** Unreadable with `grep` and `python3`, which is how a file like this gets
  debugged, for a file that is already small.

## Consequences

- About 9 KB a day and 3.5 MB a year: the hours, repos and branches of 2026-09-28 on the Dell
  (113 tallies in 12 hours, up to 20 repos and branches in one hour) at 85 bytes a row.
- What happened within an hour is gone: no timeline of a Tab's day, no count of agents at one
  moment (the hour's seconds worked, over 3600, is how many on average), no single turn.
- Cost is counted in the hour Claude Code wrote its totals, which it does seldom: right over a
  day or a week, not hour by hour.
- A tally is invisible in the files until its hour is over; `Journal::read` adds what is in
  memory, so a reader must go through the Host that owns the Journal, not the files alone.
- The shape of a row is now something old files hold. New kinds of row get a new `k`, new
  counts a new field, and a reader skips what it does not know.
