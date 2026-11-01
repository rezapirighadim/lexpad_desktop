#!/bin/sh
# Builds the Mac App Store copy of Lexpad (docs/mac-app-store-plan.md): the same
# code as the site's dmg with `--features mas` and src-tauri/tauri.mas.conf.json
# (sandboxed, bundle id app.lexpad.ios for Universal Purchase with the iPhone
# app, Apple's in-app purchase, the Services key for ⌘⇧L, no update line).
#
#   scripts/release-mas.sh            the App Store package: Lexpad.app signed
#                                     "Apple Distribution", embedded profile,
#                                     Lexpad-<version>.pkg signed "3rd Party Mac
#                                     Developer Installer", validated with Apple
#   LEXPAD_MAS_UPLOAD=1 scripts/release-mas.sh
#                                     ... and uploads it to App Store Connect
#   scripts/release-mas.sh --local    a sandboxed test copy for this Mac only:
#                                     signed "Apple Development", no profile, no
#                                     package, no Apple calls (scripts/mas-smoke.sh)
#   LEXPAD_MAS_BUILD=<n>              CFBundleVersion (default: the version);
#                                     every upload of one version needs a new one
#
# Needs (App Store mode; docs/mac-app-store-owner-steps.md says how each is made):
#   - "Apple Distribution: Reza Pirighadim (6Z95MR26LZ)" and
#     "3rd Party Mac Developer Installer: Reza Pirighadim (6Z95MR26LZ)" in
#     ~/Library/Keychains/lexpad-signing.keychain-db;
#   - the Mac App Store provisioning profile for app.lexpad.ios at
#     ~/.appstoreconnect/profiles/Lexpad_Mac_App_Store.provisionprofile;
#   - the App Store Connect API key (~/.appstoreconnect/lexpad.env).
# None of these is ever committed: this repository is public.
#
# The site's dmg is untouched: it is still scripts/release-mac.sh. The tauri CLI
# rewrites Cargo.toml's tauri features for macOSPrivateApi=false; this script
# puts Cargo.toml and Cargo.lock back afterwards, whatever happens.
set -eu

cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

local_build=0
[ "${1:-}" = "--local" ] && local_build=1

if [ -n "${LEXPAD_API_ORIGIN:-}" ] || [ -n "${LEXPAD_APP_ORIGIN:-}" ]; then
  echo "release-mas: unset LEXPAD_API_ORIGIN and LEXPAD_APP_ORIGIN; the App Store copy talks to production" >&2
  exit 1
fi
if [ "$local_build" = 0 ] && [ -n "$(git status --porcelain)" ]; then
  echo "release-mas: the working tree has changes; commit or stash them first" >&2
  exit 1
fi

version=$(node -p "require('./package.json').version")
build=${LEXPAD_MAS_BUILD:-$version}
commit=$(git rev-parse --short HEAD)
team=6Z95MR26LZ
bundle_id=app.lexpad.ios
app_identity="Apple Distribution: Reza Pirighadim ($team)"
installer_identity="3rd Party Mac Developer Installer: Reza Pirighadim ($team)"
profile="$HOME/.appstoreconnect/profiles/Lexpad_Mac_App_Store.provisionprofile"
signing_keychain="$HOME/Library/Keychains/lexpad-signing.keychain-db"

if [ "$local_build" = 1 ]; then
  app_identity=$(security find-identity -v -p codesigning | sed -n "s/.*\"\(Apple Development: [^\"]*\)\".*/\1/p" | head -1)
  [ -n "$app_identity" ] || { echo "release-mas: no Apple Development certificate on this Mac" >&2; exit 1; }
else
  [ -f "$profile" ] || { echo "release-mas: no provisioning profile at $profile (docs/mac-app-store-owner-steps.md)" >&2; exit 1; }
  if [ -f "$signing_keychain" ]; then
    pw=$(security find-generic-password -a "$USER" -s "Lexpad Local Signing keychain" -w 2>/dev/null || true)
    [ -n "$pw" ] && security unlock-keychain -p "$pw" "$signing_keychain"
    unset pw
  fi
  for id in "$app_identity"; do
    security find-identity -v -p codesigning | grep -q "\"$id\"" || {
      echo "release-mas: \"$id\" is not in a keychain (docs/mac-app-store-owner-steps.md)" >&2
      exit 1
    }
  done
  security find-identity -v | grep -q "\"$installer_identity\"" || {
    echo "release-mas: \"$installer_identity\" is not in a keychain (docs/mac-app-store-owner-steps.md)" >&2
    exit 1
  }
  if [ -z "${ASC_KEY_ID:-}" ] && [ -f "$HOME/.appstoreconnect/lexpad.env" ]; then
    set -a
    # shellcheck disable=SC1091
    . "$HOME/.appstoreconnect/lexpad.env"
    set +a
  fi
  : "${ASC_KEY_ID:?release-mas: set ASC_KEY_ID (or ~/.appstoreconnect/lexpad.env)}"
  : "${ASC_ISSUER_ID:?release-mas: set ASC_ISSUER_ID}"
fi

for f in src-tauri/binaries/ffmpeg-universal-apple-darwin src-tauri/binaries/ffprobe-universal-apple-darwin; do
  [ -f "$f" ] || { echo "release-mas: $f is missing (scripts/ffmpeg/build-macos.sh)" >&2; exit 1; }
done

rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
pnpm install --frozen-lockfile

cp src-tauri/Cargo.toml src-tauri/Cargo.toml.mas-keep
cp src-tauri/Cargo.lock src-tauri/Cargo.lock.mas-keep
restore() {
  mv -f src-tauri/Cargo.toml.mas-keep src-tauri/Cargo.toml
  mv -f src-tauri/Cargo.lock.mas-keep src-tauri/Cargo.lock
}
trap restore EXIT

extra="{\"bundle\":{\"macOS\":{\"bundleVersion\":\"$build\""
if [ "$local_build" = 0 ]; then
  extra="$extra,\"files\":{\"embedded.provisionprofile\":\"$profile\"}"
fi
extra="$extra}}}"
unset APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH APPLE_ID APPLE_PASSWORD APPLE_SIGNING_IDENTITY
echo "release-mas: building Lexpad $version ($build, $commit) for the Mac App Store, universal"
pnpm tauri build --target universal-apple-darwin --bundles app --no-sign --features mas \
  --config src-tauri/tauri.mas.conf.json --config "$extra"
restore
trap - EXIT

app=src-tauri/target/universal-apple-darwin/release/bundle/macos/Lexpad.app
[ -d "$app" ] || { echo "release-mas: $app was not made" >&2; exit 1; }

entitlements=src-tauri/mas/Lexpad.entitlements
# codesign's parser is strict where plutil is not (a `--` in a comment).
xmllint --noout src-tauri/mas/Lexpad.entitlements src-tauri/mas/ffmpeg.entitlements
if [ "$local_build" = 1 ]; then
  # Only a provisioning profile may grant the identity keys; a local copy
  # without one must not ask for them, or macOS refuses to start it.
  entitlements=$(mktemp -t lexpad-mas).plist
  cp src-tauri/mas/Lexpad.entitlements "$entitlements"
  /usr/libexec/PlistBuddy -c 'Delete :com.apple.application-identifier' -c 'Delete :com.apple.developer.team-identifier' "$entitlements"
fi

# Everything in the app readable by everyone (App Store Connect error 90255):
# the profile is kept private on this Mac (0600) and is copied in as it is.
chmod -R u+rwX,go+rX "$app"
echo "release-mas: signing with $app_identity"
# Inside out: the helpers first (sandbox + inherit only), then the app.
for tool in "$app/Contents/MacOS/ffmpeg" "$app/Contents/MacOS/ffprobe"; do
  codesign --force --sign "$app_identity" --timestamp=none --options runtime \
    --entitlements src-tauri/mas/ffmpeg.entitlements "$tool"
done
codesign --force --sign "$app_identity" --options runtime --entitlements "$entitlements" "$app"

echo "release-mas: checking"
codesign --verify --deep --strict "$app"
sig=$(codesign -dv --verbose=4 "$app" 2>&1)
echo "$sig" | grep -q "^Identifier=$bundle_id\$" || { echo "release-mas: not signed as $bundle_id" >&2; exit 1; }
codesign -d --entitlements :- "$app" 2>/dev/null | grep -q "com.apple.security.app-sandbox" || {
  echo "release-mas: the app is not sandboxed" >&2
  exit 1
}
for tool in ffmpeg ffprobe; do
  ents=$(codesign -d --entitlements :- "$app/Contents/MacOS/$tool" 2>/dev/null)
  echo "$ents" | grep -q "com.apple.security.inherit" || { echo "release-mas: $tool does not inherit the sandbox" >&2; exit 1; }
done
archs=$(lipo -archs "$app/Contents/MacOS/lexpad-desktop")
case "$archs" in
  *x86_64*arm64* | *arm64*x86_64*) ;;
  *) echo "release-mas: expected x86_64 and arm64, got: $archs" >&2; exit 1 ;;
esac
plist="$app/Contents/Info.plist"
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$plist")" = "$bundle_id" ]
[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$plist")" = "$version" ]
[ "$(/usr/libexec/PlistBuddy -c 'Print :LSApplicationCategoryType' "$plist")" = "public.app-category.education" ]
[ "$(/usr/libexec/PlistBuddy -c 'Print :NSServices:0:NSKeyEquivalent:default' "$plist")" = "L" ]
[ "$(/usr/libexec/PlistBuddy -c 'Print :ITSAppUsesNonExemptEncryption' "$plist")" = "false" ]
plutil -lint "$app/Contents/Resources/PrivacyInfo.xcprivacy" >/dev/null || { echo "release-mas: no privacy manifest" >&2; exit 1; }
# The core must be the App Store one (the core's own config::MAS).
strings "$app/Contents/MacOS/lexpad-desktop" | grep -q "This copy of Lexpad does not sell" && {
  echo "release-mas: the binary is not the mas build" >&2
  exit 1
}
[ -z "$(find "$app" ! -perm -o=r -print)" ] || { echo "release-mas: files in the app others cannot read" >&2; exit 1; }
echo "release-mas: Lexpad.app is sandboxed, signed and checked ($archs)"

if [ "$local_build" = 1 ]; then
  rm -f "$entitlements"
  # A copy LaunchServices knows could answer "Open with" for the real one.
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -u "$PWD/$app" 2>/dev/null || true
  echo "  app     $app (local test copy; never upload it; delete it when done)"
  exit 0
fi

out=src-tauri/target/universal-apple-darwin/release/bundle/mas
mkdir -p "$out"
pkg="$out/Lexpad-$version-$build.pkg"
productbuild --component "$app" /Applications --sign "$installer_identity" "$pkg"
echo "release-mas: validating $pkg with App Store Connect"
xcrun altool --validate-app -f "$pkg" -t macos --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
if [ "${LEXPAD_MAS_UPLOAD:-}" = 1 ]; then
  echo "release-mas: uploading"
  xcrun altool --upload-app -f "$pkg" -t macos --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
fi
echo
echo "release-mas: done"
echo "  package $pkg"
echo "  version $version ($build, $commit)"
echo "  sha256  $(shasum -a 256 "$pkg" | cut -d' ' -f1)"
