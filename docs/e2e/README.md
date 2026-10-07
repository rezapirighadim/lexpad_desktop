# End-to-end test on macOS, 7 October 2026

Everything ran on this Mac against a **local** stack; production was never touched.

| Part        | What ran                                                                                                                                                                                        |
| ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| API         | `lexpad_back` branch `feat/desktop-connect`, `go run ./cmd/api` on :8091, on `lexpad_desktop_e2e`, a copy of the local database migrated to 0053 (the shared local database was left as it was) |
| Web app     | `lexpad_front` branch `feat/desktop-connect`, `vite preview` on :4173, built with `VITE_API_URL=http://localhost:8091`                                                                          |
| Account     | the LOCAL demo account `lena@example.test` (`intro-video/demo/local-account.json`)                                                                                                              |
| Desktop app | `LEXPAD_API_ORIGIN=http://localhost:8091 LEXPAD_APP_ORIGIN=http://localhost:4173 pnpm tauri build --bundles app`                                                                                |

## 1. Sign-in, card, add, sign-out: `scripts/e2e-local.sh` (passed)

The Rust half (`src-tauri/src/e2e.rs`) uses the app's own modules; the browser half
(`scripts/e2e-driver.mjs`) plays the learner in Playwright's Chromium.

1. The app's `auth` code makes a PKCE pair and a state, listens on `127.0.0.1:<random port>` and
   builds the connect address. The browser signs in on the local web app
   ([01](01-browser-sign-in.png)), sees "Connect the Lexpad desktop app?" ([02](02-connect-desktop-allow.png)),
   presses Allow, and is sent to the loopback callback ([03](03-browser-connected.png)). The app
   checks the state, trades the code with the verifier (`POST /auth/desktop/token`) and saves the
   session **in the macOS Keychain** (asserted by reading it back through the keyring crate).
2. The built popup page (`dist/popup.html`) opens in the browser with Tauri's `invoke` answered by
   the app's own API client. Its capture is what the macOS reader returns for "nimble" selected in
   TextEdit, with the text around it. The card shows the real AI meanings from the local API and
   "Seen in TextEdit" with the sentence ([04](04-popup-card.png)); Enter adds it ([05](05-popup-added.png));
   the same word again says "This word is already in your notebook." ([05b](05b-popup-already-added.png)).
3. The API has the word with the sentence as its first example and the private note
   "Seen in TextEdit" (asserted; [word.json](word.json)); the web app shows both
   ([06](06-web-word-seen-in-textedit.png)). Signed-in devices lists "Desktop app · macOS 0.1.0"
   ([07](07-web-signed-in-devices.png), [session.json](session.json): platform `desktop`).
4. The Settings page, with this Mac's real Accessibility state ([08](08-settings.png)).
5. Sign-out revokes the session (its refresh token is refused with 401), empties the Keychain entry,
   and the device leaves the list ([09](09-web-devices-after-sign-out.png)).

Run log: [e2e-run.txt](e2e-run.txt).

## 2. The native app (passed): [native-macos.txt](native-macos.txt)

- The bundle is a menu-bar app (`LSUIElement`), 3.9 MB, with the Services entry.
- A second launch exits at once (single instance).
- Settings are written to `~/Library/Application Support/app.lexpad.desktop/settings.json`; no secret
  in it. A development build adds no login item.
- macOS lists **Services → Add to Lexpad**. Invoking it with `NSPerformService` (what the Services
  menu does) opens the popup: a floating window 360 points wide beside the pointer.

## 3. What macOS did not let an automated shell do

The shell these tests ran in (Claude Code inside VS Code, and Terminal) has **neither Accessibility
nor Screen Recording** permission (`AXIsProcessTrusted() == false`, `screencapture` fails). So:

- the global shortcut could not be pressed by a script, and Lexpad could not read TextEdit's
  selection (both need Accessibility);
- no screenshot of the native windows could be taken (Screen Recording);
- the native Settings window's Connect button could not be clicked.

The same code paths were covered as above (the card in a browser with the app's own API client,
the connect protocol with the app's own code). The rest is a two-minute check for the owner:

1. Open `src-tauri/target/release/bundle/macos/Lexpad.app` (or a CI build).
2. Settings → Connect to Lexpad → Allow in the browser. Settings shows your e-mail.
3. Settings → Reading the selection → Open System Settings → switch **Lexpad** on under
   Privacy & Security → **Accessibility**. (An unsigned build asks again after each rebuild.)
4. In TextEdit select a word, press **⌘⇧L**: the card opens under the word with its meanings and
   "Seen in TextEdit"; Enter adds it. With Accessibility off, ⌘⇧L shows the type-a-word box and
   the explanation instead.
5. Copy an image, select a word in an app that does not share its selection (for example a
   terminal), press ⌘⇧L, then paste: the image is still on the clipboard.

# Lexpad's window, end to end (0.2.0, 7 October 2026)

`scripts/e2e-main.sh`, against the same LOCAL stack (API from `lexpad_back` `origin/main`,
`abed267`, on :8091 with `lexpad_desktop_e2e` migrated to 0054; the web app's connect page from
`lexpad_front` `feat/desktop-app` on :4173; the LOCAL demo account). The window's page is the
built `dist/index.html` (the web app from `web/`, `feat/desktop-app` `5e2d228`) in Chromium, with
`window.__LEXPAD_CONFIG` as the app injects it and every `invoke` answered by the app's own core
code (`src-tauri/src/e2e_main.rs`: the real `connect`, and every API call through `proxy::check`
and `Api::forward` with the session the test holds in its own Keychain entry). Passed:

1. The sign-in screen offers only the browser ([main-01](main-01-sign-in.png)); the browser signs
   in on /connect-desktop and allows; the window opens Today ([main-02](main-02-today.png)).
2. A practice session ([main-03](main-03-practice-card.png)): two cards answered online; then the
   network cut and two more answered, queued in the window's IndexedDB
   ([main-04](main-04-practice-offline.png)); back online, the queue empties.
3. A word opened the way the panel does (`main:open`) shows that word ([main-05](main-05-word-from-panel.png)).
4. Settings → This computer ([main-06](main-06-settings-this-computer.png)), and Account
   ([main-07](main-07-settings-account.png)); signing out there goes through the core, which revokes
   the session and empties its Keychain entry; the window is back at sign-in.
5. Every request the page made reached the core without an Authorization header
   ([requests.json](requests.json), asserted), and the page synced. Log: [main-log.txt](main-log.txt).

Found on the way, not in the desktop app: on this local database copy the API answers `/reviews`
with `rejected: stale` for the demo account, from the plain web app in a browser too (the
reviews' progress still syncs through `/sync/push`). Worth a look in `lexpad_back`; see the to-do
list.

Not covered from an automated shell (no Accessibility or Screen Recording here): the native
window itself (title bar, Dock icon, restoring its place), native notifications appearing and
their clicks, and the global shortcut. These are on the owner's checklist.
