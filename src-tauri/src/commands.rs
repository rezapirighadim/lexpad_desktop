//! What the windows (popup, panel, Settings) may ask of the core. Each window gets only the
//! commands its capability lists (capabilities/*.json). None of them returns
//! a token, and none of them opens an arbitrary address.

use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::GlobalShortcutExt as _;
use tauri_plugin_opener::OpenerExt as _;

use crate::api::{Device, Failure};
use crate::auth::{self, Answer};
use crate::capture::{self, Capture, Permission, Via};
use crate::config;
use crate::main_window;
use crate::notify;
use crate::panel;
use crate::popup;
use crate::proxy;
use crate::settings::{remember, RecentWord};
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
    /// Accessibility was allowed in an earlier version and macOS dropped it
    /// with the update (see `settings::stale_accessibility`).
    permission_stale: bool,
    version: &'static str,
}

/// Reads the Accessibility permission, remembers the version that last had
/// it, and says whether a missing one was lost to an update.
fn accessibility_now(state: &AppState) -> (Permission, bool) {
    let now = capture::permission();
    let granted_in = state
        .settings
        .lock()
        .ok()
        .and_then(|s| s.accessibility_granted_in.clone());
    if now == Permission::Granted && granted_in.as_deref() != Some(config::VERSION) {
        let _ =
            state.update_settings(|s| s.accessibility_granted_in = Some(config::VERSION.into()));
    }
    (
        now,
        crate::settings::stale_accessibility(now, granted_in.as_deref(), config::VERSION),
    )
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
    let (permission, permission_stale) = accessibility_now(&state);
    Ok(StateDto {
        connected,
        user: if connected { user } else { None },
        notebooks,
        notebook_id,
        capture,
        shortcut: settings.shortcut,
        permission,
        permission_stale,
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
    app: AppHandle,
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
    let id = state.api.add_word(&notebook_id, &word).await.map_err(err)?;
    // Remembered for the panel's "Added from this computer" list.
    if let Some(user) = state.api.user().await {
        let added = RecentWord {
            id: id.clone(),
            headword: obj
                .get("headword")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_owned(),
            notebook_id,
            user_id: user.id,
            added_at: now_ms(),
        };
        if state
            .update_settings(|s| s.recent = remember(&s.recent, added))
            .is_ok()
        {
            let _ = app.emit("recent:changed", ());
        }
    }
    Ok(id)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
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
    let user = state.api.user().await;
    state.api.sign_out().await;
    state.update_settings(|s| {
        s.notebook_id = None;
        // Signing out forgets what this account added from here.
        if let Some(user) = &user {
            s.recent.retain(|w| w.user_id != user.id);
        }
    })?;
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

/// Sizes the popup to its content, inside the work area.
#[tauri::command]
pub fn fit_popup(app: AppHandle, height: f64) -> Result<(), String> {
    if !height.is_finite() {
        return Err(err(Failure::Error));
    }
    popup::fit(&app, height).map_err(|e| e.to_string())
}

/// Whether the app may read selections; with `request`, also asks macOS to
/// list it under Accessibility (the system shows its own prompt).
#[tauri::command]
pub fn accessibility(state: State<'_, AppState>, request: bool) -> Permission {
    if request && capture::permission() == Permission::Missing {
        capture::request_permission();
    }
    accessibility_now(&state).0
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
    open_on_launch: bool,
    open_in_browser: bool,
    development_build: bool,
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<SettingsDto, String> {
    let s = state.settings.lock().map_err(|_| "lock")?;
    Ok(SettingsDto {
        shortcut: s.shortcut.clone(),
        start_on_login: s.start_on_login,
        open_on_launch: s.open_on_launch,
        open_in_browser: s.open_in_browser,
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
    panel::hide(&app, false);
    crate::open_desktop_settings(&app);
}

/* ------------------------------------------------------------- panel */

/// The words added from this computer to the signed-in account, newest first.
#[tauri::command]
pub async fn recent(state: State<'_, AppState>) -> Result<Vec<RecentWord>, String> {
    let Some(user) = state.api.user().await else {
        return Ok(Vec::new());
    };
    let settings = state.settings.lock().map_err(|_| "lock")?;
    Ok(settings
        .recent
        .iter()
        .filter(|w| w.user_id == user.id)
        .cloned()
        .collect())
}

/// The panel's "Add a word" box: the card opens for the typed text, from
/// where the panel was, exactly as for a selection (a phrase is looked up,
/// a whole sentence offers its words to pick from).
#[tauri::command]
pub fn panel_add(app: AppHandle, text: String) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(err(Failure::Error));
    }
    let mut typed = Capture::empty(capture::permission(), None);
    typed.text = Some(capture::clip(text, capture::MAX_TEXT));
    typed.via = Via::Typed;
    typed.anchor = panel::last_icon();
    panel::hide(&app, false);
    popup::open(&app, typed);
    Ok(())
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) {
    panel::hide(&app, true);
}

/// Sizes the panel to its content.
#[tauri::command]
pub fn fit_panel(app: AppHandle, height: f64) -> Result<(), String> {
    if !height.is_finite() {
        return Err(err(Failure::Error));
    }
    panel::fit(&app, height).map_err(|e| e.to_string())
}

/// Opens the web app in the browser: its home, or one word's page. The
/// address is always built here from APP_ORIGIN; a window passes at most an id.
#[tauri::command]
pub fn open_web(app: AppHandle, word_id: Option<String>) -> Result<(), String> {
    if word_id.as_deref().is_some_and(|id| !is_id(id)) {
        return Err(err(Failure::Error));
    }
    panel::hide(&app, false);
    crate::open_web(&app, word_id.as_deref());
    Ok(())
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

/* ------------------------------------------------------- main window */

/// The answer to a request from Lexpad's window, before its body.
#[derive(Serialize)]
pub struct Head {
    status: u16,
    headers: Vec<(String, String)>,
}

/// One piece of a response body for Lexpad's window, its end, or a failure.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BodyEvent {
    Chunk { data: String },
    End,
    Error,
}

/// Response headers the page may read: what the client looks at. Nothing
/// about cookies or the session.
const RESPONSE_HEADERS: &[&str] = &["content-type", "etag", "retry-after", "content-disposition"];

/// Sends one request from Lexpad's window to the API with the session only
/// the core holds (see `proxy.rs` for what the window may send), and
/// streams the body back through `events`. A failure to send is "offline";
/// a refused request is "error": either way the page sees a failed fetch.
#[tauri::command]
pub async fn api_fetch(
    app: AppHandle,
    state: State<'_, AppState>,
    request: proxy::Request,
    events: Channel<BodyEvent>,
) -> Result<Head, String> {
    let checked = proxy::check(config::API_ORIGIN, &request).map_err(|refused| {
        log::warn!("a request from the window was refused: {refused:?}");
        err(Failure::Error)
    })?;
    let had_session = state.api.user().await.is_some();
    let mut res = state.api.forward(&checked).await.map_err(err)?;
    let status = res.status().as_u16();
    // A 401 the core could not refresh away has ended the session: the
    // other windows (the panel, Settings) hear it too.
    if status == 401 && had_session && state.api.user().await.is_none() {
        let _ = app.emit("session:changed", ());
    }
    let headers = res
        .headers()
        .iter()
        .filter(|(name, _)| RESPONSE_HEADERS.contains(&name.as_str()))
        .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_owned())))
        .collect();
    tauri::async_runtime::spawn(async move {
        use base64::Engine as _;
        loop {
            match res.chunk().await {
                Ok(Some(bytes)) => {
                    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    if events.send(BodyEvent::Chunk { data }).is_err() {
                        return;
                    }
                }
                Ok(None) => {
                    let _ = events.send(BodyEvent::End);
                    return;
                }
                Err(_) => {
                    let _ = events.send(BodyEvent::Error);
                    return;
                }
            }
        }
    });
    Ok(Head { status, headers })
}

/// Whether the core holds a session, for Lexpad's window.
#[tauri::command]
pub async fn main_signed_in(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.api.user().await.is_some())
}

/// A page Lexpad's window was asked to open before it was listening.
#[tauri::command]
pub fn main_take_pending() -> Option<String> {
    main_window::take_pending()
}

/// Whether `path` is a plain path of the web app: one leading slash, no
/// second one after it, no backslash, no whitespace or control characters,
/// and still the web app's origin once joined to it. The same rule as the
/// web app's `returnPath`.
pub fn app_path(path: &str) -> Option<url::Url> {
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('\\')
        || path.chars().any(|c| c.is_whitespace() || c.is_control())
        || path.len() > 512
    {
        return None;
    }
    let origin = url::Url::parse(config::APP_ORIGIN).ok()?;
    let url = origin.join(path).ok()?;
    (url.origin() == origin.origin() && url.username().is_empty()).then_some(url)
}

/// Opens a page of the web app in the system browser, for what the window's
/// delegated session may not do there (a password). Only a path of the web
/// app is accepted; the address is built here.
#[tauri::command]
pub fn open_in_browser(app: AppHandle, path: String) -> Result<(), String> {
    let url = app_path(&path).ok_or_else(|| err(Failure::Error))?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|_| err(Failure::Error))
}

#[tauri::command]
pub fn set_open_on_launch(state: State<'_, AppState>, on: bool) -> Result<bool, String> {
    state.update_settings(|s| s.open_on_launch = on)?;
    Ok(on)
}

#[tauri::command]
pub fn set_open_in_browser(
    app: AppHandle,
    state: State<'_, AppState>,
    on: bool,
) -> Result<bool, String> {
    state.update_settings(|s| s.open_in_browser = on)?;
    let _ = app.emit("settings:changed", ());
    Ok(on)
}

/* ------------------------------------------- desktop section, window */

/// Everything the desktop section of Settings in Lexpad's window shows.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopDto {
    os: &'static str,
    version: &'static str,
    development_build: bool,
    shortcut: String,
    start_on_login: bool,
    open_on_launch: bool,
    open_in_browser: bool,
    /// The notebook the shortcut adds to (None: the account's default).
    notebook_id: Option<String>,
    accessibility: Permission,
    accessibility_stale: bool,
    notifications: bool,
    notification_access: notify::Access,
}

#[tauri::command]
pub async fn desktop_settings(state: State<'_, AppState>) -> Result<DesktopDto, String> {
    let (accessibility, accessibility_stale) = accessibility_now(&state);
    let notification_access = tauri::async_runtime::spawn_blocking(notify::access)
        .await
        .unwrap_or(notify::Access::Unsupported);
    let s = state.settings.lock().map_err(|_| "lock")?.clone();
    Ok(DesktopDto {
        os: if cfg!(target_os = "macos") {
            "macos"
        } else {
            "windows"
        },
        version: config::VERSION,
        development_build: config::is_development_build(),
        shortcut: s.shortcut,
        start_on_login: s.start_on_login,
        open_on_launch: s.open_on_launch,
        open_in_browser: s.open_in_browser,
        notebook_id: s.notebook_id,
        accessibility,
        accessibility_stale,
        notifications: s.notifications,
        notification_access,
    })
}

/// Notifications on this computer on or off. Off takes back the reminders
/// the system holds; on asks for permission when it was never asked (macOS
/// shows its own prompt) and reads the inbox's state.
#[tauri::command]
pub async fn set_notifications(
    app: AppHandle,
    state: State<'_, AppState>,
    on: bool,
) -> Result<notify::Access, String> {
    state.update_settings(|s| s.notifications = on)?;
    if !on {
        tauri::async_runtime::spawn_blocking(notify::silence)
            .await
            .map_err(|_| err(Failure::Error))?;
        return Ok(notify::Access::Denied);
    }
    let access = tauri::async_runtime::spawn_blocking(notify::request)
        .await
        .unwrap_or(notify::Access::Unsupported);
    let _ = app.emit("settings:changed", ());
    Ok(access)
}

/// The reminder plan Lexpad's window worked out (`sync/reminder.ts`), for
/// the system to deliver; null cancels it. Answers what the web app's
/// `ReminderStatus` means: scheduled, off, denied or unsupported.
#[tauri::command]
pub async fn schedule_reminders(
    state: State<'_, AppState>,
    plan: Option<Vec<notify::Notice>>,
) -> Result<&'static str, String> {
    let on = state.settings.lock().map_err(|_| "lock")?.notifications;
    let Some(plan) = plan else {
        tauri::async_runtime::spawn_blocking(notify::silence)
            .await
            .map_err(|_| err(Failure::Error))?;
        return Ok("off");
    };
    if !on {
        // Silenced on this computer: nothing is held here.
        tauri::async_runtime::spawn_blocking(notify::silence)
            .await
            .map_err(|_| err(Failure::Error))?;
        return Ok("unsupported");
    }
    let planned = notify::plan(&plan, chrono::Utc::now());
    tauri::async_runtime::spawn_blocking(move || match notify::request() {
        notify::Access::Granted => {
            notify::replace_reminders(&planned);
            "scheduled"
        }
        notify::Access::Denied => "denied",
        _ => "unsupported",
    })
    .await
    .map_err(|_| err(Failure::Error))
}

/// The system's notification settings for this app.
#[tauri::command]
pub fn open_notification_settings(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(notify::settings_url(), None::<&str>)
        .map_err(|_| err(Failure::Error))
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
    fn only_plain_paths_of_the_web_app_open_in_the_browser() {
        let ok = super::app_path("/settings/account").unwrap();
        assert_eq!(
            ok.as_str(),
            format!("{}/settings/account", crate::config::APP_ORIGIN)
        );
        assert!(super::app_path("/words/01M43H5KGS4XJQX824GWWRSRVS?x=1").is_some());
        for bad in [
            "settings",
            "//evil.example/x",
            "/\\evil.example",
            "https://evil.example/",
            "/ space",
            "/tab\t",
            "/new\nline",
            "",
        ] {
            assert!(super::app_path(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn only_api_ids_reach_an_address() {
        assert!(super::is_id("01M43H5KGS4XJQX824GWWRSRVS"));
        assert!(!super::is_id("../me/sessions"));
        assert!(!super::is_id("01M43H5KGS4XJQX824GWWRSRV/"));
        assert!(!super::is_id(""));
    }
}
