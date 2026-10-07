#!/bin/sh
# The end-to-end test of Lexpad's window: the Rust half
# (src-tauri/src/e2e_main.rs) and the browser half (scripts/e2e-main.mjs) side
# by side, against a local API and web app. Never production: both refuse
# anything but localhost.
#
#   LEXPAD_E2E_API=http://localhost:8091 LEXPAD_E2E_APP=http://localhost:4173 \
#   LEXPAD_E2E_ACCOUNT=../intro-video/demo/local-account.json \
#   LEXPAD_E2E_PLAYWRIGHT=../front/node_modules/playwright/index.mjs scripts/e2e-main.sh
set -e
: "${LEXPAD_E2E_API:?}" "${LEXPAD_E2E_APP:?}" "${LEXPAD_E2E_ACCOUNT:?}" "${LEXPAD_E2E_PLAYWRIGHT:?}"
LEXPAD_E2E_DIR=$(mktemp -d)
export LEXPAD_E2E_API LEXPAD_E2E_APP LEXPAD_E2E_DIR
pnpm build >/dev/null
mkdir -p docs/e2e
node scripts/e2e-main.mjs "$LEXPAD_E2E_DIR" "$LEXPAD_E2E_ACCOUNT" "$LEXPAD_E2E_PLAYWRIGHT" "$LEXPAD_E2E_API" &
driver=$!
(cd src-tauri && cargo test --lib e2e_main -- --ignored --nocapture)
wait $driver
cp "$LEXPAD_E2E_DIR"/requests.json "$LEXPAD_E2E_DIR"/main-log.txt docs/e2e/ 2>/dev/null || true
echo "end-to-end (window): passed"
