# Lexpad in the Mac App Store: the plan

Owner's approval: 1 November 2026, "do the App Store too, with full care, and send it to the
App Store". From now on every desktop release ships four builds:

1. the macOS dmg from lexpad.app (unchanged, `scripts/release-mac.sh`);
2. the Windows installer (CI, unchanged);
3. the Microsoft Store MSIX (CI, unchanged);
4. **the Mac App Store build** (new, `scripts/release-mas.sh`).

The owner's steps are in [mac-app-store-owner-steps.md](mac-app-store-owner-steps.md).

## Sources (read 1 Nov 2026)

- App Review Guidelines, 2.4.5 (Mac apps), 2.5.1 (public APIs only), 3.1.1 and 3.1.2 (in-app
  purchase, subscriptions), 3.1.3(f) (multiplatform services), 5.1.1 (privacy):
  <https://developer.apple.com/app-store/review/guidelines/>
- App Sandbox and its entitlements:
  <https://developer.apple.com/documentation/security/app-sandbox>,
  related items: <https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundledocumenttypes/nsisrelateditemtype>
- Embedding a helper tool in a sandboxed app (`com.apple.security.inherit`):
  <https://developer.apple.com/documentation/xcode/embedding-a-helper-tool-in-a-sandboxed-app>
- Universal Purchase: <https://developer.apple.com/support/universal-purchase/>
- `SMAppService` (login items): <https://developer.apple.com/documentation/servicemanagement/smappservice>
- Services (`NSServices`, `NSKeyEquivalent`):
  <https://developer.apple.com/documentation/bundleresources/information-property-list/nsservices>
- Tauri's App Store guide: <https://v2.tauri.app/distribute/app-store/>

## The main decision: one App Store record (Universal Purchase)

**Recommendation: add macOS as a second platform on the iPhone app's App Store Connect record
(`Lexpad: AI Vocabulary Notebook`, app id 6822224295), with the same bundle id
`app.lexpad.ios`.** Not a separate Mac app record.

Why:

- **One subscription, no double billing.** The iPhone's group "Lexpad Pro" (`pro_monthly`,
  `pro_yearly`) then serves the Mac too. A separate record would need new product ids (ids are
  unique per developer account), a second subscription group, and a person could end up paying
  Apple twice for the same Pro.
- **No server change for receipts.** The API checks that a transaction names the bundle id it is
  set up for (`APPSTORE_BUNDLE_ID=app.lexpad.ios`). A Mac purchase under Universal Purchase names
  the same id, so `POST /billing/apple/verify`, the App Store Server API calls, the
  introductory-offer and promotional-offer signatures and the server notifications URL
  (already set on the record, V2, production and sandbox) all work as they are. The back PR
  proves this with an integration test.
- **The same prices, offers, review screenshots and App Privacy answers**, kept in one place.
- Facts checked with the App Store Connect API on 1 Nov 2026: the bundle id `app.lexpad.ios`
  is registered as UNIVERSAL (it can be used on macOS); the iPhone version 1.0.81 and both
  subscriptions are _Waiting for Review_ (the iPhone app is not live yet).

What it costs:

- The Mac App Store copy has the bundle id `app.lexpad.ios`, not the dmg's `app.lexpad.desktop`.
  They are two different apps to macOS (own settings, own Keychain entry, own sign-in). A person
  with both sees two menu-bar icons; the dmg should not be installed beside the App Store copy,
  and the download page can say so later.
- Once a native Mac app exists on the record, Apple stops offering the iPad app on Apple silicon
  Macs. That is wanted.
- The app name and subtitle are shared with the iPhone; description, keywords, screenshots and
  What's New are per platform.

## Other platforms inside the app (Guideline 2.3.10)

App Review Guideline 2.3.10, as published on 1 Nov 2026
(<https://developer.apple.com/app-store/review/guidelines/#2.3.10>): "Make sure your app is
focused on the iOS, iPadOS, macOS, tvOS, visionOS, or watchOS experience, and don't include
names, icons, or imagery of other mobile platforms or alternative app marketplaces in your app
or metadata, unless there is specific, approved interactive functionality."

What that covers, and what the owner decided on 1 Nov 2026:

- **Removed in the Mac App Store build:** Android and Google Play (another mobile platform and
  its marketplace) and the Microsoft Store badge (another app marketplace). Lexpad everywhere
  lists no Android row, the Player's end card shows no Google Play, the desktop phone card
  recommends the iPhone app only (once it is in the App Store), and an Android device in the
  signed-in list is a "Phone app" (lexpad_front `namesOtherMarketplaces`).
- **Kept**, because the rule names other _mobile_ platforms and other _app marketplaces_, and
  the owner wants the Mac app to keep recommending Lexpad's own surfaces: the iPhone app
  (Apple's own platform, linked to the App Store), the web app, the browser extension for
  Chrome, Edge and Firefox, and Windows where the app already mentions it.
- The guard is `e2e/mac-app-store-platforms.spec.ts` in lexpad_front: it opens every page of the
  Mac App Store build and fails on "Android", "Google Play", "Play Store", "Microsoft Store" or a
  link to those stores, and checks that the iPhone and browser extension rows are still there.
- The listing text names no other platform at all (metadata is held to the same rule).
- Risk: a reviewer may still read the extension's browser names strictly. If App Review asks,
  the extension rows can be hidden with the same switch in one small front change.

## What the Mac App Store build does differently from the dmg

One codebase. The Mac App Store build is `--features mas` plus the overlay
`src-tauri/tauri.mas.conf.json`; every difference is behind `config::MAS` (Rust) or
`isMacAppStore()` (lexpad_front, from `__LEXPAD_CONFIG.distribution === "mas"`).

|                                           | dmg (lexpad.app)                                                 | Mac App Store                                                                                |
| ----------------------------------------- | ---------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| Bundle id                                 | `app.lexpad.desktop`                                             | `app.lexpad.ios` (Universal Purchase)                                                        |
| Sandbox                                   | no                                                               | yes (`src-tauri/mas/Lexpad.entitlements`)                                                    |
| ⌘⇧L                                       | global shortcut; reads the selection with Accessibility, then ⌘C | the Services menu's "Add to Lexpad" key (`NSKeyEquivalent` L = ⌘⇧L); no Accessibility, no ⌘C |
| Accessibility permission                  | asked                                                            | never (`accessibility: not_needed`)                                                          |
| Start on login                            | autostart plugin (LaunchAgent)                                   | `SMAppService.mainApp`, only when the learner turns it on                                    |
| Updates                                   | "a new version" line on Today, from lexpad.app/version.json      | none: the App Store updates it (`latest_release` says `install: appstore`, no versions)      |
| Buying Pro                                | not sold in the app                                              | Apple in-app purchase, StoreKit 2 (`swift/LexpadStore`)                                      |
| Promo codes, web checkout, invite rewards | as today                                                         | refused / hidden (Guideline 3.1.1)                                                           |
| Panel's rounded corners                   | transparent webview (WebKit private key)                         | public AppKit layer mask (`panel::round_corners`)                                            |
| Subtitles next to a film                  | read from the folder                                             | same-name files as related items (Film.mkv → Film.srt); others by choosing or dropping them  |
| Episodes in the same folder               | listed                                                           | only those the learner chose or dropped together (see "Known limits")                        |
| User-Agent                                | `Lexpad-Desktop/<v> (macOS)`                                     | `Lexpad-Desktop/<v> (macOS; appstore)` (the API sells through Apple for it)                  |
| Minimum macOS                             | 11                                                               | 13 (StoreKit's AppTransaction, SMAppService)                                                 |
| Package                                   | notarized dmg                                                    | `.pkg` signed for the App Store, uploaded with `altool`                                      |

## Sandbox entitlements

`src-tauri/mas/Lexpad.entitlements`, each key with the reason beside it:

- `com.apple.security.app-sandbox`: required (2.4.5(i)).
- `com.apple.security.network.client`: the API and lexpad.app.
- `com.apple.security.network.server`: the sign-in's one-time loopback listener on
  `127.0.0.1` (RFC 8252, `auth.rs`). App Review sometimes asks about this key; the review notes
  explain it (see below).
- `com.apple.security.files.user-selected.read-only`: films and subtitles the learner opens.
- `com.apple.security.files.downloads.read-write`: exports saved from Lexpad's window go to
  Downloads, as today.
- `com.apple.application-identifier` and `com.apple.developer.team-identifier`: the App Store
  identity, matched by the provisioning profile.

Not asked for (and why): Accessibility and Apple Events (not allowed in a sandbox and not
needed), microphone and camera (the bundled web app does not use them today; when the Player's
"say after the actor" ships, add `com.apple.security.device.audio-input` and
`NSMicrophoneUsageDescription` in the same change), bookmarks (see "Known limits").

**Subtitles next to a film.** `Info.mas.plist` declares the subtitle types (srt, vtt, webvtt,
ass, ssa) with `NSIsRelatedItemType`. When the sandbox will not let the core list the film's
folder, `media/files.rs` asks macOS for the same-name subtitles through `NSFileCoordinator`
with a file presenter whose primary item is the film (`swift/.../Related.swift`). macOS grants
only exact same-name files (Film.srt, not Film.en.srt).

## The shortcut ⌘⇧L, and what the learner sees

A sandboxed app may not read another app's selection with Accessibility or send it ⌘C, so the
Mac App Store build registers no global shortcut. Instead its Services entry "Add to Lexpad"
carries `NSKeyEquivalent` `L` (an upper-case letter means Shift; Command is implied): ⌘⇧L.

What the learner sees:

- Select a word in Safari, Mail, Notes, Pages, Word, Kindle and most other apps, press ⌘⇧L:
  Lexpad's card opens with the word, as in the dmg. macOS sends the selected text; Lexpad never
  reads anything else.
- The same is in the right-click menu: Services › Add to Lexpad.
- The card has no example sentence from the page (the Services path gives only the selection;
  the dmg's Accessibility path gives the sentence around it). Nothing is made up instead.
- In an app that does not offer Services for its text (some Electron and game apps), ⌘⇧L does
  nothing; the menu-bar panel's box still adds a typed word.
- Settings › This computer shows the key read-only: "Select a word in any app and press ⌘⇧L.
  You can change the key in System Settings › Keyboard › Keyboard Shortcuts › Services." There
  is no Accessibility row.
- If another app already uses ⌘⇧L, macOS gives that app the key; the learner can pick another
  key for Add to Lexpad in System Settings.

## No self-updater

The updater plugin was never in the app. The only update path is the Today line that reads
lexpad.app/version.json; in the Mac App Store build the core answers `install: appstore` with
no versions and never fetches the file, and the web app shows no line (front PR). Guideline
2.4.5(vii) is met: the App Store is the only update mechanism.

## Bundled FFmpeg

The same LGPL-only FFmpeg and FFprobe as the dmg (`docs/ffmpeg.md`, `scripts/ffmpeg/`).
`scripts/release-mas.sh` signs both with `src-tauri/mas/ffmpeg.entitlements` (exactly
`app-sandbox` and `inherit`), then the app, inside out, and refuses a build where either does
not inherit the sandbox. They run inside Lexpad's sandbox and read only what Lexpad may.
Children are killed when their run is dropped (`kill_on_drop`) and stop on a broken pipe if
Lexpad quits (2.4.5(iii): no process outlives the app). The licence notice and source offer stay
in About, as in the dmg; the App Store listing's "License Agreement" stays Apple's standard
EULA (LGPL needs no change to it, since the libraries are separate executables that can be
replaced).

## Notifications

Unchanged: `UNUserNotificationCenter` for the daily reminder and the inbox's messages works in
the sandbox. No push entitlement is needed (the server's messages come from the inbox poll).

## Signing in

Unchanged: the system browser, a loopback redirect, PKCE (RFC 8252). It needs
`network.server` for the listener. No password is typed in the app, so Guideline 4.8 (Sign in
with Apple) is met by the web sign-in page itself, which offers Apple next to Google.

## Pro purchase

- StoreKit 2 in Swift (`src-tauri/swift/LexpadStore/Sources/LexpadStore/LexpadStore.swift`),
  compiled by `build.rs` with `swiftc` only for `--features mas`, linked into the core, and
  called through two C functions by `src-tauri/src/appstore.rs`. It is the iPhone shell's
  `StoreKitPlugin.swift` for the Mac: the same methods, answers and error codes.
- The window calls `store_*` commands (granted only to Lexpad's window, `capabilities/main.json`).
  In the dmg and on Windows every one answers `unavailable`.
- The web app (front PR) uses the iPhone's billing code for the Mac: Apple prices from StoreKit
  only, the server's checkout for the eligibility JWS, verify, finish after 200, Restore
  purchases, manage in Apple's page, the 3.1.2 disclosure with Terms and Privacy links.
- The server (back PR) treats a desktop session whose User-Agent says `(macOS; appstore)` as
  sold by Apple: checkout answers Apple, promo codes are refused, purchase links in the bell are
  removed, reports name the install `appstore`. The switch is the same `selling.ios` as the
  iPhone (they sell the same products).
- StoreKit needs a window to attach its sheet to from macOS 15.2; the bridge passes Lexpad's
  key window (`purchase(confirmIn:)`).

## Privacy manifest and App Privacy

- The App Privacy answers are per record, so the Mac shares the iPhone's. The desktop app
  collects nothing the iPhone app does not (account, the learner's words, usage events, crash
  data through the API). Check them once before submitting; no change expected.
- The privacy manifest is `src-tauri/mas/PrivacyInfo.xcprivacy`, put in `Contents/Resources`
  by `bundle.macOS.files`: no tracking, the same collected data types as the iPhone's manifest,
  and the "required reason" APIs the core reaches: file timestamps (the media engine's cache of
  the learner's own files: `C617.1`, `3B52.1`) and the system clock for elapsed time (`35F9.1`).

## Export compliance, category, age rating

- Export compliance: the app uses only the encryption built into macOS (TLS through
  Security.framework, the Keychain): exempt. `ITSAppUsesNonExemptEncryption = false` in
  `Info.mas.plist`, so App Store Connect asks nothing per build.
- Category: Education (`LSApplicationCategoryType` `public.app-category.education`, from
  `bundle.category`); shared with the record.
- Age rating: the record's (12+, set for the iPhone); the questionnaire is per app.

## One codebase, two macOS builds

- `cargo` feature `mas` (macOS only) and `config::MAS`.
- `src-tauri/tauri.mas.conf.json`: identifier, `macOSPrivateApi: false`, `.app` only, macOS 13,
  `Info.mas.plist`, the entitlements.
- `scripts/release-mas.sh` builds it (universal), signs inside out, checks it (sandboxed,
  helpers inherit, bundle id, version, category, the Services key, the export answer, the
  binary is the mas one), packages, validates with Apple, and uploads with
  `LEXPAD_MAS_UPLOAD=1`. `--local` makes a sandboxed test copy signed "Apple Development".
- The tauri CLI rewrites Cargo.toml's tauri features when `macOSPrivateApi` is false; the script
  restores Cargo.toml and Cargo.lock on every exit, so the dmg build stays as it is.
- CI: unchanged (no macOS runner, by the repository's rule). The Linux job compiles the default
  features; `pnpm check` on this Mac compiles both.

## Release flow (every desktop release from now on)

1. As today: bump, tag, CI Windows + MSIX, `scripts/release-mac.sh` for the dmg.
2. `LEXPAD_MAS_UPLOAD=1 scripts/release-mas.sh` from the same tag (`LEXPAD_MAS_BUILD` when a
   version is uploaded again).
3. In App Store Connect: the macOS version, its What's New, the build, Submit for Review (the
   owner confirms the submit).
4. GitHub Release notes mention the Mac App Store version; the App Store package itself is not
   attached (it is useless outside the App Store).

## Review risks and how each is avoided

| Risk                                                    | Guideline      | What we do                                                                                                                                                                                                                                                                   |
| ------------------------------------------------------- | -------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Private API (transparent webview)                       | 2.5.1          | panel transparency off in mas; corners by a public layer mask. wry still sets a few WebKit preference keys by name (PiP, fullscreen on old macOS); they are strings, not symbols, and Tauri apps ship in the store with them. If Apple objects, pin a wry fork without them. |
| Not sandboxed / helper not sandboxed                    | 2.4.5(i)       | script refuses such a build                                                                                                                                                                                                                                                  |
| Asking for Accessibility                                | 2.4.5, sandbox | never asked in mas                                                                                                                                                                                                                                                           |
| Other update mechanism                                  | 2.4.5(vii)     | no update line, no version fetch                                                                                                                                                                                                                                             |
| Login item without consent                              | 2.4.5(iii)     | SMAppService only from Settings; off by default                                                                                                                                                                                                                              |
| Selling outside IAP; links to web checkout; promo codes | 3.1.1          | front + back PRs hide and refuse them                                                                                                                                                                                                                                        |
| Pro not offered as IAP although sold elsewhere          | 3.1.3(f)       | Pro is sold in the app                                                                                                                                                                                                                                                       |
| Subscription disclosure                                 | 3.1.2          | the iPhone's sheet: length, price, renewal, cancel, Terms, Privacy, Restore                                                                                                                                                                                                  |
| Naming other platforms ("get the Android app")          | 2.3.10         | Android, Google Play and the Microsoft Store are hidden in mas; an e2e scan guards it (section "Other platforms inside the app")                                                                                                                                             |
| Reviewer cannot sign in                                 | 2.1            | apple-reviewer@lexpad.app (the iPhone's, owner 1 Nov 2026); the notes explain sign-in opens the browser                                                                                                                                                                      |
| `network.server` questioned                             | 2.4.5          | review notes: loopback-only sign-in listener (RFC 8252)                                                                                                                                                                                                                      |
| Generative AI content                                   | 1.1/1.2        | Report on every AI card (already there)                                                                                                                                                                                                                                      |
| Metadata                                                | 2.3            | real screenshots of this build, numbers from lexpad.app, no unsupported services                                                                                                                                                                                             |
| Crashes on review hardware                              | 2.1            | universal binary; `--local` sandboxed smoke on this Mac before upload                                                                                                                                                                                                        |

## Known limits of the sandboxed build (honest list)

- Episodes: the next episode plays only if it was chosen or dropped together with the first
  (or the whole folder was dropped). A later change can ask once for the folder with an open
  panel and keep a security-scoped bookmark (`files.bookmarks.app-scope`).
- Continue watching after a restart: the file must be opened again (same fix as above).
- Subtitles named differently from the film (Film.en.srt, a Subs folder): choose or drop them.
- A start by the login item has no `--login` flag, so it may open a window (the dmg opens none).
- "Use Lexpad for video files" calls a Launch Services function the sandbox may refuse; it then
  says it could not, and the learner can use Finder's Get Info › Open with.

## What is left before Submit

See the end of [mac-app-store-owner-steps.md](mac-app-store-owner-steps.md).
