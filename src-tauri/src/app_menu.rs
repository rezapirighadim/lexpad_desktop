//! Lexpad's two windows and how one reaches the other (spec
//! `uiux/specs/2026-10-25-video-player.md` §24.3): ⌘1 / Ctrl+1 is Lexpad,
//! ⌘2 / Ctrl+2 the Player, from anywhere in the app; Lexpad opens on Player
//! home unless the learner chose Today (`launch`); and on macOS the menu bar
//! mirrors the Player's keys in a Playback and a Subtitles menu, and the
//! Window menu lists both windows and Player home. Playback carries the
//! Player's line tools too (spec `uiux/2026-11-09-player-line-tools/spec.md`
//! §2.4), whose check marks and Say rows the page keeps true
//! (`player_menu_state`).
//!
//! One Dock tile on macOS, for both windows (decision in
//! `docs/player-dock-icon.md`): a tile of its own would need the Player as a
//! helper app inside Lexpad.app, with its own bundle, signing and IPC.

use serde::Deserialize;
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
    // The line tools (spec §2.4). No ⌥ accelerators either: in a menu of the
    // whole app ⌥T and ⌥S would take "†" and "ß" from every text field in
    // Lexpad's window; the Player's page answers those keys itself.
    (TYPE_LINE, "Type What You Hear", "typeLine", None),
    (TYPE_EVERY, "Type Every Line", "typeEvery", None),
    (SAY_LINE, "Say It Back", "sayLine", None),
    (SAY_EVERY, "Say Every Line Back", "sayEvery", None),
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

/// The line tools' menu ids (spec §2.4).
const TYPE_LINE: &str = "m.typeLine";
const TYPE_EVERY: &str = "m.typeEvery";
const SAY_LINE: &str = "m.sayLine";
const SAY_EVERY: &str = "m.sayEvery";

/// Whether the Player's page offers Say it back where it is now (spec §8:
/// not where the speech service is missing, for example). Hidden until the
/// page says otherwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SayRows {
    Shown,
    #[default]
    Hidden,
}

/// What the Playback menu's line tools show: the two check marks and whether
/// the Say rows are there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct LineTools {
    pub type_every: bool,
    pub say_every: bool,
    pub say: SayRows,
}

impl LineTools {
    /// Say Every Line Back is never shown checked while its row is away.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn say_every_checked(self) -> bool {
        self.say_every && self.say == SayRows::Shown
    }
}

/// The Playback menu's line-tool items, kept to change them later (macOS).
#[cfg(target_os = "macos")]
pub struct LineToolsMenu {
    playback: tauri::menu::Submenu<tauri::Wry>,
    type_every: tauri::menu::CheckMenuItem<tauri::Wry>,
    say_line: tauri::menu::MenuItem<tauri::Wry>,
    say_every: tauri::menu::CheckMenuItem<tauri::Wry>,
    /// What the menu shows now.
    now: std::sync::Mutex<LineTools>,
}

#[cfg(target_os = "macos")]
impl LineToolsMenu {
    fn apply(&self, want: LineTools) -> Result<(), String> {
        let mut now = self.now.lock().map_err(|_| "lock")?;
        let e = |e: tauri::Error| e.to_string();
        if want.say != now.say {
            match want.say {
                SayRows::Shown => {
                    // Right after Type Every Line, wherever that is now.
                    let at = self
                        .playback
                        .items()
                        .map_err(e)?
                        .iter()
                        .position(|i| i.id().as_ref() == TYPE_EVERY)
                        .ok_or("no Type Every Line in Playback")?
                        + 1;
                    self.playback
                        .insert_items(&[&self.say_line, &self.say_every], at)
                        .map_err(e)?;
                }
                SayRows::Hidden => {
                    self.playback.remove(&self.say_line).map_err(e)?;
                    self.playback.remove(&self.say_every).map_err(e)?;
                }
            }
        }
        self.type_every.set_checked(want.type_every).map_err(e)?;
        self.say_every
            .set_checked(want.say_every_checked())
            .map_err(e)?;
        *now = want;
        Ok(())
    }
}

/// The Player's page tells the menu bar what its line tools are now
/// (`invoke('player_menu_state', { typeEvery, sayEvery, say })`, `say` being
/// "shown" or "hidden"), whenever one of them changes. Windows has no menu
/// bar, so there it does nothing.
#[tauri::command]
pub fn player_menu_state(
    app: AppHandle,
    type_every: bool,
    say_every: bool,
    say: SayRows,
) -> Result<(), String> {
    set_line_tools(
        &app,
        LineTools {
            type_every,
            say_every,
            say,
        },
    )
}

/// Back to the first state when the Player closes: checks off, Say rows away.
pub fn player_closed(app: &AppHandle) {
    if let Err(e) = set_line_tools(app, LineTools::default()) {
        log::warn!("could not reset the Playback menu: {e}");
    }
}

fn set_line_tools(app: &AppHandle, want: LineTools) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if let Some(menu) = app.try_state::<LineToolsMenu>() {
        return menu.apply(want);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, want);
    Ok(())
}

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
    use tauri::menu::{AboutMetadata, CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
    let item =
        |id: &str, title: &str, accel: Option<&str>| MenuItem::with_id(app, id, title, true, accel);
    // An item's title and accelerator from PLAYER_ITEMS.
    fn entry(id: &str) -> (&str, Option<&str>) {
        PLAYER_ITEMS
            .iter()
            .find(|(i, ..)| *i == id)
            .map(|(_, title, _, accel)| (*title, *accel))
            .unwrap_or((id, None))
    }
    let player_item = |id: &str| -> tauri::Result<MenuItem<tauri::Wry>> {
        let (title, accel) = entry(id);
        item(id, title, accel)
    };
    let player_check = |id: &str| -> tauri::Result<CheckMenuItem<tauri::Wry>> {
        let (title, accel) = entry(id);
        CheckMenuItem::with_id(app, id, title, true, false, accel)
    };
    // The line tools: the Say rows are made now but join Playback only when
    // the page offers Say it back (LineToolsMenu::apply).
    let type_every = player_check(TYPE_EVERY)?;
    let say_line = player_item(SAY_LINE)?;
    let say_every = player_check(SAY_EVERY)?;
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
            &player_item(TYPE_LINE)?,
            &type_every,
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
    let menu = Menu::with_items(
        app,
        &[&lexpad, &file, &edit, &playback, &subtitles, &window],
    )?;
    app.manage(LineToolsMenu {
        playback,
        type_every,
        say_line,
        say_every,
        now: std::sync::Mutex::new(LineTools::default()),
    });
    Ok(menu)
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
            "typeLine",
            "typeEvery",
            "sayLine",
            "sayEvery",
        ];
        for (id, _, command, accel) in PLAYER_ITEMS {
            assert!(id.starts_with("m."));
            assert!(known.contains(command), "{command}");
            // Only modified keys in the menu: a bare key would be taken from text fields.
            assert!(accel.is_none_or(|a| a.contains("Cmd")), "{id}");
        }
    }

    /// The line tools (spec `uiux/2026-11-09-player-line-tools/spec.md`
    /// §2.4): right after Replay the Line, each sending its Player command,
    /// with no accelerator at all (⌥T and ⌥S would take "†" and "ß" from
    /// every text field).
    #[test]
    fn the_line_tools_follow_replay_with_no_accelerator() {
        let want = [
            ("m.typeLine", "Type What You Hear", "typeLine"),
            ("m.typeEvery", "Type Every Line", "typeEvery"),
            ("m.sayLine", "Say It Back", "sayLine"),
            ("m.sayEvery", "Say Every Line Back", "sayEvery"),
        ];
        let replay = PLAYER_ITEMS
            .iter()
            .position(|(id, ..)| *id == "m.replay")
            .unwrap();
        for (n, (id, title, command)) in want.into_iter().enumerate() {
            let (i, t, c, accel) = PLAYER_ITEMS[replay + 1 + n];
            assert_eq!((i, t, c), (id, title, command));
            assert_eq!(accel, None, "{id}");
        }
        // The two "every line" modes are the check items, the two Say rows
        // the ones that come and go (LineToolsMenu).
        assert_eq!((TYPE_EVERY, SAY_EVERY), ("m.typeEvery", "m.sayEvery"));
        assert_eq!((SAY_LINE, SAY_EVERY), ("m.sayLine", "m.sayEvery"));
    }

    #[test]
    fn the_page_names_the_say_rows_shown_or_hidden() {
        let parse = |s: &str| serde_json::from_str::<SayRows>(s);
        assert_eq!(parse("\"shown\"").unwrap(), SayRows::Shown);
        assert_eq!(parse("\"hidden\"").unwrap(), SayRows::Hidden);
        assert!(parse("\"locked\"").is_err());
        assert!(parse("\"Shown\"").is_err());
        // Before the page says anything: no check marks, no Say rows.
        let first = LineTools::default();
        assert_eq!(first.say, SayRows::Hidden);
        assert!(!first.type_every && !first.say_every);
        // Say Every Line Back is not left checked on a row that is away.
        let away = LineTools {
            type_every: true,
            say_every: true,
            say: SayRows::Hidden,
        };
        assert!(!away.say_every_checked());
        assert!(LineTools {
            say: SayRows::Shown,
            ..away
        }
        .say_every_checked());
    }
}
