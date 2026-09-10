#!/usr/bin/env bash
# Build the release app, install it to /Applications, and pin it to the Dock (only the first time).
# Re-run after changing the icon or the code: `pnpm app:install`.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
name="sidebar-term"
dest="/Applications/$name.app"
# Ask cargo: a worktree builds into the main checkout's target (.claude/worktrees/.cargo/config.toml).
target="$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/src-tauri/Cargo.toml" |
  node -e 'let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => console.log(JSON.parse(s).target_directory))')"

if pgrep -x "$name" >/dev/null; then
  echo "Quit $name first (Cmd-Q), then run this again." >&2
  exit 1
fi

cd "$root"
pnpm tauri build --bundles app
built="$target/release/bundle/macos/$name.app"

icon_changed=1
if [[ -f "$dest/Contents/Resources/icon.icns" ]] &&
   cmp -s "$built/Contents/Resources/icon.icns" "$dest/Contents/Resources/icon.icns"; then
  icon_changed=0
fi

rm -rf "$dest"
ditto "$built" "$dest"
touch "$dest"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$dest"
echo "Installed $dest"

restart_dock=0
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
