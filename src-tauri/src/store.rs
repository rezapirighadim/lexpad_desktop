//! Where the session lives between runs: the operating system's credential
//! store (the macOS Keychain, the Windows Credential Manager), through the
//! keyring crate. Only the refresh token and who it belongs to are kept
//! there; the access token lives in memory and is refreshed at start.

use serde::{Deserialize, Serialize};

/// The service name the credential is filed under.
const SERVICE: &str = "app.lexpad.desktop";

/// What the credential store holds.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub refresh_token: String,
    pub user: User,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub email: String,
    pub display_name: String,
}

/// Reads and writes the saved session. A trait so tests can keep it in memory.
pub trait SessionStore: Send + Sync {
    fn load(&self) -> Option<Saved>;
    fn save(&self, saved: &Saved) -> Result<(), String>;
    fn clear(&self);
}

/// The real store: one generic credential per API, so a development build
/// against a local API never reads or overwrites the production session.
pub struct Keyring {
    account: String,
}

impl Keyring {
    pub fn new(api_origin: &str) -> Self {
        Self {
            account: format!("session {api_origin}"),
        }
    }

    fn entry(&self) -> Option<keyring::Entry> {
        keyring::Entry::new(SERVICE, &self.account)
            .map_err(|e| log::warn!("credential store unavailable: {e}"))
            .ok()
    }
}

impl SessionStore for Keyring {
    fn load(&self) -> Option<Saved> {
        let secret = self.entry()?.get_password().ok()?;
        serde_json::from_str(&secret).ok()
    }

    fn save(&self, saved: &Saved) -> Result<(), String> {
        let entry = self.entry().ok_or("credential store unavailable")?;
        let json = serde_json::to_string(saved).map_err(|e| e.to_string())?;
        entry.set_password(&json).map_err(|e| e.to_string())
    }

    fn clear(&self) {
        if let Some(entry) = self.entry() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => log::warn!("could not remove the saved session: {e}"),
            }
        }
    }
}

/// An in-memory store for tests.
#[cfg(test)]
#[derive(Default)]
pub struct Memory(pub std::sync::Mutex<Option<Saved>>);

#[cfg(test)]
impl SessionStore for Memory {
    fn load(&self) -> Option<Saved> {
        self.0.lock().unwrap().clone()
    }
    fn save(&self, saved: &Saved) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(saved.clone());
        Ok(())
    }
    fn clear(&self) {
        *self.0.lock().unwrap() = None;
    }
}
