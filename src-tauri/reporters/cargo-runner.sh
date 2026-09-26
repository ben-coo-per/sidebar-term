#!/bin/bash
# sidebar-term's cargo target runner (CARGO_TARGET_<TRIPLE>_RUNNER="<this file> <triple>"): runs
# every binary cargo hands it exactly as cargo would, and reports test binaries to the Suite
# progress file so the Tab shows how far a `cargo test` or `cargo nextest run` is.
#
#   <this file> <triple> <binary> [args...]
#
# - A `runner` the repo (or ~/.cargo) configured for the triple in `.cargo/config.toml` wins:
#   the binary is handed to it untouched, nothing is reported.
# - Only binaries under `target/<profile>/deps/` are test binaries; `cargo run` binaries and
#   build scripts are exec'd as they are.
# - `cargo test`: the binary is asked `--list --format terse` first (its test count), then run;
#   its exit status says passed or failed. A `harness = false` binary gets that `--list` too.
# - nextest: its `--list` runs are passed through (and counted), and each `--exact <test>`
#   process is one case, passed or failed by its exit status.
# The binary's stdin, stdout, stderr, arguments and exit status are what they would have been.
# Protocol: docs/architecture.md "Suites".

triple="$1"; shift
bin="$1"; shift

# Prints the `runner` of `[target.<triple>]` in a cargo config file, if there is one.
runner_in() {
  local raw
  raw="$(awk -v s1="[target.$triple]" -v s2="[target.\"$triple\"]" -v s3="[target.'$triple']" '
    /^[[:space:]]*\[/ { line = $0; gsub(/[[:space:]]+$/, "", line); sub(/^[[:space:]]+/, "", line)
                        in_section = (line == s1 || line == s2 || line == s3); next }
    in_section && /^[[:space:]]*runner[[:space:]]*=/ {
      sub(/^[[:space:]]*runner[[:space:]]*=[[:space:]]*/, ""); print; exit }' "$1" 2>/dev/null)"
  [ -n "$raw" ] || return 1
  case "$raw" in
    \[*) raw="$(printf '%s' "$raw" | tr -d '[]"' | tr ',' ' ')" ;;   # ["cmd", "arg"]
    \"*) raw="${raw#\"}"; raw="${raw%%\"*}" ;;                        # "cmd arg"
    \'*) raw="${raw#\'}"; raw="${raw%%\'*}" ;;
  esac
  CONFIGURED="$raw"
}

configured_runner() {
  local dir="$PWD" f
  while :; do
    for f in "$dir/.cargo/config.toml" "$dir/.cargo/config"; do
      [ -f "$f" ] && runner_in "$f" && return 0
    done
    [ "$dir" = "/" ] && break
    dir="$(dirname "$dir")"
  done
  for f in "${CARGO_HOME:-$HOME/.cargo}/config.toml" "${CARGO_HOME:-$HOME/.cargo}/config"; do
    [ -f "$f" ] && runner_in "$f" && return 0
  done
  return 1
}

if configured_runner; then
  # shellcheck disable=SC2086  # the configured runner is "cmd args", split as cargo splits it
  exec $CONFIGURED "$bin" "$@"
fi

case "$bin" in
  */target/*/deps/*) ;;
  *) exec "$bin" "$@" ;;
esac
dir="$SIDEBAR_TERM_PROGRESS_DIR"
if [ -z "$dir" ] || [ ! -d "$dir" ]; then
  exec "$bin" "$@"
fi
file="$dir/$$.jsonl"

emit() { printf '{"t":%s000,%s}\n' "$(date +%s)" "$*" >> "$file" 2>/dev/null; }
has() { local want="$1" a; shift; for a in "$@"; do [ "$a" = "$want" ] && return 0; done; return 1; }

if has --list "$@"; then
  # nextest listing the tests: pass the output through, and count it once (not the
  # `--ignored` listing, which nextest only runs when asked to run ignored tests).
  if has --ignored "$@"; then
    exec "$bin" "$@"
  fi
  emit '"ev":"start","runner":"cargo","pid":'"$$"
  tmp="$(mktemp "${TMPDIR:-/tmp}/sidebar-term-list.XXXXXX")" || exec "$bin" "$@"
  "$bin" "$@" | tee "$tmp"
  rc=${PIPESTATUS[0]}
  n=$(grep -c ': test$' "$tmp")
  rm -f "$tmp"
  [ "$rc" -eq 0 ] && emit '"ev":"total","total":'"$n"
  exit "$rc"
fi

if has --exact "$@"; then
  # nextest running one test in its own process.
  "$bin" "$@"
  rc=$?
  if [ "$rc" -eq 0 ]; then status=passed; else status=failed; fi
  emit '"ev":"case","status":"'"$status"'"'
  exit "$rc"
fi

# cargo test: one process runs the binary's tests as threads. `--list` counts ignored tests
# too, so take those off unless the run includes them.
emit '"ev":"start","runner":"cargo","pid":'"$$"
n=$("$bin" "$@" --list --format terse 2>/dev/null </dev/null | grep -c ': test$')
if ! has --ignored "$@" && ! has --include-ignored "$@"; then
  ignored=$("$bin" "$@" --list --ignored --format terse 2>/dev/null </dev/null | grep -c ': test$')
  n=$((n - ignored))
  [ "$n" -lt 0 ] && n=0
fi
emit '"ev":"total","total":'"$n"
"$bin" "$@"
rc=$?
if [ "$rc" -eq 0 ]; then status=passed; else status=failed; fi
emit '"ev":"end","status":"'"$status"'"'
exit "$rc"
