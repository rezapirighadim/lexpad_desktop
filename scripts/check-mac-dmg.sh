#!/bin/sh
# Checks a Mac .dmg the way Gatekeeper will on a learner's Mac, and fails
# loudly on anything that would make macOS ask for "Open Anyway":
#
#   - the .dmg: Developer ID signature, notarized, ticket stapled;
#   - Lexpad.app inside it: a deep and strict signature, notarized, ticket
#     stapled to the app itself (once the app is copied to Applications it
#     is checked on its own, and without a stapled ticket macOS must reach
#     Apple online; when it cannot, the app is blocked);
#   - every Mach-O file in the app (the core, FFmpeg, FFprobe, any helper,
#     framework or library): Developer ID of team 6Z95MR26LZ, hardened
#     runtime, secure timestamp;
#   - Apple's own pre-distribution check (syspolicy_check), when this macOS
#     has it.
#
#   scripts/check-mac-dmg.sh <file.dmg>
#
# Used by scripts/release-mac.sh on every notarized build, and to check a
# published download. Read-only: mounts the image read-only.
set -eu

dmg=${1:?usage: check-mac-dmg.sh <file.dmg>}
team=6Z95MR26LZ
fail=0
bad() { echo "check-mac-dmg: FAIL $*" >&2; fail=1; }
ok() { echo "check-mac-dmg: ok   $*"; }

[ -f "$dmg" ] || { echo "check-mac-dmg: no file $dmg" >&2; exit 1; }

# The .dmg itself.
out=$(spctl -a -vvv -t install "$dmg" 2>&1 || true)
echo "$out" | grep -q "accepted" && echo "$out" | grep -q "source=Notarized Developer ID" &&
  ok "dmg accepted by Gatekeeper as Notarized Developer ID" || bad "dmg: spctl says: $out"
xcrun stapler validate "$dmg" >/dev/null 2>&1 && ok "dmg has its ticket stapled" || bad "dmg: no stapled ticket"
codesign -dv "$dmg" 2>&1 | grep -q "TeamIdentifier=$team" && ok "dmg signed by team $team" || bad "dmg: not signed by team $team"

mnt=$(mktemp -d)
hdiutil attach "$dmg" -nobrowse -readonly -noautoopen -mountpoint "$mnt" >/dev/null
trap 'hdiutil detach "$mnt" >/dev/null 2>&1 || true; rmdir "$mnt" 2>/dev/null || true' EXIT
app=$(find "$mnt" -maxdepth 1 -name '*.app' | head -n 1)
[ -n "$app" ] || { echo "check-mac-dmg: FAIL no .app in the dmg" >&2; exit 1; }

# The app.
out=$(spctl -a -vvv "$app" 2>&1 || true)
echo "$out" | grep -q "accepted" && echo "$out" | grep -q "source=Notarized Developer ID" &&
  ok "app accepted by Gatekeeper as Notarized Developer ID" || bad "app: spctl says: $out"
xcrun stapler validate "$app" >/dev/null 2>&1 && ok "app has its own ticket stapled" ||
  bad "app: no ticket stapled to $(basename "$app") (macOS must reach Apple online at first open)"
codesign --verify --deep --strict --verbose=4 "$app" >/dev/null 2>&1 && ok "app signature verifies (deep, strict)" ||
  bad "app: codesign --verify --deep --strict fails: $(codesign --verify --deep --strict --verbose=4 "$app" 2>&1 | tail -n 2)"

# Every Mach-O file inside the app.
count=0
for f in $(find "$app" -type f); do
  file -b "$f" | grep -q "Mach-O" || continue
  count=$((count + 1))
  sig=$(codesign -dv --verbose=4 "$f" 2>&1 || true)
  name=${f#"$app"/}
  echo "$sig" | grep -q "^Authority=Developer ID Application: .*($team)\$" || { bad "$name: not signed with the Developer ID"; continue; }
  echo "$sig" | grep -q "^TeamIdentifier=$team\$" || bad "$name: team is not $team"
  echo "$sig" | grep -Eq "flags=0x[0-9a-f]*\(.*runtime" || bad "$name: no hardened runtime"
  echo "$sig" | grep -q "^Timestamp=" || bad "$name: no secure timestamp"
done
[ "$count" -gt 0 ] && ok "$count Mach-O files: Developer ID, hardened runtime, timestamp" || bad "no Mach-O file found in the app"

# Apple's own pre-distribution check (macOS 14 and later).
if command -v syspolicy_check >/dev/null 2>&1; then
  out=$(syspolicy_check distribution "$app" 2>&1 || true)
  echo "$out" | grep -q "passed all pre-distribution checks" && ok "syspolicy_check distribution passes" ||
    bad "syspolicy_check distribution: $(echo "$out" | grep -E '^[A-Z][A-Za-z ]+$' | head -n 3 | tr '\n' ' ')"
fi

if [ "$fail" = 1 ]; then
  echo "check-mac-dmg: $dmg would make macOS warn or block on a learner's Mac" >&2
  exit 1
fi
echo "check-mac-dmg: $dmg passes every Gatekeeper check"
