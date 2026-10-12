#!/bin/sh
# Builds the macOS release on this Mac: the universal (Apple silicon + Intel)
# Lexpad.app and .dmg against production, checks it, runs the placement smoke
# test, and prints the .dmg's SHA-256 for the landing's downloads page.
#
#   scripts/release-mac.sh
#
# macOS is built here and not in CI because a macOS runner is billed at ten
# times a Linux minute on this private repository: one CI release build was
# about 70 billed minutes. CI builds the Windows installers on a tag (see
# README, "Release"); this script is the macOS half.
#
# What it checks, in place of CI's "install and start" (which would collide
# with the copy of Lexpad installed and running on this Mac: the second
# launch hands over to the first and exits, and a start may register its own
# path as the login item):
# - the .dmg mounts and holds Lexpad.app, and its binary has both
#   architectures;
# - the placement smoke test (`--features smoke-test`, never shipped): it
#   opens the popup all over every monitor and the panel from the icon, and
#   fails unless every window stays inside its monitor's work area. It uses
#   its own settings folder and no credential store, and runs beside an
#   installed copy.
set -eu

cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

# Production only: a release built against a local stack would ship it.
if [ -n "${LEXPAD_API_ORIGIN:-}" ] || [ -n "${LEXPAD_APP_ORIGIN:-}" ]; then
  echo "release-mac: unset LEXPAD_API_ORIGIN and LEXPAD_APP_ORIGIN; a release talks to production" >&2
  exit 1
fi

# A release is a committed state, so the .dmg says exactly what it carries.
if [ -n "$(git status --porcelain)" ]; then
  echo "release-mac: the working tree has changes; commit or stash them first" >&2
  exit 1
fi

version=$(node -p "require('./package.json').version")
commit=$(git rev-parse --short HEAD)
if ! git describe --exact-match --tags HEAD >/dev/null 2>&1; then
  echo "release-mac: note: HEAD ($commit) has no tag; the release flow tags v$version first"
fi

rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
pnpm install --frozen-lockfile

# Signing: until an Apple Developer ID exists, every release is signed with
# the same self-signed certificate, "Lexpad Local Signing", made once on this
# Mac (README, "macOS signing"). The app's designated requirement is then
# `identifier "app.lexpad.desktop" and certificate leaf = H"<that cert>"`,
# the same in every release, so macOS keeps the Accessibility grant across
# updates (an ad-hoc signature changed with every build, and the grant with
# it). The certificate's key lives only in its own keychain, unlocked here
# with the password kept in the login keychain; it never leaves this Mac.
# Gatekeeper still warns on first open: this is not notarized.
#
# TODO(signing, macOS): once the Apple Developer Team ID exists, import the
# "Developer ID Application" certificate into the login keychain and set
# APPLE_SIGNING_IDENTITY to it, and for notarization APPLE_API_KEY,
# APPLE_API_ISSUER, APPLE_API_KEY_PATH (or APPLE_ID, APPLE_PASSWORD,
# APPLE_TEAM_ID) in the environment; tauri build signs and notarizes when
# they are present. Users then remove and re-add Lexpad under Accessibility
# once more. See README, "Release".
identity="Lexpad Local Signing"
signing_keychain="$HOME/Library/Keychains/lexpad-signing.keychain-db"
if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
  [ -f "$signing_keychain" ] || {
    echo "release-mac: $signing_keychain is missing; make it once (README, \"macOS signing\")" >&2
    exit 1
  }
  keychain_password=$(security find-generic-password -a "$USER" -s "$identity keychain" -w) || {
    echo "release-mac: the signing keychain's password is not in the login keychain (README, \"macOS signing\")" >&2
    exit 1
  }
  security unlock-keychain -p "$keychain_password" "$signing_keychain"
  unset keychain_password
  case "$(security list-keychains -d user)" in
    *lexpad-signing.keychain-db*) ;;
    *) echo "release-mac: add $signing_keychain to the keychain search list (README, \"macOS signing\")" >&2; exit 1 ;;
  esac
  security find-identity -p codesigning "$signing_keychain" | grep -q "\"$identity\"" || {
    echo "release-mac: no \"$identity\" in $signing_keychain" >&2
    exit 1
  }
  export APPLE_SIGNING_IDENTITY="$identity"
fi
identifier=$(node -p "require('./src-tauri/tauri.conf.json').identifier")

echo "release-mac: building Lexpad $version ($commit), universal, production"
pnpm tauri build --target universal-apple-darwin

bundle=src-tauri/target/universal-apple-darwin/release/bundle
dmg="$bundle/dmg/Lexpad_${version}_universal.dmg"
[ -f "$dmg" ] || { echo "release-mac: $dmg was not made" >&2; exit 1; }

echo "release-mac: checking the .dmg"
mnt=$(mktemp -d)
hdiutil attach "$dmg" -nobrowse -readonly -mountpoint "$mnt" >/dev/null
trap 'hdiutil detach "$mnt" >/dev/null 2>&1 || true' EXIT
[ -d "$mnt/Lexpad.app" ] || { echo "release-mac: Lexpad.app is not in the .dmg" >&2; exit 1; }
archs=$(lipo -archs "$mnt/Lexpad.app/Contents/MacOS/lexpad-desktop")
case "$archs" in
  *x86_64*arm64* | *arm64*x86_64*) echo "release-mac: binary architectures: $archs" ;;
  *) echo "release-mac: expected x86_64 and arm64, got: $archs" >&2; exit 1 ;;
esac
# Signed with the one stable identity, deep and strict, hardened runtime.
codesign --verify --deep --strict "$mnt/Lexpad.app" || { echo "release-mac: the signature does not verify" >&2; exit 1; }
sig=$(codesign -dv --verbose=4 "$mnt/Lexpad.app" 2>&1)
echo "$sig" | grep -q "^Identifier=$identifier\$" || { echo "release-mac: not signed as $identifier" >&2; exit 1; }
echo "$sig" | grep -q "^Authority=$APPLE_SIGNING_IDENTITY\$" || { echo "release-mac: not signed by $APPLE_SIGNING_IDENTITY" >&2; exit 1; }
echo "$sig" | grep -q "flags=0x10000(runtime)" || { echo "release-mac: no hardened runtime" >&2; exit 1; }
requirement=$(codesign -d -r- "$mnt/Lexpad.app" 2>&1 | sed -n 's/^designated => //p')
case "$requirement" in
  "identifier \"$identifier\" and "*certificate*) echo "release-mac: designated requirement: $requirement" ;;
  *) echo "release-mac: unexpected designated requirement: $requirement" >&2; exit 1 ;;
esac
plist_version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$mnt/Lexpad.app/Contents/Info.plist")
[ "$plist_version" = "$version" ] || { echo "release-mac: app says $plist_version, package.json says $version" >&2; exit 1; }
hdiutil detach "$mnt" >/dev/null
trap - EXIT

# After the .dmg is made: this build writes the arm64 binary of the same
# target folder, which the universal app was assembled from.
echo "release-mac: placement smoke test"
host=$(uname -m)
[ "$host" = arm64 ] && smoke_target=aarch64-apple-darwin || smoke_target=x86_64-apple-darwin
pnpm tauri build --no-bundle --features smoke-test --target "$smoke_target"
smoke_out=$(mktemp)
set +e
"src-tauri/target/$smoke_target/release/lexpad-desktop" --smoke-test >"$smoke_out" 2>&1
code=$?
set -e
tail -n 5 "$smoke_out"
if [ "$code" -ne 0 ] || ! grep -q '^SMOKE failures 0' "$smoke_out"; then
  echo "release-mac: smoke test failed (exit $code); full output in $smoke_out" >&2
  exit 1
fi

# The smoke build replaced the arm64 half the universal app was built from;
# the .dmg itself is untouched, and its checksum is what gets published.
sha=$(shasum -a 256 "$dmg" | cut -d' ' -f1)
size=$(stat -f %z "$dmg")
echo
echo "release-mac: done"
echo "  file    $dmg"
echo "  version $version ($commit)"
echo "  bytes   $size"
echo "  sha256  $sha"
echo "  signed  $APPLE_SIGNING_IDENTITY ($requirement)"
