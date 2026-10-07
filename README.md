# Lexpad for macOS and Windows

Add the words you meet in **any** app to your [Lexpad](https://lexpad.app) notebook.

Select a word in a document, an e-mail, a PDF or a chat, press **⌘⇧L** on a Mac or
**Ctrl+Shift+L** on Windows, and a small card shows its part of speech, level, pronunciation
and up to three meanings with examples. Press **Enter** to add it, **Esc** to close. The sentence
you met it in is kept as its first example, and a private note says where: "Seen in Microsoft
Word". Nothing selected? A small box lets you type the word. Selected a whole sentence? Pick the
word out of it, and the sentence comes along.

Lexpad lives in the menu bar (macOS) or the system tray (Windows), starts when you log in (you
can turn that off), and runs once. It never asks for your password: you allow it from Lexpad in
your browser, and it appears under Settings → Signed-in devices as "Desktop app", where it can be
signed out on its own.

On macOS there is also **Services → Add to Lexpad** when you right-click a selection.

## What leaves your computer

- When you look a word up: the word, and the sentence it was in as a hint for the meaning.
- When you add it: the word, its meaning, that sentence as an example, and the private note
  "Seen in <app name>".
- Never a window title (they carry document names, e-mail subjects and the names of people you
  chat with), never the rest of the text, never anything you did not select. The selection is
  read only at the moment you press the shortcut.

On macOS, reading another app's selection needs the **Accessibility** permission (System
Settings → Privacy & Security → Accessibility). Lexpad explains this and opens the right pane;
without it you can still type a word. Where an app does not share its selection, Lexpad copies
it (⌘C / Ctrl+C) and then puts your clipboard back exactly as it was.

## Develop

Needs Node 22, pnpm 9 and Rust (stable). Rust was installed with the official installer into
the user's home, with no sudo and no change to the shell profile:

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal -c rustfmt -c clippy
export PATH="$HOME/.cargo/bin:$PATH"          # in each shell, or add it yourself
rustup target add x86_64-apple-darwin          # for universal macOS builds on Apple silicon
```

```
corepack enable pnpm
pnpm install
pnpm app:dev        # run against production (api.lexpad.app, app.lexpad.app)
pnpm check          # prettier, tsc, vitest, vite build, rustfmt, clippy, cargo test
```

Against a local stack (the API and the web app from `lexpad_back` and `lexpad_front`; the API's
`CORS_ALLOWED_ORIGINS` must include the web app's origin):

```
LEXPAD_API_ORIGIN=http://localhost:8091 LEXPAD_APP_ORIGIN=http://localhost:4173 pnpm tauri build --bundles app
open src-tauri/target/release/bundle/macos/Lexpad.app
```

A development build keeps its session in its own Keychain entry and never adds itself to the
login items. Icons are drawn by `pnpm icons` from the Lexpad mark.

Conventions and rules are in `CLAUDE.md`.

## Release

`pnpm tauri build` makes, unsigned for now:

| Platform | Output                                                                                 |
| -------- | -------------------------------------------------------------------------------------- |
| macOS    | `Lexpad.app` and `Lexpad_<version>_universal.dmg` (`--target universal-apple-darwin`)  |
| Windows  | `Lexpad_<version>_x64-setup.exe` (NSIS, per-user) and `Lexpad_<version>_x64_en-US.msi` |

GitHub Actions (`.github/workflows/build.yml`) runs the gate on every push and pull request and
builds both on push to `main` (macos-latest, windows-latest), uploading the installers as
artifacts. No secrets are needed yet.

Still to do before a public release (marked `TODO(signing)` in the workflow):

- **macOS signing and notarization.** Needs the Apple Developer Team ID and a "Developer ID
  Application" certificate. Then set `bundle.macOS.signingIdentity` (or `APPLE_SIGNING_IDENTITY`)
  and the notarization secrets (`APPLE_API_KEY`/`APPLE_API_ISSUER`, or `APPLE_ID`/`APPLE_PASSWORD`/
  `APPLE_TEAM_ID`); `tauri build` signs and notarizes when they are present. Until then a
  downloaded build must be opened with right-click → Open, and macOS asks again for
  Accessibility after every rebuild because the unsigned app's identity changes.
- **Windows signing.** An Authenticode certificate or Azure Trusted Signing
  (`bundle.windows.signCommand`), or SmartScreen warns on install.
- **Microsoft Store (later).** The Store takes an MSIX package or, since 2023, a signed
  `.exe`/`.msi` installer submitted as a Win32 app. The simplest route is to submit the signed
  NSIS installer; an MSIX needs a packaging step (MSIX Packaging Tool or `makeappx`) with the
  Store's publisher identity. Needs a Partner Center account.
- **Updater.** Off in 0.1 (no `tauri-plugin-updater`). Turn it on with a signing key pair and an
  update endpoint before 1.0.
- **Mac App Store** is not planned: it forbids reading other apps' selections through the
  Accessibility API, which is the point of the app.

## Manual test on Windows

Windows cannot be run from the Mac this was built on; CI builds it. Before a Windows release,
go through `docs/windows-manual-test.md`.
