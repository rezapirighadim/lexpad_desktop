#!/bin/sh
# The Mac App Store build's purchase bridge (src-tauri/swift/LexpadStore) against
# StoreKit's local test environment: lists the products of a test fixture
# (Tests/LexpadStoreTests/Lexpad.storekit), buys one with an account token,
# reads it as unfinished, finishes it, and finds it among the entitlements.
# No Apple Account, no network, no money, no window on screen (the host app is
# LSUIElement and its one window is never shown).
#
#   scripts/mas-storekit-test.sh
#
# Needs Xcode and XcodeGen (`brew install xcodegen`); signs the test host with
# this Mac's "Apple Development" certificate of team 6Z95MR26LZ. Builds in a
# temporary folder and removes it.
set -eu
cd "$(dirname "$0")/../src-tauri/swift/LexpadStore"
command -v xcodegen >/dev/null || { echo "mas-storekit-test: install XcodeGen (brew install xcodegen)" >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp -R Harness Sources Tests "$work/"
(cd "$work/Harness" && xcodegen generate --quiet)
xcodebuild test -project "$work/Harness/LexpadStoreHarness.xcodeproj" -scheme StoreKitHost \
  -destination 'platform=macOS' -derivedDataPath "$work/dd" -quiet 2>&1 |
  grep -v -E 'linkd.autoShortcut|Process Instance Registry' || true
xcrun xcresulttool get test-results summary --path "$work"/dd/Logs/Test/*.xcresult 2>/dev/null |
  node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const r=JSON.parse(s);console.log(`mas-storekit-test: ${r.result}, ${r.passedTests} passed, ${r.failedTests} failed`);process.exit(r.result==="Passed"?0:1)})'
