//! The app's own settings, in `settings.json` in the app's config folder.
//! Nothing secret lives here: the session is in the credential store.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The shortcut on first run: ⌘⇧L on a Mac, Ctrl+Shift+L on Windows.
pub const DEFAULT_SHORTCUT: &str = "CommandOrControl+Shift+L";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Identifies this install to the API, so signing in again replaces this
    /// computer's old session instead of listing it twice.
    pub device_id: String,
    pub shortcut: String,
    pub start_on_login: bool,
    /// The notebook words go to, unless the word cannot be in its language.
    pub notebook_id: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_id: uuid::Uuid::new_v4().to_string(),
            shortcut: DEFAULT_SHORTCUT.into(),
            start_on_login: true,
            notebook_id: None,
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

    #[test]
    fn a_file_from_an_older_version_keeps_what_it_has() {
        let s: Settings = serde_json::from_str(r#"{"deviceId":"d1","shortcut":"Alt+L"}"#).unwrap();
        assert_eq!(s.device_id, "d1");
        assert_eq!(s.shortcut, "Alt+L");
        assert!(s.start_on_login);
    }
}
