//! The player's own preferences on this computer (`media-settings.json` in
//! the app's data folder, beside the resume file): whether recent files are
//! remembered for "Continue watching", and which app opened each type
//! before the learner chose Lexpad for it (to give it back).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Prefs {
    pub remember_recent: bool,
    /// Extension → the bundle id (macOS) that opened it before Lexpad.
    pub previous_handlers: HashMap<String, String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            remember_recent: true,
            previous_handlers: HashMap::new(),
        }
    }
}

pub struct PrefsFile {
    file: Option<PathBuf>,
    prefs: Mutex<Prefs>,
}

impl PrefsFile {
    pub fn load(file: Option<PathBuf>) -> PrefsFile {
        let prefs = file
            .as_ref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        PrefsFile {
            file,
            prefs: Mutex::new(prefs),
        }
    }

    pub fn get(&self) -> Prefs {
        self.prefs.lock().map(|p| p.clone()).unwrap_or_default()
    }

    pub fn update(&self, change: impl FnOnce(&mut Prefs)) -> Prefs {
        let Ok(mut p) = self.prefs.lock() else {
            return Prefs::default();
        };
        change(&mut p);
        if let (Some(file), Ok(bytes)) = (&self.file, serde_json::to_vec(&*p)) {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let tmp = file.with_extension("json.tmp");
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, file);
            }
        }
        p.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_on_disk() {
        let file =
            std::env::temp_dir().join(format!("lexpad-media-prefs-{}.json", uuid::Uuid::new_v4()));
        let p = PrefsFile::load(Some(file.clone()));
        assert!(p.get().remember_recent);
        p.update(|p| {
            p.remember_recent = false;
            p.previous_handlers
                .insert("mkv".into(), "org.videolan.vlc".into());
        });
        let again = PrefsFile::load(Some(file.clone()));
        assert!(!again.get().remember_recent);
        assert_eq!(again.get().previous_handlers["mkv"], "org.videolan.vlc");
        let _ = std::fs::remove_file(file);
    }
}
