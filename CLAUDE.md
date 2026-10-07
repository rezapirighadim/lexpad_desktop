# lexpad_desktop — conventions

The desktop app for Lexpad, on macOS and Windows: select a word in **any** app, press the
shortcut (⌘⇧L / Ctrl+Shift+L), and a small card shows its meaning while it goes into your
notebook with the sentence you met it in. It is the browser extension's card for the whole
computer. Since 0.2 it is also **the whole Lexpad app** in a window of its own (Today, practice,
the notebook, Lex, Progress, Settings), working offline. Tauri 2: a Rust core (`src-tauri/`),
three small TypeScript pages (`src/`) and the web app's own build (`web/`). It is a client of the
same API as the web app (`lexpad_back`); the connect page and the window's app live in the web app
(`lexpad_front`), and the extension is `lexpad_extension`.

## Three rules that come before everything else

These hold in every repository of this project. Nothing below them overrides
them, and no deadline is a reason to bend one.

- **Never hard-code a value that belongs in data.** Prices, allowances,
  limits, switches, dates, addresses and keys come from the database, a
  feature flag, the API contract or the environment. A number that genuinely
  has to live in code is a named constant with a comment saying why it cannot
  live anywhere else. A literal dropped into a screen, a handler or a query
  to make something work today is a bug shipped on purpose.
- **Never show anyone fake data.** Sample values, fixtures and stubs exist
  only in test files and are imported only by tests; they are never bundled,
  never returned by a screen or a handler, and never used as a fallback when
  a real call fails. When a call fails, the screen says it failed. Something
  that looks like real data but is not is worse than an error message.
- **Never break what already works.** Do not disable, bypass, comment out or
  quietly weaken existing behaviour to make a new change fit, and do not
  leave a repository in a state where its own gate does not pass. If
  something in the way is genuinely wrong, say so and fix it properly in the
  same change. Every change: builds, lint clean, tests pass, generated code
  regenerated, docs updated.

## Layout

```
src/popup.html, src/popup/       the add-a-word popup (view.ts is the card; tested with happy-dom)
src/panel.html, src/panel/       the menu-bar / tray panel (tested with happy-dom)
src/settings.html, src/settings/ the Settings window
src/styles/tokens.css            the web app's tokens (hex) and faces (src/assets/fonts), for the panel
src/lib/text.ts, script.ts       copied from lexpad_extension; keep them identical
src/lib/compose.ts               the word as saved, and what a capture turns into (tested)
src/lib/backend.ts               the only bridge to the core: typed `invoke` wrappers
src-tauri/src/lib.rs             wiring: shortcut, windows, plugins
src-tauri/src/tray.rs            the menu-bar / tray icon (1x+2x template on macOS, tray.ico frame on Windows), right-click menu
src-tauri/src/panel.rs           the panel window: placing it from the icon's rect on any monitor (tested), showing, hiding
src-tauri/src/capture/           reading the selection: macos.rs (AX, then ⌘C), windows.rs (UIA, then Ctrl+C)
src-tauri/src/services_macos.rs  "Add to Lexpad" in the macOS Services menu
src-tauri/src/api.rs             the API client: the only holder of tokens (tested with a local server)
src-tauri/src/auth.rs            RFC 8252 sign-in: PKCE, state, loopback listener (tested)
src-tauri/src/store.rs           the session in the Keychain / Credential Manager (keyring crate)
src-tauri/src/commands.rs        everything a window may ask; capabilities/*.json say which window may ask what
src-tauri/src/popup.rs           showing, placing and hiding the popup; it grows inside the work area
src-tauri/src/placement.rs       where windows go: the work area of the monitor under the pointer, units per platform (tested)
src-tauri/src/smoke.rs           CI's placement smoke test, only with `--features smoke-test` (never shipped)
src-tauri/src/main_window.rs     Lexpad's window: the web app from web/, its config, navigation lock, place (tested)
src-tauri/src/proxy.rs           what the window may send to the API through the core (tested)
src-tauri/src/notify/            notifications: the reminder plan, the inbox poll and its rules (tested), macos.rs, windows.rs
web/                             the web app built for the window by scripts/build-web.sh (committed; never edit; PROVENANCE.txt)
src-tauri/Info.plist             LSUIElement (no Dock icon) and the NSServices entry
src-tauri/src/e2e.rs             the local end-to-end test (ignored by default), with scripts/e2e-local.sh
src-tauri/src/e2e_main.rs        the window's end-to-end test (ignored), with scripts/e2e-main.{sh,mjs}
docs/e2e/                        the last end-to-end run: what ran, screenshots, evidence
docs/screenshots/                the panel in light and dark, and the tray icons (scripts/panel-screenshots.mjs, icons.py --preview)
scripts/icons.py                 every icon, drawn from landing/site/assets/favicon-v2.svg (`pnpm icons`)
docs/windows-manual-test.md      the Windows checklist, since Windows cannot run here
```

## Lexpad's window (0.2): the design

- **The web app, bundled, never loaded from app.lexpad.app.** Like the Android shell, the app
  ships a build of `lexpad_front` (`LEXPAD_TARGET=native`, made by `scripts/build-web.sh` into
  `web/`, copied beside the small pages by `vite.config.ts`). It runs offline from its own
  IndexedDB (reads local first, writes to the outbox, sync when online), and a release says
  exactly which front commit it carries (`web/PROVENANCE.txt`). The web app picks its desktop
  platform (`lib/platform/desktop.ts` in front) when the core has injected
  `window.__LEXPAD_CONFIG` (platform `desktop`) and Tauri's IPC is there.
- **The core stays the only holder of tokens.** The window never gets a session of its own: every
  API call goes to the core (`api_fetch`), which checks it (`proxy.rs`: the API's own origin under
  `/api/v1/`, the client's headers only, no sign-in, refresh, password, other-session or assistant
  calls) and sends it with the desktop app's own delegated session from the credential store,
  refreshing it single-flight, streaming the body back over a channel. No server change was
  needed: the delegated session may do everything the app does day to day, and what it may not
  (a password, linking Google or Apple, connecting another client or an assistant) the window
  opens in the browser (`open_in_browser`, a checked same-origin path).
- **Signing in is the same RFC 8252 connect**: the window's sign-in screen is one button that
  runs `connect` (system browser, loopback, PKCE). No password is ever typed in the window, and
  Google and Apple accounts work, since their sign-in happens in the real browser.
- **The window may only show the app's own pages** (`is_app_page`); any other address goes to the
  system browser (https and mailto only, `may_open_outside`), never into the window. Files the
  page saves go to Downloads.
- **Opening it**: Open Lexpad (panel, tray menu), a click on the Dock icon, a second launch, a
  word from the panel (`/words/<id>`, also before the page listens: `main_take_pending`), a
  clicked notification, and at start when the learner asks (Settings). "Open Lexpad in my
  browser instead" sends all of those to the browser. The window is made when opened and
  destroyed when closed; its place is remembered and brought back inside the work area.
- **Settings live in the app's Settings**: "This computer" (`/settings/desktop` in the web app,
  shown only on the desktop platform) holds the shortcut, the quick-add notebook, Accessibility,
  notifications, start-up and the version, through the core's commands. The small Settings
  window stays as the fallback for a signed-out app and the first run, and for someone who
  prefers the browser; tray "Settings…" opens the window's page when signed in.
- **Notifications** (`notify/`): the daily reminder's plan is the web app's own
  (`sync/reminder.ts`, real counts only), handed to the core (`schedule_reminders`) and to the
  system (UNUserNotificationCenter / scheduled toasts) so it arrives with the window shut. The
  server's messages (gifts, announcements) cannot come through Firebase here, so the core reads
  the inbox (`GET /notifications`) at start and every 30 minutes and pops up only what `pick`
  lets through (see its doc: unread, fresh, not come-back, product news only when allowed, not
  already popped elsewhere when the API says so). A click opens the window.
- **Accessibility after an update**: an unsigned build gets a new code signature each release,
  and macOS stops applying the old grant while the switch still looks on. The core remembers
  the version that last had the permission (`accessibility_granted_in`) and says so plainly
  (`permissionStale`) instead of "not allowed yet".

## Hard rules

- **The core is the only part that touches tokens or the API.** The windows call commands and
  render answers; a command never returns a token. A new capability is a new command in
  `commands.rs`, listed in `build.rs` and granted in the window's capability file; nothing else
  is callable. No `fetch` from a window, and the CSP's `connect-src` is IPC only.
- **The app never sees a password.** Signing in is RFC 8252: the system browser, a loopback
  redirect on `127.0.0.1:<random port>/callback`, PKCE S256 and a random state (`auth.rs`). The
  web app's `/connect-desktop` asks the API for a one-time code bound to the challenge and the
  address (`POST /me/desktop-codes`) and redirects; the app checks the state in constant time and
  trades code + verifier at `POST /auth/desktop/token` for a delegated session of platform
  `desktop`. That session cannot connect other clients, change the password or allow an
  assistant, is listed under Signed-in devices as "Desktop app", and is revoked on sign-out.
- **Tokens live in the operating system's credential store**, one entry per API origin (so a
  development build never touches the production session): the refresh token and who it
  belongs to. The access token is memory only. Refresh is single-flight (the session mutex in
  `api.rs`); a refused refresh forgets the session.
- **The selection is read only when the shortcut is pressed, and only the selection.**
  Accessibility first (macOS `AXSelectedText`, Windows UIA TextPattern), then the clipboard:
  save every item and type, send ⌘C / Ctrl+C after the shortcut's own modifiers are released,
  read, put back exactly; if nothing was copied the clipboard was never touched. A password
  field is never read. The text around the selection is used only to find the one sentence the
  word was in; nothing else of it leaves the device.
- **"Seen in <App name>", never a window title.** The private note is the app's display name
  ("Seen in Microsoft Word"): window titles carry document names, e-mail subjects and the people
  in a chat, and the note reaches exports and shared notebooks. `seenIn` strips control
  characters and keeps it short. The sentence is kept as the first example only when the
  accessibility interface gave the text around it; the clipboard path gives no sentence, and
  nothing is ever made up to fill one.
- **The card is the extension's card.** Same copy, same states (looking up, meanings, "already in
  your notebook", offline, "not available right now" with no reason or number), same routing by
  script (`script.ts`), same `compose`. A change to the card's behaviour lands in both repos.
- **Learners bring their own words.** The app adds only what the learner selected or typed. AI
  enriches that word; nothing suggests words or fills a notebook.
- **Limits are enforced, never announced.**
- **Permissions stay minimal.** Capabilities grant the event listener and our own commands per
  window, nothing else: no shell, no fs, no http from the windows. The only addresses the app
  opens are on `APP_ORIGIN`: the connect page, the web app's home and a word's page
  (`/words/<id>`, the id checked as an API id), a checked same-origin path for Lexpad's window,
  and the system's Accessibility and Notifications settings, all built in Rust. A small window
  passes at most an id, never an address; Lexpad's window may hand the system browser an https
  or mailto link the learner clicked, nothing else. No remote
  code; the updater is off (TODO before 1.0).
- **Every window stays inside the work area** of its monitor: never under the menu bar, the Dock
  or a taskbar on any edge (`placement.rs`, from `Monitor::work_area`, which is
  `NSScreen.visibleFrame` / `GetMonitorInfoW` `rcWork`). A window taller than the work area is
  cut to it and its page scrolls; the card keeps its header and its buttons in view (sticky).
  Placement works in global points on macOS and physical pixels on Windows; never mix them.
  The card is dragged by its header (`data-tauri-drag-region="deep"`, the window's only
  `core:window` permission) and nothing about where it was is remembered.
- **No Dock icon while idle.** `LSUIElement` and the Accessory activation policy; only
  Lexpad's window and the Settings window bring a Dock icon while open (`refresh_dock`). The
  panel and the card never do.
- **Recently added words are this computer's own record** (`settings.json`, at most five, each
  tagged with the account): the panel lists them; signing out forgets that account's. Nothing is
  suggested and nothing is fetched to fill the list.
- **Development builds never register themselves to start at login** (`config::is_development_build`:
  debug, or built against a non-production API).

## Build and release

- `pnpm check` (prettier, tsc, vitest, vite build, rustfmt, clippy `-D warnings`, cargo test) is
  the pre-commit gate and CI. Enable the hooks with `git config core.hooksPath .githooks`.
- CI also installs and starts the built app on both systems, and runs the placement smoke test
  (`pnpm tauri build --no-bundle --features smoke-test`, then `lexpad-desktop --smoke-test`; it
  prints every rectangle and exits 1 if one leaves its work area). Run it locally the same way;
  it does not touch the credential store, the shortcut or the login items, and runs beside an
  installed copy.
- `pnpm app:dev` runs it against production; for a local stack build with
  `LEXPAD_API_ORIGIN=http://localhost:8091 LEXPAD_APP_ORIGIN=http://localhost:4173 pnpm tauri build --bundles app`
  (the API's CORS must allow the web app's origin).
- `scripts/e2e-local.sh` runs the end-to-end test against a local API and web app (see
  `docs/e2e/README.md`); it refuses anything but localhost. `scripts/e2e-main.sh` does the same
  for Lexpad's window: sign in through the browser, Today, practice online and offline, a word
  opened from the panel, sign out.
- After any change in `lexpad_front` that the window should carry: commit it there, run
  `scripts/build-web.sh <front checkout>` here and commit `web/` with the front commit in the
  message. The script refuses a dirty front.
- Rust comes from the official rustup installer into the user's home (`~/.cargo`), no sudo.
- Releases: see README. Unsigned until the Apple Team ID and a Windows certificate exist; the
  TODOs are marked in `.github/workflows/build.yml`.
- Bump `version` in `package.json` only; `tauri.conf.json` reads it. Keep `Cargo.toml` in step.

## Git

Conventional Commits, small commits, no AI attribution in messages (the `.githooks/commit-msg`
hook rejects it). Remote over HTTPS only; commits and tags unsigned (`commit.gpgsign=false`,
`tag.gpgsign=false` locally). Pushing `main` deploys nothing; CI only builds artifacts.
