#!/bin/bash
# sidebar-term's `go test -exec` wrapper (GOFLAGS=-exec=<this file>): runs each package's test
# binary as `go test` would, and reports it to the Suite progress file so the Tab shows how far
# the run is.
#
#   <this file> <pkg.test> [-test.* args...]
#
# - `go run` uses -exec too: a binary not named `*.test` is exec'd untouched.
# - The binary is asked `-test.list '^(Test|Example)'` first (its test count: this runs the
#   package's TestMain, if it has one, once more), then run; its exit status says passed or failed.
# The binary's stdin, stdout, stderr, arguments and exit status are what they would have been.
# Protocol: docs/architecture.md "Suites".

bin="$1"; shift

case "$bin" in
  *.test) ;;
  *) exec "$bin" "$@" ;;
esac
dir="$SIDEBAR_TERM_PROGRESS_DIR"
if [ -z "$dir" ] || [ ! -d "$dir" ]; then
  exec "$bin" "$@"
fi
file="$dir/$$.jsonl"

emit() { printf '{"t":%s000,%s}\n' "$(date +%s)" "$*" >> "$file" 2>/dev/null; }

emit '"ev":"start","runner":"go","pid":'"$$"
n=$("$bin" -test.list '^(Test|Example)' 2>/dev/null </dev/null | grep -c '^[A-Za-z_]')
emit '"ev":"total","total":'"$n"
"$bin" "$@"
rc=$?
if [ "$rc" -eq 0 ]; then status=passed; else status=failed; fi
emit '"ev":"end","status":"'"$status"'"'
exit "$rc"
