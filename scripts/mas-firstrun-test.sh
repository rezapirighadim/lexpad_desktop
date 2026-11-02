#!/bin/sh
# The Mac App Store copy's first launch, as after a fresh install: does ⌘⇧L's
# Services entry come alive with no second launch? (services_live.rs; the
# owner's TestFlight 0.3.27 needed a relaunch, 1 Nov 2026.)
#
#   scripts/mas-firstrun-test.sh [path/to/Lexpad.app]
#
# It copies the --local build of scripts/release-mas.sh into ~/Applications
# under an identifier and a service name nobody has ever used
# (app.lexpad.ios.firstrun.<random>, "Add to Lexpad (first-run test …)"), so
# macOS has no Services entry and no container for it: a fresh install. The
# real app's entry, the dmg's and the owner's settings are never touched.
# It starts the copy with --login (no window), waits for the core's probe to
# say the service is live, checks macOS lists the service with its key, and
# that no window opened. Then it removes the copy, its LaunchServices record
# and its Services entry (`pbs -update`, a rescan; never -flush). The menu
# bar shows the copy's icon for those seconds.
set -eu

cd "$(dirname "$0")/.."
src=${1:-src-tauri/target/universal-apple-darwin/release/bundle/macos/Lexpad.app}
[ -d "$src" ] || { echo "mas-firstrun-test: no app at $src (scripts/release-mas.sh --local)" >&2; exit 1; }
tag=$(openssl rand -hex 4)
id="app.lexpad.ios.firstrun.$tag"
name="Add to Lexpad (first-run test $tag)"
app="$HOME/Applications/LexpadFirstRun-$tag.app"
lsreg=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
log=$(mktemp)
ents=$(mktemp -t lexpad-firstrun).plist
pid=""

cleanup() {
  [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
  sleep 1
  "$lsreg" -u "$app" 2>/dev/null || true
  rm -rf "$app" "$log" "$ents"
  /System/Library/CoreServices/pbs -update 2>/dev/null || true
}
trap cleanup EXIT

mkdir -p "$HOME/Applications"
ditto "$src" "$app"
plist="$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $id" -c "Set :NSServices:0:NSMenuItem:default $name" "$plist"
cp src-tauri/mas/Lexpad.entitlements "$ents"
/usr/libexec/PlistBuddy -c 'Delete :com.apple.application-identifier' -c 'Delete :com.apple.developer.team-identifier' "$ents"
identity=$(security find-identity -v -p codesigning | sed -n "s/.*\"\(Apple Development: [^\"]*\)\".*/\1/p" | head -1)
codesign --force --sign "$identity" --options runtime --entitlements "$ents" "$app" 2>/dev/null

if /System/Library/CoreServices/pbs -dump 2>/dev/null | grep -q "$id"; then
  echo "mas-firstrun-test: macOS already knows $id; not a fresh install" >&2
  exit 1
fi

start=$(date +%s)
"$app/Contents/MacOS/lexpad-desktop" --login 2>"$log" >/dev/null &
pid=$!
live=""
for _ in $(seq 1 60); do
  if grep -q "services live after" "$log"; then
    live=$(grep -o "services live after [0-9]* probes" "$log")
    break
  fi
  kill -0 "$pid" 2>/dev/null || { echo "mas-firstrun-test: the copy quit" >&2; exit 1; }
  sleep 1
done
took=$(($(date +%s) - start))
[ -n "$live" ] || { echo "mas-firstrun-test: FAIL, the service did not come alive in 60 s" >&2; exit 1; }
echo "mas-firstrun-test: $live, ${took} s after launch"

entry=$(/System/Library/CoreServices/pbs -dump 2>/dev/null | grep -A4 "NSBundleIdentifier = \"$id\"")
echo "$entry" | grep -q "default = L;" || { echo "mas-firstrun-test: FAIL, macOS lists no ⌘⇧L for the service" >&2; exit 1; }
echo "mas-firstrun-test: macOS lists \"$name\" with ⌘⇧L"

windows=$(swift - "$pid" <<'SWIFT'
import CoreGraphics
let pid = Int32(CommandLine.arguments[1])!
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
print(list.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == pid && ($0[kCGWindowLayer as String] as? Int) == 0 }.count)
SWIFT
)
[ "$windows" = 0 ] || { echo "mas-firstrun-test: FAIL, $windows window(s) opened" >&2; exit 1; }
echo "mas-firstrun-test: passed (no window opened, no relaunch needed)"
