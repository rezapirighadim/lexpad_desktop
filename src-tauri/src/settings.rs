//! The app's own settings, in `settings.json` in the app's config folder.
//! Nothing secret lives here: the session is in the credential store.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The shortcut on first run: ⌘⇧L on a Mac, Ctrl+Shift+L on Windows.
pub const DEFAULT_SHORTCUT: &str = "CommandOrControl+Shift+L";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Identifies this install to the API, so signing in again replaces this
    /// computer's old session instead of listing it twice.
    pub device_id: String,
    pub shortcut: String,
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
            start_on_login: true,
            notebook_id: None,
            recent: Vec::new(),
            open_on_launch: false,
            open_in_browser: false,
            main_window: None,
            notifications: true,
            inbox: crate::notify::InboxMemory::default(),
            accessibility_granted_in: None,
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
    pub fn load(&self) -> (Settings, bool) {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => (serde_json::from_str(&text).unwrap_or_default(), false),
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
        assert!(first.start_on_login);
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
