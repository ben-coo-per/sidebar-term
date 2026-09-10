#!/usr/bin/env bash
# Build the release app, install it to /Applications, and pin it to the Dock (only the first time).
# Re-run after changing the icon or the code: `pnpm app:install`. Safe to run from inside the app:
# if it is running, it builds first, then quits the app, installs and relaunches it. Tabs come back
# at their last cwd; whatever was running in them (agents included) is killed.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
name="sidebar-term"
bundle_id="com.bencooper.sidebarterm"
dest="/Applications/$name.app"
log="${TMPDIR:-/tmp}/$name-install.log"

# Ask LaunchServices, by bundle id: `pgrep -x sidebar-term` misses the running app on macOS 26.
is_running() {
  [[ "$(osascript -e "application id \"$bundle_id\" is running" 2>/dev/null)" == true ]]
}

install_built() {
  local built="$1" icon_changed=1 restart_dock=0
  if [[ -f "$dest/Contents/Resources/icon.icns" ]] &&
     cmp -s "$built/Contents/Resources/icon.icns" "$dest/Contents/Resources/icon.icns"; then
    icon_changed=0
  fi

  rm -rf "$dest"
  ditto "$built" "$dest"
  touch "$dest"
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$dest"
  echo "Installed $dest"

  if ! defaults read com.apple.dock persistent-apps 2>/dev/null | grep -q "$name.app"; then
    defaults write com.apple.dock persistent-apps -array-add \
      "<dict><key>tile-data</key><dict><key>file-data</key><dict><key>_CFURLString</key><string>$dest</string><key>_CFURLStringType</key><integer>0</integer></dict></dict></dict>"
    echo "Pinned to the Dock"
    restart_dock=1
  fi
  [[ $icon_changed == 1 ]] && restart_dock=1
  if [[ $restart_dock == 1 ]]; then
    killall Dock
    echo "Dock restarted to show the current icon"
  fi
}

# Second stage, detached from the app (own session, see below): quit it, install, relaunch.
if [[ ${1:-} == --swap ]]; then
  built="$2"
  osascript -e "quit app id \"$bundle_id\"" || true
  for _ in {1..60}; do
    is_running || break
    sleep 0.5
  done
  if is_running; then
    echo "$name did not quit within 30 s; nothing installed." >&2
    exit 1
  fi
  install_built "$built"
  open "$dest"
  exit 0
fi

# Ask cargo: a worktree builds into the main checkout's target (.claude/worktrees/.cargo/config.toml).
target="$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/src-tauri/Cargo.toml" |
  node -e 'let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => console.log(JSON.parse(s).target_directory))')"

cd "$root"
pnpm tauri build --bundles app
built="$target/release/bundle/macos/$name.app"

if is_running; then
  # Quitting the app SIGHUPs (then SIGKILLs) each Tab's process group, which may include this
  # script. So the rest runs in a new session (setsid), outside every Tab's group and terminal.
  echo "Built. Quitting $name to install it; it relaunches in a few seconds (log: $log)."
  nohup /usr/bin/perl -MPOSIX -e 'setsid() >= 0 or die "setsid: $!\n"; exec @ARGV or die "exec: $!\n"' \
    "$root/scripts/install-app.sh" --swap "$built" >"$log" 2>&1 </dev/null &
  exit 0
fi

install_built "$built"
