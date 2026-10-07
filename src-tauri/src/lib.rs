//! Lexpad for macOS and Windows: a menu-bar / tray app that adds the word you
//! selected in any app to your Lexpad notebook, with its meaning.
//!
//! - `capture`: reads the selection in the app in front (accessibility API,
//!   then the clipboard, put back exactly).
//! - `popup`: the card near the selection; `panel`: the menu-bar / tray panel;
//!   `tray`: the icon and its fallback menu; `commands`: what the windows may ask.
//! - `api`: the only holder of tokens; `auth`: RFC 8252 sign-in in the browser;
//!   `store`: the session in the system's credential store.
//! - `services_macos`: "Add to Lexpad" in the macOS Services menu.
//! - `main_window`: Lexpad's own window, the whole web app, whose calls go
//!   through the core (`proxy`) so the page never holds a session.

mod api;
mod auth;
mod capture;
mod commands;
mod config;
#[cfg(test)]
mod e2e;
#[cfg(test)]
mod e2e_main;
mod main_window;
mod notify;
mod panel;
mod placement;
mod popup;
mod proxy;
#[cfg(target_os = "macos")]
mod services_macos;
mod settings;
#[cfg(feature = "smoke-test")]
mod smoke;
mod state;
mod store;
mod tray;

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_global_shortcut::{GlobalShortcutExt as _, ShortcutState};
use tauri_plugin_opener::OpenerExt as _;

use crate::settings::SettingsFile;
use crate::state::AppState;

pub const SETTINGS: &str = "settings";

/// Opens Settings, with a Dock icon on macOS while it is open.
pub fn show_settings(app: &AppHandle) {
    refresh_dock(app, true);
    if let Some(win) = app.get_webview_window(SETTINGS) {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// Settings, from the tray, the panel or the card: the desktop section of
/// Settings in Lexpad's window when the learner is signed in and uses the
/// window, else the small Settings window (which also says hello on the
/// first run and connects a signed-out app).
pub fn open_desktop_settings(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let signed_in = tauri::async_runtime::block_on(state.api.user()).is_some();
    let in_browser = state
        .settings
        .lock()
        .map(|s| s.open_in_browser)
        .unwrap_or(false);
    if signed_in && !in_browser {
        main_window::open(app, Some(main_window::DESKTOP_SETTINGS.into()));
    } else {
        show_settings(app);
    }
}

/// Shows the Dock icon (macOS) while Lexpad's window or Settings is open,
/// or about to open (`opening`), and hides it when neither is: idle, the app
/// lives in the menu bar only.
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
pub fn refresh_dock(app: &AppHandle, opening: bool) {
    #[cfg(target_os = "macos")]
    {
        let visible = |label: &str| {
            app.get_webview_window(label)
                .and_then(|w| w.is_visible().ok())
                .unwrap_or(false)
        };
        let policy = if opening || visible(SETTINGS) || visible(main_window::LABEL) {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        let _ = app.set_activation_policy(policy);
    }
}

/// Registers or removes the app from the login items. A development build
/// (debug, or pointed at a local API) never registers itself.
pub fn apply_start_on_login(app: &AppHandle, on: bool) {
    if config::is_development_build() {
        return;
    }
    let launcher = app.autolaunch();
    let result = if on {
        launcher.enable()
    } else {
        launcher.disable()
    };
    if let Err(e) = result {
        log::warn!("could not change start on login: {e}");
    }
}

/// Opens Lexpad: its own window, or the web app in the browser when the
/// learner prefers that (Settings). At its home, or at a word's page when
/// `word_id` is an API id (the command checks it). Every address is built
/// here, from APP_ORIGIN; nothing else is ever opened from the tray or the panel.
pub fn open_web(app: &AppHandle, word_id: Option<&str>) {
    let in_browser = app
        .try_state::<AppState>()
        .and_then(|s| s.settings.lock().ok().map(|s| s.open_in_browser))
        .unwrap_or(false);
    if !in_browser {
        main_window::open(app, word_id.map(|id| format!("/words/{id}")));
        return;
    }
    open_in_browser(app, word_id);
}

/// The web app in the system browser: its home, or a word's page.
pub fn open_in_browser(app: &AppHandle, word_id: Option<&str>) {
    let url = match word_id {
        Some(id) => format!("{}/words/{id}", config::APP_ORIGIN),
        None => format!("{}/", config::APP_ORIGIN),
    };
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        log::warn!("could not open the web app: {e}");
    }
}

/// Where the session is kept: the system's credential store, or nothing at
/// all for the CI smoke test.
fn session_store(smoke: bool) -> Arc<dyn store::SessionStore> {
    #[cfg(feature = "smoke-test")]
    if smoke {
        return Arc::new(smoke::Nothing);
    }
    let _ = smoke;
    Arc::new(store::Keyring::new(config::API_ORIGIN))
}

pub fn run() {
    #[cfg(feature = "smoke-test")]
    let smoke = smoke::requested();
    #[cfg(not(feature = "smoke-test"))]
    let smoke = false;

    let mut builder = tauri::Builder::default();
    // First, so a second launch only brings this one forward. The CI smoke
    // test runs beside an installed copy, so it is not "a second launch".
    if !smoke {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            open_web(app, None)
        }));
    }
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        popup::trigger(app);
                    }
                })
                .build(),
        )
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // A smoke-test build run with --smoke-test (CI only) keeps its
            // settings in a temporary folder and never touches the
            // credential store, the shortcut or the login items.
            let dir = if smoke {
                std::env::temp_dir().join("lexpad-desktop-smoke-test")
            } else {
                app.path().app_config_dir()?
            };
            let file = SettingsFile::new(&dir);
            let (settings, first_run) = file.load();
            if first_run {
                file.save(&settings)
                    .map_err(Box::<dyn std::error::Error>::from)?;
            }
            let store = session_store(smoke);
            let shortcut = settings.shortcut.clone();
            let start_on_login = settings.start_on_login;
            let open_on_launch = settings.open_on_launch;
            app.manage(AppState {
                api: api::Api::new(config::API_ORIGIN, store),
                settings: Mutex::new(settings),
                settings_file: file,
                capture: Mutex::new(None),
                notebooks: Mutex::new(Vec::new()),
                connect_cancel: Mutex::new(None),
            });

            let handle = app.handle().clone();
            if smoke {
                panel::create(&handle)?;
                tray::build(&handle)?;
                #[cfg(feature = "smoke-test")]
                smoke::start(handle);
                return Ok(());
            }
            if let Err(e) = handle.global_shortcut().register(shortcut.as_str()) {
                log::warn!("shortcut {shortcut} not available: {e}");
            }
            panel::create(&handle)?;
            tray::build(&handle)?;
            if first_run || start_on_login {
                apply_start_on_login(&handle, start_on_login);
            }
            #[cfg(target_os = "macos")]
            services_macos::register(&handle);
            notify::init(&handle);
            notify::start(&handle);

            // First run, or not signed in yet: open Settings to say hello.
            let api_user = tauri::async_runtime::block_on(handle.state::<AppState>().api.user());
            if first_run || api_user.is_none() {
                show_settings(&handle);
            }
            if open_on_launch {
                open_web(&handle, None);
            }
            Ok(())
        })
        .on_window_event(|window, event| match (window.label(), event) {
            // A click anywhere else puts the popup away, like a menu.
            (popup::LABEL, WindowEvent::Focused(false)) => {
                let _ = window.hide();
            }
            // The panel too; the app clicked into already has the keyboard.
            (panel::LABEL, WindowEvent::Focused(false)) => {
                panel::hide(window.app_handle(), false);
            }
            (SETTINGS, WindowEvent::CloseRequested { api, .. }) => {
                api.prevent_close();
                let _ = window.hide();
                refresh_dock(window.app_handle(), false);
            }
            // Lexpad's window is let go when closed; where it was is kept.
            (main_window::LABEL, WindowEvent::CloseRequested { .. }) => {
                main_window::remember(window.app_handle());
            }
            (main_window::LABEL, WindowEvent::Destroyed) => {
                refresh_dock(window.app_handle(), false);
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::state,
            commands::lookup,
            commands::add_word,
            commands::set_notebook,
            commands::connect,
            commands::cancel_connect,
            commands::disconnect,
            commands::hide_popup,
            commands::fit_popup,
            commands::accessibility,
            commands::open_accessibility_settings,
            commands::get_settings,
            commands::set_shortcut,
            commands::set_start_on_login,
            commands::open_settings,
            commands::app_info,
            commands::recent,
            commands::panel_add,
            commands::hide_panel,
            commands::fit_panel,
            commands::open_web,
            commands::quit,
            commands::api_fetch,
            commands::main_signed_in,
            commands::main_take_pending,
            commands::open_in_browser,
            commands::set_open_on_launch,
            commands::set_open_in_browser,
            commands::desktop_settings,
            commands::set_notifications,
            commands::schedule_reminders,
            commands::open_notification_settings,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Lexpad")
        .run(|app, event| match event {
            // Closing every window keeps the app in the menu bar / tray.
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                if code.is_none() {
                    api.prevent_exit();
                } else {
                    main_window::remember(app);
                }
            }
            // A click on the Dock icon (macOS) opens Lexpad's window.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => open_web(app, None),
            _ => {}
        });
}
