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
//! - `msix` (Windows): what differs in the Microsoft Store's package.
//! - `media`: the video player's engine (bundled FFmpeg, the `lexmedia`
//!   scheme, subtitles, episodes); `player_dev`: its development page.

mod api;
mod app_menu;
mod auth;
mod capture;
mod commands;
mod config;
#[cfg(test)]
mod e2e;
#[cfg(test)]
mod e2e_main;
mod main_window;
mod media;
#[cfg(windows)]
mod msix;
mod notify;
mod panel;
mod placement;
#[cfg(feature = "player-dev")]
mod player_dev;
mod player_window;
mod popup;
mod proxy;
mod release;
#[cfg(target_os = "macos")]
mod services_macos;
mod settings;
#[cfg(feature = "smoke-test")]
mod smoke;
mod state;
mod store;
mod tray;
mod welcome;
mod word_queue;

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, DragDropEvent, Manager, WindowEvent};
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
        let policy = if opening
            || visible(SETTINGS)
            || visible(main_window::LABEL)
            || visible(player_window::LABEL)
            || visible(welcome::LABEL)
        {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        let _ = app.set_activation_policy(policy);
    }
}

/// Registers or removes the app from the login items. A development build
/// (debug, or pointed at a local API) and the player test build never
/// register themselves. The Store's package uses its StartupTask instead (`msix`).
pub fn apply_start_on_login(app: &AppHandle, on: bool) {
    if config::is_development_build() || config::PLAYER_TEST {
        return;
    }
    #[cfg(windows)]
    if msix::is_packaged() {
        match msix::set_startup(on) {
            Ok(now) if now != on => log::warn!(
                "Windows kept start on login {}",
                if now { "on" } else { "off" }
            ),
            Ok(_) => {}
            Err(e) => log::warn!("could not change start on login: {e}"),
        }
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

/// Whether the app starts when the learner logs in, as the system has it.
pub fn start_on_login_enabled(app: &AppHandle) -> bool {
    #[cfg(windows)]
    if msix::is_packaged() {
        return msix::startup_enabled().unwrap_or(false);
    }
    app.autolaunch().is_enabled().unwrap_or(false)
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

/// Opens the web app's report page (`/report-ai`) with what the popup card
/// showed in the fragment, where the learner reports AI-written content (app
/// store rules for generative AI). Lexpad's own window or the browser, as
/// `open_web` decides. The fragment is checked by the command; the address
/// is built here.
pub fn open_report(app: &AppHandle, fragment: &str) {
    let path = format!("/report-ai#r={fragment}");
    let in_browser = app
        .try_state::<AppState>()
        .and_then(|s| s.settings.lock().ok().map(|s| s.open_in_browser))
        .unwrap_or(false);
    if !in_browser {
        main_window::open(app, Some(path));
        return;
    }
    if let Err(e) = app
        .opener()
        .open_url(format!("{}{path}", config::APP_ORIGIN), None::<&str>)
    {
        log::warn!("could not open the report page: {e}");
    }
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

/// Files opened from the system (Finder's or Explorer's "Open with", a
/// second launch with files): to the player, which comes forward.
pub fn open_media(app: &AppHandle, paths: &[std::path::PathBuf]) -> bool {
    if paths.is_empty() || !media::open_from_system(app, paths) {
        return false;
    }
    // macOS sends a double-clicked file just after start, when the hello
    // (Settings, signed out) may already be up: the player comes instead.
    if LAUNCHED.get().is_some_and(|t| t.elapsed() < LAUNCH_GRACE) {
        if let Some(w) = app.get_webview_window(SETTINGS) {
            let _ = w.hide();
        }
    }
    #[cfg(feature = "player-dev")]
    player_dev::open(app);
    #[cfg(not(feature = "player-dev"))]
    player_window::open(app, player_window::Opened::File);
    true
}

/// When the app started, and how soon after it a file opened from the
/// system counts as "launched to open this file".
static LAUNCHED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
const LAUNCH_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

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
    let _ = LAUNCHED.set(std::time::Instant::now());
    #[cfg(feature = "smoke-test")]
    let smoke = smoke::requested();
    #[cfg(not(feature = "smoke-test"))]
    let smoke = false;

    let mut builder = tauri::Builder::default();
    // First, so a second launch only brings this one forward. The CI smoke
    // test and the player's development page run beside an installed copy,
    // so they are not "a second launch".
    if !smoke && !cfg!(feature = "player-dev") {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // "Open with Lexpad" on Windows starts a second Lexpad with the
            // file; it hands the file here and quits.
            if open_media(app, &media::files_in_args(&args)) {
                return;
            }
            // A pinned Player's taskbar button (Windows) starts Lexpad with --player.
            if args.iter().any(|a| a == player_window::PLAYER_FLAG) {
                player_window::open(app, player_window::Opened::Launch);
            } else {
                app_menu::open_where_left_off(app);
            }
        }));
    }
    builder
        // Without its link script: the plugin's default script catches every
        // click on a target="_blank" link, cancels it and asks for its own
        // `open_url` command, which no window is granted, so the link went
        // nowhere. The click must reach the webview, whose new-window and
        // navigation handlers (main_window.rs) hand it to the browser, checked.
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        // The media engine's open dialog, called from the core only: no
        // window is granted any of the dialog plugin's own commands.
        .plugin(tauri_plugin_dialog::init())
        // The video player's media, from the core to the page (media/protocol.rs).
        .register_asynchronous_uri_scheme_protocol(
            media::protocol::SCHEME,
            |ctx, request, responder| {
                let app = ctx.app_handle().clone();
                let webview = ctx.webview_label().to_owned();
                tauri::async_runtime::spawn(async move {
                    let response = media::protocol::handle(&app, &webview, request).await;
                    // macOS: answer on the main thread. WebKit cancels a
                    // request (a seek, a closed window) on the main thread, and
                    // wry checks the task is still live before each
                    // didReceiveResponse/Data/Finish; from a worker thread the
                    // cancel can land between that check and the call, WebKit
                    // then throws "This task has already been stopped", and an
                    // Objective-C exception through Rust aborts the app (the
                    // release benchmark's "panic in a function that cannot
                    // unwind", 2026-10-25). On the main thread the check and the
                    // call cannot be split. Windows answers as before.
                    #[cfg(target_os = "macos")]
                    {
                        let main = app.clone();
                        if let Err(e) = main.run_on_main_thread(move || responder.respond(response))
                        {
                            log::warn!("media: could not answer on the main thread: {e}");
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    responder.respond(response);
                });
            },
        )
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
            // The menu bar (macOS): Playback, Subtitles, and both windows in Window.
            #[cfg(target_os = "macos")]
            if !smoke {
                let menu = app_menu::build(app.handle())?;
                app.set_menu(menu)?;
            }
            app.on_menu_event(|app, event| app_menu::handle(app, event.id().as_ref()));

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
            app.manage(Arc::new(media::MediaState::new(
                (!smoke).then(|| app.path().app_data_dir().ok()).flatten(),
            )));
            app.manage(word_queue::WordQueue::load(
                (!smoke)
                    .then(|| app.path().app_data_dir().ok())
                    .flatten()
                    .map(|d| d.join("pending-words.json")),
            ));
            app.manage(AppState {
                api: api::Api::new(config::API_ORIGIN, store),
                settings: Mutex::new(settings),
                settings_file: file,
                capture: Mutex::new(None),
                notebooks: Mutex::new(Vec::new()),
                connect_cancel: Mutex::new(None),
            });

            let handle = app.handle().clone();
            // Whether the server takes the activity headers, asked once in
            // the background; no word ever waits for it.
            handle.state::<AppState>().api.kick_activity();
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
            // Words saved while signed out or offline, sent when signed in.
            commands::flush_words(&handle);

            // Started by "Open with Lexpad" (Windows passes the file here):
            // straight into the player, without the hello below.
            let opened = open_media(
                &handle,
                &media::files_in_args(&std::env::args().collect::<Vec<_>>()),
            );
            // A pinned Player's taskbar button (Windows) starts Lexpad with --player.
            let player_flag = std::env::args().any(|a| a == player_window::PLAYER_FLAG);
            if player_flag && !opened {
                player_window::open(&handle, player_window::Opened::Launch);
            }
            // The first launch asks what to do first (spec §24.2), unless a
            // video opened Lexpad or a session is already there. Later runs
            // still say hello in Settings while signed out.
            let api_user = tauri::async_runtime::block_on(handle.state::<AppState>().api.user());
            if !opened && !player_flag && first_run && api_user.is_none() {
                welcome::open(&handle);
            } else if !opened && !player_flag && api_user.is_none() {
                show_settings(&handle);
            }
            #[cfg(feature = "player-dev")]
            if !opened {
                player_dev::open(&handle);
            }
            if open_on_launch && !opened {
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
            // Which window was last in front: where "Open Lexpad" goes back to.
            (label, WindowEvent::Focused(true)) => app_menu::note_focus(window.app_handle(), label),
            // The Player is let go when closed too; where it was is kept.
            // The page finishes closing it (and destroys it); if it never
            // does, the core closes it after a moment or on a second request.
            (player_window::LABEL, WindowEvent::CloseRequested { .. }) => {
                player_window::remember(window.app_handle());
                player_window::close_requested(window.app_handle());
            }
            (player_window::LABEL, WindowEvent::Destroyed) => {
                player_window::destroyed();
                refresh_dock(window.app_handle(), false);
            }
            (welcome::LABEL, WindowEvent::Destroyed) => {
                refresh_dock(window.app_handle(), false);
            }
            // Closing the welcome without an answer counts as Both.
            (welcome::LABEL, WindowEvent::CloseRequested { .. }) => {
                welcome::closed_without_answer(window.app_handle());
            }
            // A video dropped anywhere on Lexpad's window opens the Player
            // with it; nothing plays in the learning window (spec §24.3).
            (main_window::LABEL, WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. })) => {
                open_media(window.app_handle(), paths);
            }
            // Videos and subtitles dropped on a player window.
            (label, WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }))
                if media::protocol::PLAYER_WINDOWS.contains(&label) =>
            {
                media::deliver(window.app_handle(), label, paths);
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
            commands::open_report,
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
            commands::latest_release,
            media::commands::media_engine,
            media::commands::media_pick,
            media::commands::media_take_pending,
            media::commands::media_probe,
            media::commands::media_open,
            media::commands::media_seek,
            media::commands::media_set_audio,
            media::commands::media_close,
            media::commands::media_subtitles,
            media::commands::media_subtitle_load,
            media::commands::media_subtitle_offset,
            media::commands::media_episodes,
            media::commands::media_resume_save,
            media::commands::media_dev_report,
            media::commands::media_dev_bench_files,
            media::commands::media_default_player_status,
            media::commands::media_default_player_request,
            media::commands::media_default_player_restore,
            media::commands::media_recent,
            media::commands::media_recent_remove,
            media::commands::media_recent_clear,
            media::commands::media_set_remember_recent,
            media::commands::media_prefs,
            commands::words_save,
            commands::words_pending,
            commands::words_flush,
            commands::words_discard,
            commands::words_send_one,
            player_window::open_lexpad,
            player_window::open_player,
            player_window::player_account,
            player_window::player_fullscreen,
            player_window::player_mini_open,
            player_window::player_back_to_big,
            player_window::player_mini_close,
            player_window::player_on_top,
            player_window::player_buttons,
            player_window::player_take_play,
            welcome::welcome_state,
            welcome::welcome_choose,
            welcome::welcome_finish,
            app_menu::set_open_at,
            app_menu::open_at,
            app_menu::open_window,
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
                    player_window::remember(app);
                }
            }
            // A click on the Dock icon (macOS) opens Lexpad's window.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => app_menu::open_where_left_off(app),
            // Finder's "Open with Lexpad" (macOS sends the files as URLs).
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Opened { urls } => {
                let paths: Vec<_> = urls.iter().filter_map(|u| u.to_file_path().ok()).collect();
                open_media(app, &paths);
            }
            _ => {}
        });
}
