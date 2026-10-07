//! What the two windows may ask of the core. Each window gets only the
//! commands its capability lists (capabilities/*.json). None of them returns
//! a token, and none of them opens an arbitrary address.

use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::GlobalShortcutExt as _;
use tauri_plugin_opener::OpenerExt as _;

use crate::api::{Device, Failure};
use crate::auth::{self, Answer};
use crate::capture::{self, Capture, Permission};
use crate::config;
use crate::popup;
use crate::state::AppState;
use crate::store::User;

/// The most a private note may hold, as the API allows.
const MAX_MEMO: usize = 500;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDto {
    connected: bool,
    user: Option<User>,
    notebooks: Vec<Value>,
    notebook_id: Option<String>,
    capture: Option<Capture>,
    shortcut: String,
    permission: Permission,
    version: &'static str,
}

fn err(f: Failure) -> String {
    f.as_str().to_owned()
}

/// Whether `id` is an API id (a ULID). A notebook id goes into an address,
/// so anything else from a window is refused before it gets there.
fn is_id(id: &str) -> bool {
    id.len() == 26
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
}

/// Everything a window needs to draw itself. Reads the notebooks again each
/// time, falling back to the last list when offline.
#[tauri::command]
pub async fn state(app: AppHandle, state: State<'_, AppState>) -> Result<StateDto, String> {
    let user = state.api.user().await;
    let mut notebooks = Vec::new();
    let mut connected = user.is_some();
    if connected {
        match state.api.notebooks().await {
            Ok(list) => {
                *state.notebooks.lock().map_err(|_| "lock")? = list.clone();
                notebooks = list;
            }
            Err(Failure::SignedOut) => {
                connected = false;
                let _ = app.emit("session:changed", ());
            }
            Err(_) => notebooks = state.notebooks.lock().map_err(|_| "lock")?.clone(),
        }
    }
    let settings = state.settings.lock().map_err(|_| "lock")?.clone();
    // The learner's choice, else the account's default notebook, else the first.
    let notebook_id = settings
        .notebook_id
        .filter(|id| notebooks.iter().any(|n| n["id"] == *id.as_str()))
        .or_else(|| {
            notebooks
                .iter()
                .find(|n| n["isDefault"] == true)
                .or_else(|| notebooks.first())
                .and_then(|n| n["id"].as_str().map(str::to_owned))
        });
    let capture = state.capture.lock().map_err(|_| "lock")?.clone();
    Ok(StateDto {
        connected,
        user: if connected { user } else { None },
        notebooks,
        notebook_id,
        capture,
        shortcut: settings.shortcut,
        permission: capture::permission(),
        version: config::VERSION,
    })
}

#[tauri::command]
pub async fn lookup(
    state: State<'_, AppState>,
    notebook_id: String,
    headword: String,
    hint: Option<String>,
) -> Result<Value, String> {
    if !is_id(&notebook_id) || headword.trim().is_empty() || headword.chars().count() > 60 {
        return Err(err(Failure::Error));
    }
    let hint = hint.map(|h| capture::clip(&h, 200));
    state
        .api
        .lookup(&notebook_id, &headword, hint.as_deref())
        .await
        .map_err(err)
}

/// Adds one word. The card is the API's own shape (WordCreate); the popup
/// composed it from the meaning card, the sentence and the private note.
#[tauri::command]
pub async fn add_word(
    state: State<'_, AppState>,
    notebook_id: String,
    word: Value,
) -> Result<String, String> {
    let Some(obj) = word.as_object() else {
        return Err(err(Failure::Error));
    };
    let headword_ok = obj
        .get("headword")
        .and_then(Value::as_str)
        .is_some_and(|h| !h.trim().is_empty());
    let memo_ok = obj
        .get("memo")
        .is_none_or(|m| m.as_str().is_some_and(|m| m.chars().count() <= MAX_MEMO));
    if !is_id(&notebook_id) || !headword_ok || !memo_ok {
        return Err(err(Failure::Error));
    }
    state.api.add_word(&notebook_id, &word).await.map_err(err)
}

#[tauri::command]
pub fn set_notebook(
    app: AppHandle,
    state: State<'_, AppState>,
    notebook_id: String,
) -> Result<(), String> {
    if !is_id(&notebook_id) {
        return Err(err(Failure::Error));
    }
    state.update_settings(|s| s.notebook_id = Some(notebook_id))?;
    let _ = app.emit("settings:changed", ());
    Ok(())
}

/// Signs in through the browser (auth.rs): opens the connect page, waits for
/// the learner to allow it, trades the code. Ends with who is signed in.
#[tauri::command]
pub async fn connect(app: AppHandle, state: State<'_, AppState>) -> Result<User, String> {
    let pkce = auth::new_pkce();
    let st = auth::new_state();
    let (listener, port) = auth::listen().await.map_err(|_| err(Failure::Error))?;
    let redirect = auth::redirect_uri(port);
    let url = auth::connect_url(config::APP_ORIGIN, &redirect, &st, &pkce.challenge);

    // A new request replaces any earlier one still waiting.
    let cancel = Arc::new(tokio::sync::Notify::new());
    if let Some(old) = state
        .connect_cancel
        .lock()
        .map_err(|_| "lock")?
        .replace(cancel.clone())
    {
        old.notify_one();
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| err(Failure::Error))?;

    let answer = tokio::time::timeout(
        auth::CONNECT_TIMEOUT,
        auth::wait_for_answer(listener, &st, &cancel),
    )
    .await;
    *state.connect_cancel.lock().map_err(|_| "lock")? = None;
    let code = match answer {
        Ok(Some(Answer::Code(code))) => code,
        Ok(Some(Answer::Denied)) => return Err("cancelled".into()),
        Ok(None) => return Err("cancelled".into()),
        Err(_) => return Err("timeout".into()),
    };
    let device_id = state.settings.lock().map_err(|_| "lock")?.device_id.clone();
    let device = Device {
        platform: "desktop",
        device_id,
        device_name: config::device_name().into(),
        app_version: config::VERSION.into(),
    };
    let user = state
        .api
        .exchange_code(&code, &pkce.verifier, &redirect, &device)
        .await
        .map_err(err)?;
    let _ = app.emit("session:changed", ());
    Ok(user)
}

#[tauri::command]
pub fn cancel_connect(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(cancel) = state.connect_cancel.lock().map_err(|_| "lock")?.take() {
        cancel.notify_one();
    }
    Ok(())
}

/// Signs out: the session is revoked on the server and forgotten here.
#[tauri::command]
pub async fn disconnect(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.api.sign_out().await;
    state.update_settings(|s| s.notebook_id = None)?;
    state.notebooks.lock().map_err(|_| "lock")?.clear();
    let _ = app.emit("session:changed", ());
    Ok(())
}

#[tauri::command]
pub fn hide_popup(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    *state.capture.lock().map_err(|_| "lock")? = None;
    popup::hide(&app);
    Ok(())
}

/// Sizes the popup to its content.
#[tauri::command]
pub fn fit_popup(app: AppHandle, height: f64) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(popup::LABEL) {
        let h = height.clamp(popup::MIN_HEIGHT, popup::MAX_HEIGHT);
        win.set_size(LogicalSize::new(popup::WIDTH, h))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Whether the app may read selections; with `request`, also asks macOS to
/// list it under Accessibility (the system shows its own prompt).
#[tauri::command]
pub fn accessibility(request: bool) -> Permission {
    if request && capture::permission() == Permission::Missing {
        capture::request_permission();
    }
    capture::permission()
}

#[tauri::command]
pub fn open_accessibility_settings(app: AppHandle) -> Result<(), String> {
    // Make sure the app is in the list before opening it.
    capture::request_permission();
    if let Some(url) = capture::permission_settings_url() {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    shortcut: String,
    start_on_login: bool,
    development_build: bool,
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<SettingsDto, String> {
    let s = state.settings.lock().map_err(|_| "lock")?;
    Ok(SettingsDto {
        shortcut: s.shortcut.clone(),
        start_on_login: s.start_on_login,
        development_build: config::is_development_build(),
    })
}

/// Changes the shortcut. The new one is registered first; if the system
/// refuses it (another app holds it), the old one stays and the window says so.
#[tauri::command]
pub fn set_shortcut(
    app: AppHandle,
    state: State<'_, AppState>,
    shortcut: String,
) -> Result<String, String> {
    let old = state.settings.lock().map_err(|_| "lock")?.shortcut.clone();
    if shortcut == old {
        return Ok(old);
    }
    let gs = app.global_shortcut();
    gs.register(shortcut.as_str())
        .map_err(|_| "taken".to_owned())?;
    let _ = gs.unregister(old.as_str());
    state.update_settings(|s| s.shortcut = shortcut.clone())?;
    let _ = app.emit("settings:changed", ());
    Ok(shortcut)
}

#[tauri::command]
pub fn set_start_on_login(
    app: AppHandle,
    state: State<'_, AppState>,
    on: bool,
) -> Result<bool, String> {
    state.update_settings(|s| s.start_on_login = on)?;
    crate::apply_start_on_login(&app, on);
    Ok(on)
}

#[tauri::command]
pub fn open_settings(app: AppHandle) {
    popup::hide(&app);
    crate::show_settings(&app);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    api_origin: &'static str,
    app_origin: &'static str,
    autostart_enabled: bool,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    AppInfo {
        version: config::VERSION,
        api_origin: config::API_ORIGIN,
        app_origin: config::APP_ORIGIN,
        autostart_enabled: app.autolaunch().is_enabled().unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_api_ids_reach_an_address() {
        assert!(super::is_id("01M43H5KGS4XJQX824GWWRSRVS"));
        assert!(!super::is_id("../me/sessions"));
        assert!(!super::is_id("01M43H5KGS4XJQX824GWWRSRV/"));
        assert!(!super::is_id(""));
    }
}
