//! Lexpad's two windows and how one reaches the other (spec
//! `uiux/specs/2026-10-25-video-player.md` §24.3): ⌘1 / Ctrl+1 is Lexpad,
//! ⌘2 / Ctrl+2 the Player, from anywhere in the app; Lexpad opens on Player
//! home unless the learner chose Today (`launch`); and on macOS the menu bar
//! mirrors the Player's keys in a Playback and a Subtitles menu, and the
//! Window menu lists both windows and Player home.
//!
//! One Dock tile on macOS, for both windows (decision in
//! `docs/player-dock-icon.md`): a tile of its own would need the Player as a
//! helper app inside Lexpad.app, with its own bundle, signing and IPC.

use tauri::{AppHandle, Emitter, Manager};

use crate::main_window;
use crate::player_window::{self, Door, Opened};
use crate::settings::{OpenAt, Place};
use crate::state::AppState;

/// Remembers which window was last in front.
pub fn note_focus(app: &AppHandle, label: &str) {
    let place = match label {
        main_window::LABEL => Place::Learn,
        player_window::LABEL => Place::Player,
        _ => return,
    };
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let same = state
        .settings
        .lock()
        .map(|s| s.last_place == place)
        .unwrap_or(true);
    if !same {
        let _ = state.update_settings(|s| s.last_place = place);
    }
}

/// Settings → This computer → "When I open Lexpad". The only writer of
/// `open_at_set`: a value set here is the learner's choice.
#[tauri::command]
pub fn set_open_at(app: AppHandle, open_at: String) -> Result<String, String> {
    let value = OpenAt::parse(&open_at).ok_or("not a place")?;
    app.state::<AppState>()
        .update_settings(|s| choose_open_at(s, value))?;
    Ok(value.as_str().into())
}

/// What the Settings control writes: the place, and that it was chosen.
pub fn choose_open_at(s: &mut crate::settings::Settings, value: OpenAt) {
    s.open_at = value;
    s.open_at_set = true;
}

/// What the learning window's Settings shows for it.
#[tauri::command]
pub fn open_at(app: AppHandle) -> String {
    let at = app
        .try_state::<AppState>()
        .and_then(|s| s.settings.lock().ok().map(|s| s.open_at))
        .unwrap_or_default();
    at.as_str().into()
}

/// A menu command for the Player: brought forward (opened if need be), then told.
fn to_player(app: &AppHandle, command: &str) {
    if app.get_webview_window(player_window::LABEL).is_none() {
        player_window::open(app, Opened::App, Some(Door::Menu));
        if command == "open" {
            // A fresh Player opens the dialog itself once it listens (the
            // same opening: its door is counted once).
            player_window::open(app, Opened::Pick, None);
            return;
        }
    } else {
        player_window::open(app, Opened::App, Some(Door::Menu));
    }
    let _ = app.emit_to(player_window::LABEL, "player:menu", command);
}

/// The menu's ids, and the Player command each sends.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const PLAYER_ITEMS: &[(&str, &str, &str, Option<&str>)] = &[
    // id, title, Player command, accelerator (⌘ ones only: a bare key in a
    // menu would be taken from every text field)
    ("m.open", "Open Video…", "open", Some("CmdOrCtrl+O")),
    ("m.play", "Play or Pause", "playPause", None),
    ("m.back", "Back 5 Seconds", "back5", None),
    ("m.forward", "Forward 5 Seconds", "forward5", None),
    ("m.replay", "Replay the Line", "replayLine", None),
    ("m.slower", "Slower", "slower", None),
    ("m.faster", "Faster", "faster", None),
    ("m.normal", "Normal Speed", "normalSpeed", None),
    ("m.next", "Next Episode", "nextEpisode", None),
    ("m.previous", "Previous Episode", "previousEpisode", None),
    ("m.mini", "Mini Player", "mini", None),
    ("m.subs", "Show or Hide Subtitles", "toggleSubtitles", None),
    ("m.sub1", "Subtitle 1", "toggleTrack1", None),
    ("m.sub2", "Subtitle 2", "toggleTrack2", None),
    ("m.sub3", "Subtitle 3", "toggleTrack3", None),
    ("m.panel", "Subtitles Panel", "subtitlesPanel", None),
    (
        "m.earlier",
        "Subtitles 0.1 s Earlier",
        "timingEarlier",
        None,
    ),
    ("m.later", "Subtitles 0.1 s Later", "timingLater", None),
    ("m.heard", "I Just Heard This Line…", "heard", None),
    ("m.transcript", "Transcript", "transcript", None),
    ("m.shortcuts", "Keyboard Shortcuts", "shortcuts", None),
    // Player home (spec B2): the page's own key is ⇧⌘H / Ctrl+Shift+H too.
    ("m.home", "Home", "home", Some("CmdOrCtrl+Shift+H")),
];

/// The Player's right-click menu tags its items' ids with this, followed by
/// the Player command the item sends (`player_window::player_context_menu`).
pub const CONTEXT_PREFIX: &str = "ctx:";

/// Handles the app menu's own items (ids starting "m.") and the Player's
/// right-click menu ("ctx:"); everything else (the tray's menu) is left to
/// its own handler.
pub fn handle(app: &AppHandle, id: &str) {
    if let Some(command) = id
        .strip_prefix(CONTEXT_PREFIX)
        .and_then(player_window::context_command)
    {
        let _ = app.emit_to(player_window::LABEL, "player:menu", command);
        return;
    }
    match id {
        "m.lexpad" => main_window::open(app, None),
        "m.player" => player_window::open(app, Opened::App, Some(Door::Menu)),
        "m.settings" => {
            let player_in_front = app
                .get_webview_window(player_window::LABEL)
                .and_then(|w| w.is_focused().ok())
                .unwrap_or(false);
            if player_in_front {
                let _ = app.emit_to(player_window::LABEL, "player:menu", "settings");
            } else {
                crate::open_desktop_settings(app);
            }
        }
        _ => {
            if let Some((_, _, command, _)) = PLAYER_ITEMS.iter().find(|(i, ..)| *i == id) {
                to_player(app, command);
            }
        }
    }
}

/// The macOS menu bar: Lexpad, File (Open Video… ⌘O), Edit (so text fields
/// copy and paste), Playback, Subtitles and Window (Lexpad ⌘1, Lexpad Player ⌘2).
#[cfg(target_os = "macos")]
pub fn build(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
    let item =
        |id: &str, title: &str, accel: Option<&str>| MenuItem::with_id(app, id, title, true, accel);
    let player_item = |id: &str| -> tauri::Result<MenuItem<tauri::Wry>> {
        let (_, title, _, accel) = PLAYER_ITEMS
            .iter()
            .find(|(i, ..)| *i == id)
            .copied()
            .unwrap_or((id, id, "", None));
        item(id, title, accel)
    };
    let lexpad = Submenu::with_items(
        app,
        "Lexpad",
        true,
        &[
            &PredefinedMenuItem::about(
                app,
                Some("About Lexpad"),
                Some(AboutMetadata {
                    // Player home's clip and picture: CC BY 3.0 asks for the credit.
                    credits: Some(
                        "Player home's clip and picture: Tears of Steel (CC) Blender Foundation | mango.blender.org, CC BY 3.0."
                            .into(),
                    ),
                    ..AboutMetadata::default()
                }),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &item("m.settings", "Settings…", Some("CmdOrCtrl+,"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some("Hide Lexpad"))?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some("Quit Lexpad"))?,
        ],
    )?;
    let file = Submenu::with_items(app, "File", true, &[&player_item("m.open")?])?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let playback = Submenu::with_items(
        app,
        "Playback",
        true,
        &[
            &player_item("m.play")?,
            &player_item("m.back")?,
            &player_item("m.forward")?,
            &player_item("m.replay")?,
            &PredefinedMenuItem::separator(app)?,
            &player_item("m.slower")?,
            &player_item("m.faster")?,
            &player_item("m.normal")?,
            &PredefinedMenuItem::separator(app)?,
            &player_item("m.next")?,
            &player_item("m.previous")?,
            &PredefinedMenuItem::separator(app)?,
            &player_item("m.mini")?,
            &player_item("m.shortcuts")?,
        ],
    )?;
    let subtitles = Submenu::with_items(
        app,
        "Subtitles",
        true,
        &[
            &player_item("m.subs")?,
            &player_item("m.sub1")?,
            &player_item("m.sub2")?,
            &player_item("m.sub3")?,
            &PredefinedMenuItem::separator(app)?,
            &player_item("m.earlier")?,
            &player_item("m.later")?,
            &player_item("m.heard")?,
            &PredefinedMenuItem::separator(app)?,
            &player_item("m.panel")?,
            &player_item("m.transcript")?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::fullscreen(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &item("m.lexpad", "Lexpad", Some("CmdOrCtrl+1"))?,
            &item("m.player", "Lexpad Player", Some("CmdOrCtrl+2"))?,
            &player_item("m.home")?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;
    Menu::with_items(
        app,
        &[&lexpad, &file, &edit, &playback, &subtitles, &window],
    )
}

/// Windows has no menu bar: Ctrl+1 and Ctrl+2 are answered by each window's
/// page, which asks for the other window through these.
#[tauri::command]
pub fn open_window(app: AppHandle, which: String) -> Result<(), String> {
    match which.as_str() {
        "lexpad" => main_window::open(&app, None),
        "player" => player_window::open(&app, Opened::App, Some(Door::Menu)),
        _ => return Err("not a window".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_control_marks_its_value_as_chosen() {
        let mut s = crate::settings::Settings::default();
        assert!(!s.open_at_set);
        choose_open_at(&mut s, OpenAt::Today);
        assert_eq!(s.open_at, OpenAt::Today);
        assert!(s.open_at_set);
        // Choosing the default is a choice too: a later default keeps away.
        choose_open_at(&mut s, OpenAt::Player);
        assert_eq!(s.open_at, OpenAt::Player);
        assert!(s.open_at_set);
    }

    #[test]
    fn every_menu_command_is_one_the_player_knows() {
        // The Player's command names (front: features/player/model/keys.ts).
        let known = [
            "open",
            "playPause",
            "back5",
            "forward5",
            "replayLine",
            "slower",
            "faster",
            "normalSpeed",
            "nextEpisode",
            "previousEpisode",
            "mini",
            "toggleSubtitles",
            "toggleTrack1",
            "toggleTrack2",
            "toggleTrack3",
            "subtitlesPanel",
            "timingEarlier",
            "timingLater",
            "heard",
            "transcript",
            "shortcuts",
            "home",
        ];
        for (id, _, command, accel) in PLAYER_ITEMS {
            assert!(id.starts_with("m."));
            assert!(known.contains(command), "{command}");
            // Only modified keys in the menu: a bare key would be taken from text fields.
            assert!(accel.is_none_or(|a| a.contains("Cmd")), "{id}");
        }
    }
}
