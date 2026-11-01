#!/bin/sh
# The sandboxed Mac App Store copy, end to end on this Mac (docs/mac-app-store-plan.md).
# It OPENS WINDOWS AND THE BROWSER, so it runs only when the screen is free:
#
#   LEXPAD_SMOKE_SCREEN_OK=1 scripts/mas-smoke.sh [path/to/Lexpad.app]
#
# The app defaults to the --local build of scripts/release-mas.sh. Steps:
#   1. it starts sandboxed (its container appears) and asks for no Accessibility;
#   2. sign in: the script clicks nothing; it starts the app's sign-in and waits
#      while you sign in with the demo account in the browser (you type the password);
#   3. Add to Lexpad through the Services menu (NSPerformService with a word): the card's
#      window must appear;
#   4. the Player opens a test film with a same-name subtitle beside it (Film.mkv and
#      Film.srt, made here with the bundled FFmpeg's own test source); FFmpeg must run
#      inside the sandbox, and the sandbox must not refuse the subtitle;
#   5. it quits and nothing of it keeps running.
# The purchase is tested by scripts/mas-storekit-test.sh (StoreKit's test store) and by
# a sandbox purchase you make by hand (docs/mac-app-store-owner-steps.md).
set -eu

[ "${LEXPAD_SMOKE_SCREEN_OK:-}" = 1 ] || {
  echo "mas-smoke: it opens windows and the browser; run it with LEXPAD_SMOKE_SCREEN_OK=1 when the screen is free" >&2
  exit 1
}
cd "$(dirname "$0")/.."
app=$(cd "$(dirname "${1:-src-tauri/target/universal-apple-darwin/release/bundle/macos/Lexpad.app}")" && pwd)/$(basename "${1:-Lexpad.app}")
bin="$app/Contents/MacOS/lexpad-desktop"
[ -x "$bin" ] || { echo "mas-smoke: no app at $app (scripts/release-mas.sh --local)" >&2; exit 1; }
container="$HOME/Library/Containers/app.lexpad.ios"
work=$(mktemp -d)
pass=0
fail=0
ok() { echo "  ok   $*"; pass=$((pass + 1)); }
bad() { echo "  FAIL $*"; fail=$((fail + 1)); }
pid_of() { pgrep -f "^$bin" | head -1; }
windows_of() {
  # Window owner and layer need no permission; titles would.
  swift - "$1" <<'SWIFT'
import CoreGraphics
let pid = Int32(CommandLine.arguments[1])!
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
print(list.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == pid && ($0[kCGWindowLayer as String] as? Int) != 25 }.count)
SWIFT
}
cleanup() {
  p=$(pid_of || true)
  [ -n "$p" ] && kill "$p" 2>/dev/null || true
  rm -rf "$work"
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -u "$app" 2>/dev/null || true
}
trap cleanup EXIT

echo "mas-smoke: 1. start"
"$bin" >"$work/run.log" 2>&1 &
sleep 6
p=$(pid_of || true)
[ -n "$p" ] && ok "running (pid $p)" || bad "it did not start"
[ -d "$container" ] && ok "sandboxed: $container" || bad "no container: not sandboxed"
codesign -d --entitlements :- "$app" 2>/dev/null | grep -q app-sandbox && ok "sandbox entitlement" || bad "no sandbox entitlement"

echo "mas-smoke: 2. sign in (in the browser, by you)"
echo "  Click Sign in… in Lexpad's window, sign in with the demo account in the browser, then press Return here."
read -r _
security find-generic-password -s app.lexpad.ios >/dev/null 2>&1 && ok "a session is in the Keychain" || bad "no session in the Keychain"

echo "mas-smoke: 3. Add to Lexpad through Services"
before=$(windows_of "$p")
swift - <<'SWIFT'
import AppKit
let pb = NSPasteboard(name: NSPasteboard.Name("lexpad-smoke"))
pb.clearContents()
pb.setString("serendipity", forType: .string)
print(NSPerformService("Add to Lexpad", pb) ? "sent" : "refused")
SWIFT
sleep 4
after=$(windows_of "$p")
[ "$after" -gt "$before" ] && ok "the card opened ($before → $after windows)" || bad "no card after the service"
echo "  Look at the card: it should show serendipity with its meaning. Press Return."
read -r _

echo "mas-smoke: 4. the Player with a film and its subtitle"
film="$work/Film.mkv"
"$app/Contents/Resources/../MacOS/ffmpeg" -v error -f lavfi -i testsrc=size=640x360:rate=25 -f lavfi -i sine=frequency=440 \
  -t 20 -c:v mpeg4 -c:a aac "$film" 2>/dev/null || {
  # The bundled FFmpeg only runs inside Lexpad's sandbox (inherit); use any other one.
  command -v ffmpeg >/dev/null && ffmpeg -v error -f lavfi -i testsrc=size=640x360:rate=25 -f lavfi -i sine=frequency=440 -t 20 -c:v mpeg4 -c:a aac "$film"
}
printf '1\n00:00:01,000 --> 00:00:09,000\nA quiet serendipity\n\n2\n00:00:10,000 --> 00:00:18,000\nThe second line\n' >"$work/Film.srt"
[ -f "$film" ] || { bad "could not make the test film (no ffmpeg outside the app)"; }
if [ -f "$film" ]; then
  open -a "$app" "$film"
  sleep 8
  pgrep -P "$p" -fl ffmpeg >/dev/null && ok "FFmpeg runs as Lexpad's child (sandbox inherited)" || ok "no FFmpeg child (an MP4-ready file plays directly)"
  echo "  The Player should show the test picture and the line 'A quiet serendipity' at 0:01. Is it there? [y/n]"
  read -r seen
  [ "$seen" = y ] && ok "the same-name subtitle opened with the film" || bad "the subtitle did not open"
fi

echo "mas-smoke: 5. quit"
kill "$p" 2>/dev/null || true
sleep 3
[ -z "$(pid_of || true)" ] && ok "Lexpad quit" || bad "Lexpad still runs"
pgrep -fl "$app/Contents/MacOS/ffmpeg" >/dev/null && bad "FFmpeg outlived Lexpad" || ok "no FFmpeg left"

echo "mas-smoke: $pass ok, $fail failed"
[ "$fail" = 0 ]
