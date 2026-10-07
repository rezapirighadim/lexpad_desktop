#!/usr/bin/env sh
# Builds the Lexpad web app for the desktop app's main window and puts it in
# web/, with web/PROVENANCE.txt saying exactly what it was built from.
#
# web/ is committed: the build is an input of this app (like the Android
# shell's www/), and CI builds the installers from this repository alone, with
# no access to lexpad_front. Never edit web/ by hand; run this again.
#
#   scripts/build-web.sh [path to a lexpad_front checkout]   (default ../front)
#
# The build is LEXPAD_TARGET=native (relative asset paths, no service worker)
# and API-agnostic: the app injects the API address at run time
# (window.__LEXPAD_CONFIG), so the same web/ serves a local stack and
# production. No error reporting is built in (VITE_SENTRY_DSN empty): the
# window's security policy lets it talk to the app's core and nothing else.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
front=$(cd "${1:-$here/../front}" && pwd)
web="$front/apps/web"
if [ ! -f "$web/package.json" ]; then
  echo "build-web: no web app at $web" >&2
  exit 1
fi
commit=$(git -C "$front" rev-parse HEAD)
branch=$(git -C "$front" rev-parse --abbrev-ref HEAD)
dirty=$(git -C "$front" status --porcelain -- apps/web packages/core | wc -l | tr -d ' ')
if [ "$dirty" != "0" ]; then
  echo "build-web: $front has uncommitted changes in apps/web or packages/core; commit them first" >&2
  exit 1
fi
version=$(sed -n 's/.*"version": "\(.*\)".*/\1/p' "$here/package.json" | head -n1)
(
  cd "$front"
  pnpm --filter @lexpad/core build >/dev/null
  VITE_SENTRY_DSN= VITE_APP_VERSION="$version" VITE_API_URL=https://api.lexpad.app \
    pnpm --filter @lexpad/web build:native >/dev/null
)
rm -rf "$here/web"
cp -R "$web/dist-native" "$here/web"
cat > "$here/web/PROVENANCE.txt" <<INFO
The Lexpad web app, built for the desktop app's main window by scripts/build-web.sh.
Do not edit; build again.

lexpad_front commit: $commit
branch: $branch
built: $(date -u +%Y-%m-%dT%H:%M:%SZ)
target: LEXPAD_TARGET=native, no error reporting
INFO
echo "build-web: $(find "$here/web" -type f | wc -l | tr -d ' ') files in web/ from $branch@$(echo "$commit" | cut -c1-7)"
