#!/usr/bin/env bash
# Launch `pnpm tauri dev` with two fake coding agents so the sidebar's agent icon and status can be
# seen without running a real agent. Open a Tab in /tmp for a fake Claude Code (streams output,
# then rings the bell: Running -> Needs input) and a Tab in any directory named `fake-codex` for a
# fake Codex (spinner title, then "Action Required"). Your real ~/.zsh* files are still sourced.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
work="${TMPDIR:-/tmp}/sidebar-term-demo"
mkdir -p "$work/claude/versions" "$work/bin" "$work/zdot" "$work/fake-codex"
cc -O2 -o "$work/claude/versions/9.9.9" "$root/scripts/demo/fake-agent.c"   # path shape = native Claude Code
cp "$work/claude/versions/9.9.9" "$work/bin/codex"                             # comm = codex
for f in zshenv zprofile zlogin; do printf '[[ -f ~/.%s ]] && source ~/.%s\n' "$f" "$f" > "$work/zdot/.$f"; done
cat > "$work/zdot/.zshrc" <<RC
[[ -f ~/.zshrc ]] && ZDOTDIR=~ source ~/.zshrc
case "\$PWD" in
  /private/tmp|/tmp) "$work/claude/versions/9.9.9" ;;
  */fake-codex) "$work/bin/codex" ;;
esac
true
RC
echo "Fake Codex directory: $work/fake-codex"
cd "$root" && ZDOTDIR="$work/zdot" exec pnpm tauri dev
