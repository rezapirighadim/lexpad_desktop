//! What the app holds while it runs.

use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::api::Api;
use crate::capture::Capture;
use crate::settings::{Settings, SettingsFile};

pub struct AppState {
    pub api: Api,
    pub settings: Mutex<Settings>,
    pub settings_file: SettingsFile,
    /// What the last shortcut (or Services, or the tray) captured; the popup
    /// asks for it when it opens.
    pub capture: Mutex<Option<Capture>>,
    /// Notebooks from the last successful read, so a popup opened offline
    /// still has something to show.
    pub notebooks: Mutex<Vec<Value>>,
    /// Ends a connect that is waiting for the browser.
    pub connect_cancel: Mutex<Option<Arc<tokio::sync::Notify>>>,
    /// The Mac App Store copy's sign-in waiting for `lexpad://auth/callback`
    /// (auth.rs); the deep-link handler in lib.rs delivers to it.
    pub scheme_wait: Arc<crate::auth::SchemeWait>,
}

impl AppState {
    pub fn update_settings(&self, change: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
        let mut settings = self.settings.lock().map_err(|_| "settings lock")?;
        change(&mut settings);
        self.settings_file.save(&settings)?;
        Ok(settings.clone())
    }
}
