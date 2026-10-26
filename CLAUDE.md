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
src-tauri/src/msix.rs            Windows, the Store's package only: start on login as its StartupTask, is_packaged
src-tauri/src/release.rs         the newest desktop versions from lexpad.app/version.json, for the window's update line (tested)
msix/                            the Microsoft Store's MSIX: AppxManifest.xml (Partner Center identity) and Assets/ (icons.py)
scripts/msix.ps1, msix-smoke.ps1 CI: pack the unsigned .msix; install a test-signed copy and check it (README, "Microsoft Store")
src-tauri/src/notify/            notifications: the reminder plan, the inbox poll and its rules (tested), macos.rs, windows.rs
web/                             the web app built for the window by scripts/build-web.sh (committed; never edit; PROVENANCE.txt)
src-tauri/Info.plist             LSUIElement (no Dock icon) and the NSServices entry
src-tauri/src/e2e.rs             the local end-to-end test (ignored by default), with scripts/e2e-local.sh
src-tauri/src/e2e_main.rs        the window's end-to-end test (ignored), with scripts/e2e-main.{sh,mjs}
docs/e2e/                        the last end-to-end run: what ran, screenshots, evidence
docs/screenshots/                the panel in light and dark, and the tray icons (scripts/panel-screenshots.mjs, icons.py --preview)
scripts/icons.py                 every icon, drawn from landing/site/assets/favicon-v2.svg (`pnpm icons`);
                                 it rewrites icon.ico after `tauri icon` (BMP layers 16-96 px, PNG 256, planes 1)
scripts/icon-check.ps1           CI: the exe's icon layers, the shortcuts, and what Explorer draws at each size
docs/windows-manual-test.md      the Windows checklist, since Windows cannot run here
src-tauri/src/media/             the video player's media engine (see "The video player's engine" below)
src-tauri/src/media/commands.rs  the player's commands (typed for the page in src/lib/media/api.ts)
src-tauri/src/word_queue.rs      words saved before they can reach the account (signed out, offline), sent after sign-in (tested)
src-tauri/src/player_dev.rs      the engine's development page's window, only with `--features player-dev` (never shipped)
src/lib/media/                   the page side: api.ts (typed commands), stream.ts (MSE), cues.ts (subtitle timing, tested)
src/player-dev.html, player-dev/ the development page (LEXPAD_PLAYER_DEV=1 + feature player-dev): <video> + MSE + 3 subtitle tracks
src-tauri/src/player_window.rs   the Lexpad Player's window (the web app's /player), the mini player, its Windows taskbar button (tested)
src-tauri/src/app_menu.rs        the two windows: where Lexpad opens, ⌘1 / ⌘2, the macOS menu bar's Playback, Subtitles and Window menus (tested)
src-tauri/src/welcome.rs         the first launch's window and its commands (tested)
src/welcome.html, src/welcome/   "What would you like to do first?" and the soft sign-in (tested with happy-dom)
docs/player-dock-icon.md         why the Player shares Lexpad's Dock tile on macOS (phase 1)
scripts/ffmpeg/                  the bundled LGPL FFmpeg: versions.env (pinned), build-macos.sh, fetch-prebuilt.sh, check-lgpl.sh
scripts/media-samples.sh         the player's test matrix (synthetic clips and subtitles; local only, never committed)
scripts/player-bench.sh          start-up, seek latency, CPU and memory on this Mac, through the development page
src-tauri/windows/media-open-with.nsh  the installer's "Open with Lexpad" and Default Apps entry (never the default by itself)
docs/ffmpeg.md                   FFmpeg's licence (LGPL only), sources, builds, and how to update it
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
  calls; the activity headers `X-Lexpad-Surface` / `X-Lexpad-Context` only when well formed, and a
  surface only when it is the desktop app's own, `desktop.`…) and sends it with the desktop app's own delegated session from the credential store,
  refreshing it single-flight, streaming the body back over a channel. No server change was
  needed: the delegated session may do everything the app does day to day, and what it may not
  (a password, linking Google or Apple, connecting another client or an assistant) the window
  opens in the browser (`open_in_browser`, a checked same-origin path).
- **Signing in is the same RFC 8252 connect**: the window's sign-in screen is one button that
  runs `connect` (system browser, loopback, PKCE). No password is ever typed in the window, and
  Google and Apple accounts work, since their sign-in happens in the real browser.
- **The window may only show the app's own pages** (`is_app_page`); any other address goes to the
  system browser (https and mailto only, `may_open_outside`), never into the window. A clicked link
  (`target="_blank"`, `window.open`) must reach those handlers: the opener plugin is built
  with `open_js_links_on_click(false)`, since its default script cancels such a click and calls
  its own `open_url`, which no window is granted (links did nothing up to 0.3.14). Files the
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
  lets through (see its doc: unread, fresh, not come-back, product news only when allowed, a
  note about the plan only when its row says `popUp`, not already popped elsewhere when the API
  says so). A click opens the window on the row's `tapPath` (else its old `link`, else Today),
  only when it is a plain in-app path (`click_path`); a message's own button that opens an
  address goes through `window.open`, so `on_new_window` hands it to the system browser.
- **A newer version is said on Today** (`release.rs`, command `latest_release`, front
  `lib/updates.ts`): the updater is off, so the window asks the core about once a day, after a
  sync, and the core reads the desktop section of `lexpad.app/version.json` (a fixed address,
  no session, the usual User-Agent; `LEXPAD_SITE_ORIGIN` at build time for a test). The landing's
  `scripts/features.py` writes that section from `DESKTOP_VERSIONS` and `MS_STORE_VERSION`, so
  a release that moves those moves the notice (README, "Release steps" 4 and 5). The core
  returns versions and download pages with `os` and `install` (`msix::is_packaged`: store or
  direct); the web app decides what to offer (a Store copy only hears of a version the Store
  serves, Store first) and checks every address. Links leave through `on_new_window`. Offline
  or with the site down nothing is shown and nothing breaks.
- **Accessibility after an update**: up to 0.3.2 the Mac build was ad-hoc signed, a new code
  identity each release, and macOS stopped applying the old grant while the switch still looked
  on. From 0.3.3 `scripts/release-mac.sh` signs with one self-signed certificate, "Lexpad Local
  Signing" (README, "macOS signing"), so the designated requirement is stable and the grant
  survives updates; users updating from ≤0.3.2 remove Lexpad with − and add it again once. The
  first release signed with the Developer ID ("Developer ID Application: Reza Pirighadim
  (6Z95MR26LZ)", 2026-10) changes the designated requirement once more, so every user re-adds
  Lexpad under Accessibility one last time; after that the requirement names the team, not a
  certificate, so a renewed Developer ID certificate keeps the grant. Never make a new local
  certificate while the fallback is in use, and never commit or export a signing key. The core still remembers the version that last had the permission
  (`accessibility_granted_in`) and says so plainly (`permissionStale`), and the card and the
  Settings window ask again every few seconds and on focus while they explain the permission
  (`src/lib/accessibility.ts`), so the hint goes away once it is allowed.

## The video player's engine (branch feat/video-player, 2026-10)

The owner approved a video player in Lexpad's window (memory: lexpad-video-player). The UI is a
page written by us (designed separately); this repository holds the engine.

- **One stream for both webviews.** WKWebView and WebView2 both play H.264 (8-bit 4:2:0) + AAC
  in MP4, so every file becomes that (`media/plan.rs`): `direct` (an MP4 that already is, one
  audio track: served as the file with byte ranges), `remux` (copied into fragmented MP4),
  `audioTranscode` (AC-3, DTS, Opus, MP3... to AAC), `videoTranscode` (HEVC, VP9, AV1, ProRes,
  MPEG-2/4, 10-bit H.264, interlaced: hardware H.264, VideoToolbox / NVENC / QSV / AMF / Media
  Foundation, OpenH264 last; never x264). HEVC/VP9/AV1 are transcoded even where one webview
  could play them: both platforms must behave the same. HDR (PQ, HLG) is tone-mapped to SDR
  BT.709 (zscale + Hable, CPU, capped at 1080p).
- **The bundled FFmpeg is LGPL only** (docs/ffmpeg.md): Tauri sidecars, built on the Mac
  (`scripts/ffmpeg/build-macos.sh`) or fetched pinned for Windows; `check-lgpl.sh` refuses
  anything else. `src-tauri/binaries/` is never committed; a build needs it (Tauri checks
  `externalBin`), so run the script first. Every child process gets CREATE_NO_WINDOW on Windows.
- **The `lexmedia` scheme, not a socket.** `lexmedia://localhost` (macOS),
  `http://lexmedia.localhost` (Windows): `/f/<session>` byte ranges for `direct`,
  `/s/<session>/<run>?from=<offset>` pulls of a run's fragmented MP4 (`media/stream.rs`). A run
  is one FFmpeg from a start time with `-copyts` and a fixed `TIMELINE_OFFSET`, so every run is
  on one timeline; a seek outside the buffer starts a new run. A run holds at most 48 MB the
  page has not pulled, then FFmpeg waits (back-pressure). Only the player windows are answered.
  The CSP's `connect-src` and `media-src` allow this scheme; nothing else was opened.
- **Files by id only.** The core grants what the learner dropped (Tauri's drag-drop event),
  chose (the dialog plugin, called from the core: no window has its commands) or opened with
  Lexpad, plus the videos and subtitles beside a granted video (episodes, matched subtitles).
  A window gets opaque ids; no command takes a path.
- **Subtitles are ours** (`media/subs/`): SRT, WebVTT, ASS/SSA (basic styling kept), mov_text
  through FFmpeg; embedded tracks are copied as raw bytes so old encodings reach the detector
  (chardetng, then encoding_rs); a Persian Windows-1256 track's Arabic yeh/kaf are written as
  Persian. The language comes from the text (whatlang plus a letter check for fa/ar/ur), the
  file name and the container's tag, with a confidence. Picture subtitles (PGS, VobSub) are
  reported as unsupported.
- **Opening files**: drag and drop on a player window, the open dialog, "Open with Lexpad"
  (macOS `RunEvent::Opened`; Windows argv and the single-instance hand-over). Lexpad never
  becomes the default by itself (macOS rank Alternate; Windows OpenWithProgids only); "Use
  Lexpad for video files" (`media/default_player.rs`) sets it on macOS after the page asked,
  and opens Settings > Default apps on Windows.
- **Words saved without a session** (signed out, offline) wait in `pending-words.json`
  (`word_queue.rs`) and go to the notebook of their language after sign-in; never to a notebook
  of another language. A word saved signed out moves to the account that signs in only within 24 h
  (`may_join`, owner 26 Oct 2026, the same limit as anonymous activity events); an older one, or
  one dated ahead of the clock by more than five minutes, stays on this computer unsent as
  `too_old_to_move` for the learner to discard (`words_discard`) or send on a tap
  (`words_send_one`, to the signed-in account whatever its age, with its own surface). A word saved
  under one account never goes to another (`words_send_one` refuses it: `not_this_account`).
  `player_account` answers `userId` (null signed out), so the Player labels its events with the
  core's account.

## The video player's windows (branch feat/video-player, 2026-10)

The Player's page is the web app's `/player` route (lexpad_front `features/player`), bundled in
`web/` like the rest; this repository holds its window and the app's structure around it (spec
`uiux/specs/2026-10-25-video-player.md` §2, §13, §24).

- **Two windows, one app.** Lexpad's window (learning) and the Player (`player_window.rs`, label
  `player`): the same bundled build, configuration script, navigation lock and session rule as
  Lexpad's window, started on `#/player` by its initialization script (`?from=app|file|pick`
  says how it was opened). Frameless: on macOS the traffic lights sit at (18, 18) over the page
  and hide with the player's chrome (`player_buttons`); on Windows the page draws the caption
  buttons. 72% of the work area at 16:9, its place remembered (`player_window` in settings.json).
  The mini player is the same window made 448×252 and kept on top, in the screen's corner, and
  back again (one decoder, one stream). Full screen is the system's (macOS space, Windows
  borderless).
- **Each opens the other.** "Watch" in the learning window's rail and its Watch page open the
  Player (`open_player`, with `pick` for the file dialog or a Continue watching `file_id`);
  the Player's "Lexpad" button and "See it in your notebook" open the learning window at a
  checked app path (`open_lexpad`). ⌘1 / ⌘2 are the Window menu's items on macOS; on Windows
  each page answers Ctrl+1 / Ctrl+2 (`open_window`). Files opened from the system and drops on
  either window open the Player.
- **Lexpad opens where it was left** (`app_menu`): the window last in front (`last_place`),
  unless Settings → This computer → "When I open Lexpad" names Today or the Player (`open_at`).
  The Dock, a second launch and the tray's Open Lexpad go there; a pinned Player button on
  Windows starts Lexpad with `--player`.
- **One Dock tile on macOS** for both windows (`docs/player-dock-icon.md`). Windows gives the
  Player its own taskbar button (AppUserModelID `Lexpad.Player`, `taskbar::own_button`).
- **The menu bar mirrors the Player's keys** (Playback, Subtitles) with no bare-key
  accelerators: a menu item with a plain key would be taken from every text field. Each item
  sends its command to the Player (`player:menu`); the page owns the keys.
- **The first launch asks what to do first** (`welcome.rs`, `src/welcome/`): Watch, Learn or
  Both, then a soft sign-in for that path with a provider hint (`connect(provider)` adds
  `provider=google|apple` to the connect page). It replaces the first run's Settings hello; it
  is skipped when a video opened Lexpad or a session exists; closing it counts as Both. The
  answer (`focus`) picks where Lexpad opens and which tips show, and locks nothing. No trial,
  no Pro and no numbers on it.
- **Capabilities.** `player.json` grants the Player the engine, the words queue, the card's
  calls, the API proxy and its own window's controls; `main.json` keeps of the engine only what
  the Watch page and Settings → Video player read; `welcome.json` its five commands.
- **Engine requests** go in `NOTES-player-ui.md`, never as edits to `src-tauri/src/media/`.

## Hard rules

- **The core is the only part that touches tokens or the API.** The windows call commands and
  render answers; a command never returns a token. A new capability is a new command in
  `commands.rs`, listed in `build.rs` and granted in the window's capability file; nothing else
  is callable. No `fetch` from a window to the network: the CSP's `connect-src` is IPC and the
  local `lexmedia` scheme (the video player's media, served by the core) only.
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
- **Where a word came from, for the activity reports** (reports/specs/2026-10-26-activity-tracking.md
  in the workspace): the core's own lookup and add send `X-Lexpad-Surface` from the capture that
  opened the popup (`commands::surface_of`: the shortcut, the selection or a word typed after it,
  is `desktop.shortcut`; the Services menu `desktop.services`; the tray panel's box `desktop.tray`;
  a word the video player saves through `words_save` (sent now or from the queue), or looks up and
  adds through `lookup` / `add_word`, `desktop.player` or `desktop.player.transcript`, from its
  `from`, which wins over the popup's capture and refuses any other value),
  and only while GET /config says `activity: true` (`Api::activity_now`: the cached answer only,
  never awaited on a word's path; refreshed in the background at start, after sign-in and when 6
  hours old, one read at a time; unknown or a failed read is off). The window's app sets its own (`desktop.app.*`) and the
  proxy lets them through. Background calls (the inbox poll, settings) say nothing.
- **Every call says which copy it is, in its User-Agent and nothing else** (`config::user_agent`,
  since 0.3.11): `Lexpad-Desktop/<version> (macOS)`, `(Windows; store)` for the Microsoft
  Store's MSIX (`msix::is_packaged`) or `(Windows; direct)` for the installer from lexpad.app.
  The API keeps it with the session the sign-in opens, so the operator console can show
  "Desktop app · Windows · Store". No machine name, account or other detail goes in it.
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
- **The card's AI-written meaning can be reported** (store rules for generative AI, Microsoft
  Store 11.16, 2026-10-13). Once meanings arrive the card shows a quiet Report; it calls
  `open_report` with base64url of the card's text (`src/lib/report.ts`, the extension's same
  payload), and the core checks it is base64url only and opens the web app's
  `/report-ai#r=…` in Lexpad's window or the browser (as `open_web` decides), where the form is.
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
  the pre-commit gate. Enable the hooks with `git config core.hooksPath .githooks`. The hook on
  this Mac is the only thing that compiles the macOS-only modules on every commit.
- CI is billed by the minute (private repository; macOS ×10, Windows ×2), so it is kept small:
  every push and pull request runs one Linux job (what `pnpm check` runs, on Linux with dead code
  allowed since capture and notifications are stubs there, then clippy for the Windows target); docs-only pushes run nothing. Only a `v*` tag or a manual run
  (`gh workflow run build`) builds on Windows: installers, a silent install and start, and the
  placement smoke test. Do not add a macOS job or a per-push Windows job back.
- The placement smoke test is `pnpm tauri build --no-bundle --features smoke-test`, then
  `lexpad-desktop --smoke-test`; it prints every rectangle and exits 1 if one leaves its work area.
  It does not touch the credential store, the shortcut or the login items, and runs beside an
  installed copy.
- **macOS release**: `scripts/release-mac.sh` on this Mac (universal `.dmg`, production, signed
  with the Developer ID and checked with `codesign`, notarized with `notarytool` and the App
  Store Connect API key from `~/.appstoreconnect/lexpad.env`, stapled, smoke-tested, SHA-256
  printed; the Mac must be awake and unlocked). Without the Developer ID certificate it says so
  and falls back to "Lexpad Local Signing", not notarized. The certificate's key is made by
  `scripts/make-developer-id-csr.sh` (README, "macOS signing"). The whole release flow (tag, Windows artifact from CI, the Mac
  `.dmg`, the landing's downloads) is in README, "Release steps".
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
- **Microsoft Store**: the tag's CI run also makes `Lexpad_<version>_x64.msix` (artifact
  `lexpad-desktop-msix`, unsigned; the Store signs it). Each release is a new Partner Center
  submission with that file (README, "Microsoft Store"). Anything that writes outside the app's
  own folders or the credential store must be checked inside the package: a packaged app's
  registry writes stay in the package (that is why start on login is `msix.rs` there).
- Releases: see README. macOS is signed with the Developer ID and notarized (Apple team
  6Z95MR26LZ), locally only: CI builds no macOS. Windows is unsigned until a certificate exists
  (TODO in `.github/workflows/build.yml`). No key, key id or issuer id is ever committed here:
  this repository is public.
- Bump `version` in `package.json` only; `tauri.conf.json` reads it. Keep `Cargo.toml` in step.
- **GitHub Release** (every release, owner 2026-10-09): after the Windows CI run and the Mac
  `.dmg`, `gh release create v<version> --verify-tag --latest` titled "Lexpad for Mac and Windows
  <version>" (no "(beta)": out of beta since 2026-10-21, owner), with `Lexpad_<version>_universal.dmg`, `Lexpad_<version>_x64-setup.exe`,
  `Lexpad_<version>_x64_en-US.msi` (the CI artifact's names) and a `SHA256SUMS.txt` of those three
  files. Its values must equal the landing's `site/downloads/lexpad-desktop.sha256`; download the
  release back and run `shasum -a 256 -c SHA256SUMS.txt`. Notes: what's new in plain English
  (from the landing changelog), the install notes as the download pages word them (Mac signed
  and notarized since 0.3.7, the Windows installers unsigned), links to lexpad.app/download/mac and /windows, the CI run id, then a "Technical"
  list with commit ids. This repository is public: no secrets, keys or private notes in a release.

## Git

Conventional Commits, small commits, no AI attribution in messages (the `.githooks/commit-msg`
hook rejects it). Remote over HTTPS only; commits and tags unsigned (`commit.gpgsign=false`,
`tag.gpgsign=false` locally). Pushing `main` deploys nothing; CI builds artifacts only for a tag.
