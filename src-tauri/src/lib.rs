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

mod api;
mod auth;
mod capture;
mod commands;
mod config;
#[cfg(test)]
mod e2e;
mod panel;
mod popup;
#[cfg(target_os = "macos")]
mod services_macos;
mod settings;
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
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(win) = app.get_webview_window(SETTINGS) {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
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

/// Opens the web app in the browser: its home, or a word's page when `word_id`
/// is an API id (the command checks it). The address is built here, from
/// APP_ORIGIN; nothing else is ever opened from the tray or the panel.
pub fn open_web(app: &AppHandle, word_id: Option<&str>) {
    let url = match word_id {
        Some(id) => format!("{}/words/{id}", config::APP_ORIGIN),
        None => format!("{}/", config::APP_ORIGIN),
    };
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        log::warn!("could not open the web app: {e}");
    }
}

pub fn run() {
    tauri::Builder::default()
        // First, so a second launch only brings this one forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_settings(app)
        }))
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
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let dir = app.path().app_config_dir()?;
            let file = SettingsFile::new(&dir);
            let (settings, first_run) = file.load();
            if first_run {
                file.save(&settings)
                    .map_err(Box::<dyn std::error::Error>::from)?;
            }
            let store = Arc::new(store::Keyring::new(config::API_ORIGIN));
            let shortcut = settings.shortcut.clone();
            let start_on_login = settings.start_on_login;
            app.manage(AppState {
                api: api::Api::new(config::API_ORIGIN, store),
                settings: Mutex::new(settings),
                settings_file: file,
                capture: Mutex::new(None),
                notebooks: Mutex::new(Vec::new()),
                connect_cancel: Mutex::new(None),
            });

            let handle = app.handle().clone();
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

            // First run, or not signed in yet: open Settings to say hello.
            let api_user = tauri::async_runtime::block_on(handle.state::<AppState>().api.user());
            if first_run || api_user.is_none() {
                show_settings(&handle);
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
                #[cfg(target_os = "macos")]
                let _ = window
                    .app_handle()
                    .set_activation_policy(tauri::ActivationPolicy::Accessory);
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
        ])
        .build(tauri::generate_context!())
        .expect("error while building Lexpad")
        .run(|_app, event| {
            // Closing every window keeps the app in the menu bar / tray.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
