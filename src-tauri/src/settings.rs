//! The app's own settings, in `settings.json` in the app's config folder.
//! Nothing secret lives here: the session is in the credential store.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The shortcut on first run: ⌘⇧L on a Mac, Ctrl+Shift+L on Windows. The
/// player test build takes ⌘⌥⇧L / Ctrl+Alt+Shift+L, so it never fights an
/// installed Lexpad for the same keys.
pub const DEFAULT_SHORTCUT: &str = if crate::config::PLAYER_TEST {
    "CommandOrControl+Alt+Shift+L"
} else {
    "CommandOrControl+Shift+L"
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Identifies this install to the API, so signing in again replaces this
    /// computer's old session instead of listing it twice.
    pub device_id: String,
    pub shortcut: String,
    /// Start Lexpad when the learner logs in. Off on a new install (spec
    /// `uiux/specs/2026-10-26-desktop-first-run.md` §6: nothing in the
    /// background unasked, no "Background Items Added" on the first run); it
    /// is offered in Settings → This computer. A file written by an older
    /// version without the field keeps what it meant then: on.
    #[serde(default = "on")]
    pub start_on_login: bool,
    /// The notebook words go to, unless the word cannot be in its language.
    pub notebook_id: Option<String>,
    /// The last words added from this computer, newest first, for the panel.
    /// Each names the account it was added to, so another account signed in
    /// on the same computer never sees them.
    pub recent: Vec<RecentWord>,
    /// Open Lexpad's window when the app starts (at login, or by hand).
    pub open_on_launch: bool,
    /// "Open Lexpad" and a word opened from the panel go to the web app in
    /// the browser instead of this app's own window.
    pub open_in_browser: bool,
    /// Where Lexpad's window was when it last closed: its content rectangle
    /// in placement units (`placement`), and whether it filled the screen.
    pub main_window: Option<WindowPlace>,
    /// Notifications on this computer (the daily reminder and the server's
    /// messages). The account's reminder time is the account's; this only
    /// silences this computer.
    pub notifications: bool,
    /// What this computer remembers about the inbox (`notify::pick`).
    pub inbox: crate::notify::InboxMemory,
    /// The version that last found Accessibility allowed. An unsigned build
    /// has a new code signature each release, and macOS then no longer
    /// applies the old grant though the switch still looks on: missing
    /// after it was allowed in another version is that, not a refusal.
    pub accessibility_granted_in: Option<String>,
    /// What the learner came for, inferred from what they did first: a
    /// video opened is `watch`, `learn` or `both` were written by the Start
    /// window of 0.3.x (retired 8 Nov 2026) and are kept as they are. It only
    /// changes tips and is sent with the onboarding answers; it locks nothing.
    /// None until known.
    pub focus: Option<Focus>,
    /// The window that was last in front: where "Open Lexpad" goes back to.
    pub last_place: Place,
    /// Settings → This computer → "When I open Lexpad".
    pub open_at: OpenAt,
    /// `open_at` was chosen by the learner in Settings, not left at its
    /// default. Written `true` only by that control (and by the migration of
    /// a file whose value could only have come from it), so a later change of
    /// the default never overrides a choice (Player spec B1, 8 Nov 2026).
    pub open_at_set: bool,
    /// The one-time note on Player home, "Lexpad now opens on the Player.
    /// Prefer Today? Change it in Settings", is due: set by the migration of
    /// an install that opened where it left off, cleared once the note showed.
    pub player_home_note: bool,
    /// Where the Player's window was when it last closed.
    pub player_window: Option<WindowPlace>,
    /// A video has been opened on this computer.
    pub played_video: bool,
    /// "Lexpad is still here" was shown once, when the last window closed (§6).
    pub tray_hint_shown: bool,
    /// The account (its id) that the API said has used a Lexpad phone app.
    /// The server never takes that back (`everUsed`), so once it is known
    /// the tray says "Lexpad everywhere…" without asking again, offline too.
    /// Another account signed in here is asked for afresh (`everywhere`).
    pub phone_used_by: Option<String>,
    /// The shortcut card's hint when a video app is in front: when each
    /// app's showed, and how many in all (`video_hint`).
    pub video_hint: crate::video_hint::VideoHint,
}

fn on() -> bool {
    true
}

/// Whether "Lexpad is still here" is due now (Start window spec §6): only
/// when no Lexpad window is left open, and only until it has once been shown
/// on this computer (`tray_hint_shown`, set once it really showed).
pub fn tray_hint_due(settings: &Settings, any_window_open: bool, quitting: bool) -> bool {
    !quitting && !any_window_open && !settings.tray_hint_shown
}

/// What a start does to the login item: register it when the learner turned
/// it on (again each start, so it carries `--login` after an update), remove
/// one an old install left behind when a new settings file says off, and
/// otherwise leave the system alone.
pub fn login_item_action(start_on_login: bool, first_run: bool) -> Option<bool> {
    if start_on_login {
        Some(true)
    } else if first_run {
        Some(false)
    } else {
        None
    }
}

/// What the learner came for, as their first actions say (Start window spec §3).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Focus {
    Watch,
    Learn,
    Both,
}

/// One of Lexpad's two windows.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Place {
    #[default]
    Learn,
    Player,
}

/// "When I open Lexpad": Player home (the default since 8 Nov 2026, owner
/// QB1) or Today. "Where I left off" (`leftOff`, the old default) is read as
/// the Player; `SettingsFile::load` migrates it and says so once.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OpenAt {
    Today,
    #[default]
    #[serde(alias = "leftOff")]
    Player,
}

impl OpenAt {
    /// The word the windows' pages use for it.
    pub fn as_str(self) -> &'static str {
        match self {
            OpenAt::Today => "today",
            OpenAt::Player => "player",
        }
    }

    /// The control's word; "leftOff" is an older page's, read as the Player.
    pub fn parse(word: &str) -> Option<OpenAt> {
        match word {
            "today" => Some(OpenAt::Today),
            "player" | "leftOff" => Some(OpenAt::Player),
            _ => None,
        }
    }
}

/// Brings a settings file from before `openAtSet` (0.3.29 and older) to the
/// launch of 8 Nov 2026 (owner QB1: every launch opens Player home). Such a
/// file always held `openAt`, chosen or not, because the old default was
/// written too; only a value other than that default can have come from the
/// learner. So `today` and `player` stay and count as chosen, and a missing
/// or `leftOff` value becomes the Player, with the one-time note. Returns
/// whether anything changed (the file is then written back).
pub fn migrate(raw: &serde_json::Value, s: &mut Settings) -> bool {
    let Some(map) = raw.as_object() else {
        return false;
    };
    if map.contains_key("openAtSet") {
        return false;
    }
    match map.get("openAt").and_then(|v| v.as_str()) {
        Some("today") => {
            s.open_at = OpenAt::Today;
            s.open_at_set = true;
        }
        Some("player") => {
            s.open_at = OpenAt::Player;
            s.open_at_set = true;
        }
        _ => {
            s.open_at = OpenAt::Player;
            s.open_at_set = false;
            s.player_home_note = true;
        }
    }
    true
}

/// A window's place on screen, remembered between runs.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowPlace {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub maximized: bool,
}

/// How many recently added words the panel shows. A handful is what fits
/// under the "Add a word" box without scrolling; the notebook has the rest.
pub const RECENT_MAX: usize = 5;

/// A word added from this computer: enough to list it and open it in the web app.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecentWord {
    pub id: String,
    pub headword: String,
    pub notebook_id: String,
    pub user_id: String,
    /// Milliseconds since the Unix epoch.
    pub added_at: u64,
}

/// `list` with `word` first, without an older entry for the same word, and
/// no longer than RECENT_MAX.
pub fn remember(list: &[RecentWord], word: RecentWord) -> Vec<RecentWord> {
    let mut out = Vec::with_capacity(RECENT_MAX);
    out.push(word.clone());
    out.extend(
        list.iter()
            .filter(|w| {
                w.id != word.id
                    && !(w.user_id == word.user_id
                        && w.notebook_id == word.notebook_id
                        && w.headword == word.headword)
            })
            .cloned(),
    );
    out.truncate(RECENT_MAX);
    out
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_id: uuid::Uuid::new_v4().to_string(),
            shortcut: DEFAULT_SHORTCUT.into(),
            start_on_login: false,
            notebook_id: None,
            recent: Vec::new(),
            open_on_launch: false,
            open_in_browser: false,
            main_window: None,
            notifications: true,
            inbox: crate::notify::InboxMemory::default(),
            accessibility_granted_in: None,
            focus: None,
            last_place: Place::Learn,
            open_at: OpenAt::Player,
            open_at_set: false,
            player_home_note: false,
            player_window: None,
            played_video: false,
            tray_hint_shown: false,
            phone_used_by: None,
            video_hint: Default::default(),
        }
    }
}

/// Reads and writes `settings.json`.
pub struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    pub fn new(dir: &Path) -> Self {
        Self {
            path: dir.join("settings.json"),
        }
    }

    /// The saved settings, and whether this is the first run (no file yet).
    /// A file from an older version is migrated (`migrate`) and written back.
    pub fn load(&self) -> (Settings, bool) {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => {
                let (settings, migrated) = parse(&text);
                if migrated {
                    if let Err(e) = self.save(&settings) {
                        log::warn!("could not write the migrated settings: {e}");
                    }
                }
                (settings, false)
            }
            Err(_) => (Settings::default(), true),
        }
    }

    pub fn save(&self, settings: &Settings) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
        // Write then rename, so a crash mid-write never leaves half a file.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}

/// A settings file's text, migrated, and whether the migration changed it.
pub fn parse(text: &str) -> (Settings, bool) {
    let raw: serde_json::Value = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
    let mut settings: Settings = serde_json::from_value(raw.clone()).unwrap_or_default();
    let migrated = migrate(&raw, &mut settings);
    (settings, migrated)
}

/// Whether a missing Accessibility permission is one macOS dropped because
/// the app changed (an update of an unsigned build), rather than one the
/// learner never gave or took back: it was allowed in another version.
pub fn stale_accessibility(
    now: crate::capture::Permission,
    granted_in: Option<&str>,
    version: &str,
) -> bool {
    now == crate::capture::Permission::Missing && granted_in.is_some_and(|v| v != version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_has_defaults_and_a_device_id_that_sticks() {
        let dir = std::env::temp_dir().join(format!("lexpad-settings-{}", uuid::Uuid::new_v4()));
        let file = SettingsFile::new(&dir);
        let (first, is_first) = file.load();
        assert!(is_first);
        // Nothing starts at login until the learner asks (Start window spec §6).
        assert!(!first.start_on_login);
        assert!(!first.played_video && !first.tray_hint_shown);
        // A new install opens Player home, with no note: nothing changed for it.
        assert_eq!(first.open_at, OpenAt::Player);
        assert!(!first.open_at_set && !first.player_home_note);
        assert_eq!(first.shortcut, DEFAULT_SHORTCUT);
        file.save(&first).unwrap();
        let (again, is_first) = file.load();
        assert!(!is_first);
        assert_eq!(again.device_id, first.device_id);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn word(id: &str, headword: &str, user: &str) -> RecentWord {
        RecentWord {
            id: id.into(),
            headword: headword.into(),
            notebook_id: "n1".into(),
            user_id: user.into(),
            added_at: 1,
        }
    }

    #[test]
    fn recent_words_are_newest_first_without_repeats_and_at_most_five() {
        let mut list = Vec::new();
        for (i, w) in ["a", "b", "c", "d", "e", "f"].iter().enumerate() {
            list = remember(&list, word(&i.to_string(), w, "u1"));
        }
        let heads: Vec<_> = list.iter().map(|w| w.headword.as_str()).collect();
        assert_eq!(heads, ["f", "e", "d", "c", "b"]);
        // The same word again moves to the top instead of showing twice.
        list = remember(&list, word("9", "d", "u1"));
        let heads: Vec<_> = list.iter().map(|w| w.headword.as_str()).collect();
        assert_eq!(heads, ["d", "f", "e", "c", "b"]);
        // Another account's word of the same spelling is its own entry.
        list = remember(&list, word("10", "d", "u2"));
        assert_eq!(list.iter().filter(|w| w.headword == "d").count(), 2);
    }

    #[test]
    fn a_file_from_an_older_version_keeps_what_it_has() {
        let s: Settings = serde_json::from_str(r#"{"deviceId":"d1","shortcut":"Alt+L"}"#).unwrap();
        assert_eq!(s.device_id, "d1");
        assert_eq!(s.shortcut, "Alt+L");
        assert!(s.start_on_login);
        assert!(s.recent.is_empty());
        // 0.1 had no window of its own: nothing opens by itself, Open Lexpad
        // opens the window, and the window starts where the app chooses.
        assert!(!s.open_on_launch);
        assert!(!s.open_in_browser);
        assert!(s.main_window.is_none());
        assert!(s.notifications);
        assert!(s.accessibility_granted_in.is_none());
        assert!(!s.played_video && !s.tray_hint_shown);
    }

    #[test]
    fn lexpad_is_still_here_shows_once_and_only_with_every_window_shut() {
        let mut s = Settings::default();
        // A window still open: not yet.
        assert!(!tray_hint_due(&s, true, false));
        // Quitting closes every window: never then.
        assert!(!tray_hint_due(&s, false, true));
        // The last window closed: due.
        assert!(tray_hint_due(&s, false, false));
        // Once it showed it is marked, and never due again, also after a restart.
        s.tray_hint_shown = true;
        assert!(!tray_hint_due(&s, false, false));
        let again: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert!(!tray_hint_due(&again, false, false));
    }

    #[test]
    fn the_login_item_is_only_touched_when_asked_or_left_behind() {
        assert_eq!(login_item_action(true, false), Some(true));
        assert_eq!(login_item_action(true, true), Some(true));
        // A first run over an old install's login item takes it away.
        assert_eq!(login_item_action(false, true), Some(false));
        // Otherwise the system is left alone.
        assert_eq!(login_item_action(false, false), None);
    }

    #[test]
    fn an_install_that_had_start_on_login_keeps_its_choice() {
        let on: Settings = serde_json::from_str(r#"{"startOnLogin":true}"#).unwrap();
        assert!(on.start_on_login);
        let off: Settings = serde_json::from_str(r#"{"startOnLogin":false}"#).unwrap();
        assert!(!off.start_on_login);
        let new = Settings::default();
        assert!(!new.start_on_login);
    }

    /// Settings files as 0.3.29 (and 0.2.4, before "When I open Lexpad")
    /// wrote them, with their own fields only (`fixtures/settings`).
    const OLD_LEFT_OFF: &str = include_str!("../fixtures/settings/0.3.29-left-off.json");
    const OLD_TODAY: &str = include_str!("../fixtures/settings/0.3.29-today.json");
    const OLD_PLAYER: &str = include_str!("../fixtures/settings/0.3.29-player.json");
    const OLDER_NONE: &str = include_str!("../fixtures/settings/0.2.4-no-open-at.json");

    #[test]
    fn an_old_default_becomes_player_home_with_the_note_once() {
        for text in [OLD_LEFT_OFF, OLDER_NONE] {
            let (s, migrated) = parse(text);
            assert!(migrated);
            assert_eq!(s.open_at, OpenAt::Player);
            // Never chosen: a later default may still move it.
            assert!(!s.open_at_set);
            assert!(s.player_home_note);
            // Everything else the file held is kept.
            assert_eq!(s.device_id, "7d1f3c2e-0000-4000-8000-000000000001");
            assert!(s.start_on_login);
            // Written back, the file is not migrated again: the note comes once.
            let again = serde_json::to_string(&s).unwrap();
            let (s2, migrated2) = parse(&again);
            assert!(!migrated2);
            assert_eq!(s2, s);
        }
    }

    #[test]
    fn a_place_the_learner_chose_stays_and_counts_as_chosen() {
        let (s, migrated) = parse(OLD_TODAY);
        assert!(migrated);
        assert_eq!(s.open_at, OpenAt::Today);
        assert!(s.open_at_set && !s.player_home_note);
        let (s, _) = parse(OLD_PLAYER);
        assert_eq!(s.open_at, OpenAt::Player);
        assert!(s.open_at_set && !s.player_home_note);
    }

    #[test]
    fn a_new_file_is_not_migrated_and_a_chosen_value_is_told_from_the_default() {
        let fresh = serde_json::to_string(&Settings::default()).unwrap();
        let (s, migrated) = parse(&fresh);
        assert!(!migrated);
        assert!(!s.open_at_set && !s.player_home_note);
        // The Settings control writes the choice and the mark together.
        let chosen = Settings {
            open_at: OpenAt::Today,
            open_at_set: true,
            ..Settings::default()
        };
        let (s, migrated) = parse(&serde_json::to_string(&chosen).unwrap());
        assert!(!migrated);
        assert_eq!(s.open_at, OpenAt::Today);
        assert!(s.open_at_set);
    }

    #[test]
    fn the_migrated_file_is_written_back_by_load() {
        let dir = std::env::temp_dir().join(format!("lexpad-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), OLD_LEFT_OFF).unwrap();
        let file = SettingsFile::new(&dir);
        let (s, first) = file.load();
        assert!(!first);
        assert!(s.player_home_note);
        let text = std::fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(text.contains("\"openAtSet\": false"));
        assert!(text.contains("\"openAt\": \"player\""));
        // The note is cleared once shown; the next start keeps it cleared.
        let mut s = s;
        s.player_home_note = false;
        file.save(&s).unwrap();
        let (again, _) = file.load();
        assert!(!again.player_home_note);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_pages_words_for_the_choice() {
        assert_eq!(OpenAt::parse("today"), Some(OpenAt::Today));
        assert_eq!(OpenAt::parse("player"), Some(OpenAt::Player));
        // An older page's "Where I left off" is the Player now.
        assert_eq!(OpenAt::parse("leftOff"), Some(OpenAt::Player));
        assert_eq!(OpenAt::parse("nowhere"), None);
        assert_eq!(OpenAt::Today.as_str(), "today");
        assert_eq!(OpenAt::Player.as_str(), "player");
    }

    #[test]
    fn accessibility_lost_to_an_update_is_told_apart_from_a_refusal() {
        use crate::capture::Permission::*;
        assert!(stale_accessibility(Missing, Some("0.1.1"), "0.2.0"));
        assert!(!stale_accessibility(Missing, Some("0.2.0"), "0.2.0"));
        assert!(!stale_accessibility(Missing, None, "0.2.0"));
        assert!(!stale_accessibility(Granted, Some("0.1.1"), "0.2.0"));
        assert!(!stale_accessibility(NotNeeded, Some("0.1.1"), "0.2.0"));
    }
}
