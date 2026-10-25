# The Player's Dock icon on macOS (phase 1 decision, 25 Oct 2026)

The owner approved "the player gets its own Dock icon while open" (spec
`uiux/specs/2026-10-25-video-player.md` §24.3, Q1). On Windows that is cheap and done: the
Player's window gets its own AppUserModelID (`Lexpad.Player`) and taskbar button
(`src-tauri/src/player_window.rs`, `taskbar::own_button`).

On macOS one process has exactly one Dock tile. A second tile means a second app.

## What a second tile costs (measured against this repository)

| Piece                    | Work                                                                                                                                                                                                                                                                                                                           |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A helper app bundle      | `Lexpad.app/Contents/Helpers/Lexpad Player.app` with its own Info.plist, bundle id, icon set (`scripts/icons.py`) and `LSUIElement` off; Tauri builds one bundle per config, so a second Tauri app (its own `tauri.conf.json`, `src-tauri` crate or binary target) and a post-build step that places it inside the main bundle |
| Signing and notarization | `scripts/release-mac.sh` signs and staples one app; a nested app is signed first, with its own entitlements (the webview, the network), and the outer bundle after; notarization covers both                                                                                                                                   |
| The session              | The helper holds no tokens (hard rule: the core is the only holder), so every save, lookup and notebook read goes to the main app over a local IPC channel (an authenticated Unix socket or XPC), with its own tests and failure states                                                                                        |
| The engine               | The media engine (FFmpeg runs, the `lexmedia` scheme, file grants) lives in the main process today; it either moves into the helper (and the main window's Watch page asks the helper) or is served across the IPC as well                                                                                                     |
| Files and launch         | "Open with" and the default-player types belong to one bundle; the helper needs its own document types or a hand-off from the main app; single-instance and second-launch rules for both                                                                                                                                       |
| Updates and the Store    | Every release ships and checks two apps; nothing else in the Mac build changes                                                                                                                                                                                                                                                 |

That is several weeks of work with new failure modes (IPC, two signatures), against a visible
gain of one icon. **Phase 1 uses the fallback the spec names**: one Lexpad tile, Regular while
either window is open (`refresh_dock`), the Window menu listing "Lexpad ⌘1" and "Lexpad
Player ⌘2", and ⌘1 / ⌘2 from anywhere (`src-tauri/src/app_menu.rs`). The Player's own icon is
drawn in the mock-up `33-two-windows` and waits for the helper app in a later phase.
