# lexpad_desktop — conventions

The desktop app for Lexpad, on macOS and Windows: select a word in **any** app, press the
shortcut (⌘⇧L / Ctrl+Shift+L), and a small card shows its meaning while it goes into your
notebook with the sentence you met it in. It is the browser extension's card for the whole
computer. Tauri 2: a Rust core (`src-tauri/`) and two small TypeScript pages (`src/`). It is a
client of the same API as the web app (`lexpad_back`); the connect page lives in the web app
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
src/settings.html, src/settings/ the Settings window
src/lib/text.ts, script.ts       copied from lexpad_extension; keep them identical
src/lib/compose.ts               the word as saved, and what a capture turns into (tested)
src/lib/backend.ts               the only bridge to the core: typed `invoke` wrappers
src-tauri/src/lib.rs             wiring: tray, shortcut, windows, plugins
src-tauri/src/capture/           reading the selection: macos.rs (AX, then ⌘C), windows.rs (UIA, then Ctrl+C)
src-tauri/src/services_macos.rs  "Add to Lexpad" in the macOS Services menu
src-tauri/src/api.rs             the API client: the only holder of tokens (tested with a local server)
src-tauri/src/auth.rs            RFC 8252 sign-in: PKCE, state, loopback listener (tested)
src-tauri/src/store.rs           the session in the Keychain / Credential Manager (keyring crate)
src-tauri/src/commands.rs        everything a window may ask; capabilities/*.json say which window may ask what
src-tauri/src/popup.rs           showing, placing and hiding the popup
src-tauri/Info.plist             LSUIElement (no Dock icon) and the NSServices entry
src-tauri/src/e2e.rs             the local end-to-end test (ignored by default), with scripts/e2e-*.{sh,mjs}
docs/e2e/                        the last end-to-end run: what ran, screenshots, evidence
docs/windows-manual-test.md      the Windows checklist, since Windows cannot run here
```

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
  window, nothing else: no shell, no fs, no http from the windows. The only address the app
  opens is the connect page on `APP_ORIGIN` (and, on macOS, the Accessibility pane of System
  Settings), both from Rust. No remote code; the updater is off (TODO before 1.0).
- **Development builds never register themselves to start at login** (`config::is_development_build`:
  debug, or built against a non-production API).

## Build and release

- `pnpm check` (prettier, tsc, vitest, vite build, rustfmt, clippy `-D warnings`, cargo test) is
  the pre-commit gate and CI. Enable the hooks with `git config core.hooksPath .githooks`.
- `pnpm app:dev` runs it against production; for a local stack build with
  `LEXPAD_API_ORIGIN=http://localhost:8091 LEXPAD_APP_ORIGIN=http://localhost:4173 pnpm tauri build --bundles app`
  (the API's CORS must allow the web app's origin).
- `scripts/e2e-local.sh` runs the end-to-end test against a local API and web app (see
  `docs/e2e/README.md`); it refuses anything but localhost.
- Rust comes from the official rustup installer into the user's home (`~/.cargo`), no sudo.
- Releases: see README. Unsigned until the Apple Team ID and a Windows certificate exist; the
  TODOs are marked in `.github/workflows/build.yml`.
- Bump `version` in `package.json` only; `tauri.conf.json` reads it. Keep `Cargo.toml` in step.

## Git

Conventional Commits, small commits, no AI attribution in messages (the `.githooks/commit-msg`
hook rejects it). Remote over HTTPS only; commits and tags unsigned (`commit.gpgsign=false`,
`tag.gpgsign=false` locally). Pushing `main` deploys nothing; CI only builds artifacts.
