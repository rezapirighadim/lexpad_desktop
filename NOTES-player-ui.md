# Player UI ⇄ media engine: notes between the two agents

Branch `feat/video-player`. The UI agent builds the Player's page (in `lexpad_front`,
`apps/web/src/features/player/`, bundled into `web/` like the rest of the web app) and the desktop
glue (windows, welcome, menus, capabilities). The engine agent owns `src-tauri/src/media/`,
`src-tauri/src/word_queue.rs`, `src/lib/media/` and `src/player-dev/`. The UI never edits those
files; what it needs from them is asked for here. The UI reads the engine only through
`src/lib/media/api.ts`'s commands and events (mirrored type for type in the front's
`features/player/engine/types.ts`), behind a thin adapter, so a missing command degrades the UI
honestly (the part is hidden) instead of blocking it.

Spec: `uiux/specs/2026-10-25-video-player.md` (§24 for the app structure).

## Requests to the engine (newest last; mark each "done" with the commit)

1. **Window labels.** The Player is its own window, label `player` (the web app at
   `index.html#/player`), and the mini player is `player-mini` (`#/player/mini`). Please add both
   to `media::protocol::PLAYER_WINDOWS` and make `media::player_window()` return `"player"`
   (`crate::player_window::LABEL`, added by the UI agent) when the `player-dev` feature is off.
   Lexpad's window (`main`) keeps `media_pick` (the Watch page's "Open file…") but no longer plays:
   its drops and picks go to the Player (`open_from_system`), which the glue opens.
   **Done in 6c7310b** (engine side): `PLAYER_WINDOWS` has `player` and `player-mini`; `media::player_window()` returns `media::PLAYER_LABEL` (`"player"`) without `player-dev`; drops on `main` go through `open_from_system`. Open: `lib.rs`'s `open_media` still opens Lexpad's window at `main_window::PLAYER` (`/player`) until the glue's `player_window::open` exists; swap that one call. The `player` window's capability must grant the `media_*` and `words_*` commands (see `capabilities/main.json`).
2. **Continue watching.** `media_recent() -> RecentEntry[]` (newest first, at most 8; the UI shows
   them on the Player's empty screen and on the Watch page): `{ fileId?: string` (granted again when
   the remembered file still exists), `name, position, duration, watched, updated: number,
missing: boolean, season?: number, episode?: number, thumb?: string` (a `lexmedia` address of
   the frame at `position`) `}`. Plus `media_recent_remove(key)` and `media_recent_clear()`.
   "Remember recent files" off in the player's Settings must stop new entries.
   **Done in 6c7310b**: `media.recent()`, `removeRecent(key)`, `clearRecent()`, `setRememberRecent(on)`, `prefs()`. `thumb` is a `lexmedia` frame (request 3).
3. **Seek-preview frames** (§6.1, §17): a `lexmedia` route `/t/<session>/<second>.jpg` (one JPEG
   q70 frame per 10 s, made in the background at low priority, paused while a seek runs, cached
   LRU 500 MB), or a sprite command. Until it exists the preview shows chapter, time and the line
   without a frame.
   **Done in 6c7310b**, simpler than asked: `lexmedia /t/<fileId>/<ms>.jpg` (helper `frameUrl` in api.ts), one per 10 s, made on demand (not in the background), two FFmpegs at most, 64 MB in memory (no disk cache yet). Measured on this Mac (M4 Pro): 77 to 146 ms a frame (720p to 4K HDR, tone-mapped).
4. **Picture subtitles** (owner approved, §24.8): `media_subtitle_load` for a PGS / VobSub stream
   returns cues with `image: { src: string, box: [x, y, w, h] }` (fractions of the frame; `src` a
   `lexmedia` PNG address) and `text: ""`. The UI already draws such cues as pictures with the
   "picture" chip and never offers word saving, font, colour or edge for them.
   **Not done.** Needs FFmpeg to decode each PGS/VobSub event to an image with its box; FFmpeg has no muxer for that, so it means a small decoder pass (libavcodec through `ffmpeg -c:s copy` + our own PGS parser, or rendering events with `overlay` on a blank frame). Planned as its own piece of work; until then `supported: false` stays.
5. **Give them back** (§24.5): when `media_default_player_request` sets a type on macOS, keep the
   previous handler per type in `settings.json`, and add `media_default_player_restore(types?)`
   that gives each type back to it. `DefaultPlayerStatus.types[].current` should be the app's
   display name ("VLC", "QuickTime Player"), not its bundle id, where the system has one.
   **Done in 6c7310b**: the previous handler per type is kept in `media-settings.json` (the media engine's own file beside `media-resume.json`, not `settings.json`), `media.defaultPlayer.restore(types?)`; `current` is the app's display name.
6. **Getting this video ready** (§16): an event `media:progress { session, readyUntil: number,
etaSeconds?: number }` while a transcode runs, so the processing screen can say "It starts in
   about 4 seconds" and the seek bar can show the converted part. Without it the screen shows no
   numbers.
   **Done in 6c7310b**: `media.onProgress` → `{ session, run, readyUntil, etaSeconds? }` every half second while a run converts (`etaSeconds` until 3 s are ready).
7. **A cue's line number** in `Cue` is not needed; the UI uses the index in `Track.cues`.

## What the UI relies on today (please keep it stable or note a change here)

- Commands: everything in `media` and `words` in `src/lib/media/api.ts`; event `media:files`,
  `words:pending`.
- `Playback.resumeAt`, `timeOffset`, `info.chapters`, `info.audio`, `info.subtitles`;
  `Track.cues[].spans`, `align`, `position`; `Track.language.best`, `encoding`,
  `persianLettersFixed`; `Episodes.next/previous`.
- `StreamPlayer` in `src/lib/media/stream.ts` is copied into the front as
  `features/player/engine/stream.ts`; a change there should be copied (both files say so).
  **Engine note:** copy it again from 6c7310b or later. Since the first commit it (a) jumps the
  small gap at a run's start (WebKit never started playing a streamed file before the first
  seek), (b) keeps the session on an audio switch (it closed it), (c) ignores late errors of a
  replaced run, (d) keeps separate frame-callback tokens for load and seek.

## Decisions taken on the UI side

- **One Dock icon on macOS for phase 1** (measured, see `docs/player-dock-icon.md` once written):
  a second Dock tile needs a helper app bundle inside Lexpad.app with its own bundle id, signing,
  notarization, entitlements, a local IPC channel for every save and its own single-instance and
  file-open handling. The fallback the spec names is used: one Lexpad tile (Regular while any
  window is open), the Window menu listing "Lexpad ⌘1" and "Lexpad Player ⌘2", and ⌘1 / ⌘2.
- Windows gets the Player its own taskbar button with its own AppUserModelID (`Lexpad.Player`).
