//! "Lexpad on your phone…" / "Lexpad everywhere…": one row in the tray panel
//! and one item in the tray's right-click menu (uiux
//! `2026-10-31-ecosystem-cards/spec.md` §6). Both open Settings → Lexpad
//! everywhere in Lexpad's window, where the phone has its QR code.
//!
//! Which words it says comes from the API's `GET /me/surfaces`, the rows the
//! web app's Lexpad everywhere page draws: a phone app counts as used when the
//! `android` or `ios` row has `everUsed`. The server never takes `everUsed`
//! back, so once it is true for an account it is kept in `settings.json`
//! (`phone_used_by`) and never asked again. Until then the panel asks each
//! time it opens, alongside the account's state, so the label is never older
//! than the last opening. Signed out there is no row and no menu item.

use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt as _;

use crate::api::Failure;
use crate::state::AppState;

/// The web app's Settings → Lexpad everywhere.
pub const PATH: &str = "/settings/everywhere";

/// The words while no phone app was used, and after.
pub const OFFER: &str = "Lexpad on your phone…";
pub const USED: &str = "Lexpad everywhere…";

pub fn label(phone_used: bool) -> &'static str {
    if phone_used {
        USED
    } else {
        OFFER
    }
}

/// What the tray shows for the account signed in now: nothing when signed
/// out (`None`), else whether a phone app was ever used.
pub async fn phone_used(state: &AppState) -> Option<bool> {
    let user = state.api.user().await?;
    let known = state
        .settings
        .lock()
        .ok()
        .and_then(|s| s.phone_used_by.clone());
    if let Some(answer) = remembered(known.as_deref(), &user.id) {
        return Some(answer);
    }
    match state.api.phone_used().await {
        Ok(true) => {
            let id = user.id.clone();
            if let Err(e) = state.update_settings(|s| s.phone_used_by = Some(id)) {
                log::warn!("could not remember the phone app: {e}");
            }
            Some(true)
        }
        Ok(false) => Some(false),
        Err(Failure::SignedOut) => None,
        // Offline or a refused read: the offer stands, as before any phone
        // was used. Both open the same page.
        Err(_) => Some(false),
    }
}

/// The remembered answer for `user`: only "used", and only for that account.
fn remembered(phone_used_by: Option<&str>, user: &str) -> Option<bool> {
    (phone_used_by == Some(user)).then_some(true)
}

/// Opens Settings → Lexpad everywhere: in Lexpad's window, or in the browser
/// when the learner prefers that (Settings), as `open_web` decides.
pub fn open(app: &AppHandle) {
    let in_browser = app
        .try_state::<AppState>()
        .and_then(|s| s.settings.lock().ok().map(|s| s.open_in_browser))
        .unwrap_or(false);
    if !in_browser {
        crate::main_window::open(app, Some(PATH.into()));
        return;
    }
    let url = format!("{}{PATH}", crate::config::APP_ORIGIN);
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        log::warn!("could not open Lexpad everywhere: {e}");
    }
}

/// Reads the answer again and puts the tray's menu item in step with it
/// (at start, after a sign-in or sign-out).
pub async fn sync_tray(app: AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let used = phone_used(&state).await;
    crate::tray::set_everywhere(&app, used);
}

#[cfg(test)]
mod tests {
    use super::{label, remembered, OFFER, PATH, USED};

    #[test]
    fn says_the_phone_until_a_phone_app_was_used() {
        assert_eq!(label(false), "Lexpad on your phone…");
        assert_eq!(label(true), "Lexpad everywhere…");
        assert_eq!(OFFER, label(false));
        assert_eq!(USED, label(true));
    }

    #[test]
    fn remembers_the_phone_only_for_the_account_that_used_it() {
        assert_eq!(remembered(Some("u1"), "u1"), Some(true));
        assert_eq!(remembered(Some("u1"), "u2"), None);
        assert_eq!(remembered(None, "u1"), None);
    }

    #[test]
    fn opens_a_plain_path_of_the_web_app() {
        let url = crate::commands::app_path(PATH).expect("a web app path");
        assert_eq!(url.path(), "/settings/everywhere");
        assert_eq!(url.query(), None);
    }
}
