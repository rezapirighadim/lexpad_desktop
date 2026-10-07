#!/bin/sh
# The local end-to-end test: the Rust half (src-tauri/src/e2e.rs) and the
# browser half (scripts/e2e-driver.mjs) side by side, against a local API and
# web app. Never production: both refuse anything but localhost.
#
#   LEXPAD_E2E_API=http://localhost:8091 LEXPAD_E2E_APP=http://localhost:4173 \
#   LEXPAD_E2E_ACCOUNT=../intro-video/demo/local-account.json \
#   LEXPAD_E2E_PLAYWRIGHT=../front/node_modules/playwright/index.mjs scripts/e2e-local.sh
set -e
: "${LEXPAD_E2E_API:?}" "${LEXPAD_E2E_APP:?}" "${LEXPAD_E2E_ACCOUNT:?}" "${LEXPAD_E2E_PLAYWRIGHT:?}"
LEXPAD_E2E_DIR=$(mktemp -d)
export LEXPAD_E2E_API LEXPAD_E2E_APP LEXPAD_E2E_DIR
pnpm build >/dev/null
mkdir -p docs/e2e
node scripts/e2e-driver.mjs "$LEXPAD_E2E_DIR" "$LEXPAD_E2E_ACCOUNT" "$LEXPAD_E2E_PLAYWRIGHT" &
driver=$!
(cd src-tauri && cargo test --lib e2e -- --ignored --nocapture)
wait $driver
cp "$LEXPAD_E2E_DIR"/word.json "$LEXPAD_E2E_DIR"/session.json docs/e2e/ 2>/dev/null || true
echo "end-to-end: passed"
