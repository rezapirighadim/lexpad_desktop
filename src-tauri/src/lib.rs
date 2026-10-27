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

/// Files opened from the system (Finder's or Explorer's "Open with", a drop
/// on the Dock icon, a second launch with files, the Start window's "Open a
/// video…" or a drop on it): to the Player, which comes forward and plays.
/// Nothing else opens, and a Start window already up goes (Start window spec
/// §2, state B).
pub fn open_media(app: &AppHandle, paths: &[std::path::PathBuf]) -> bool {
    if paths.is_empty() || !media::open_from_system(app, paths) {
        return false;
    }
    MEDIA_OPENED.store(true, std::sync::atomic::Ordering::SeqCst);
    note_video_opened(app);
    welcome::close(app);
    #[cfg(feature = "player-dev")]
    player_dev::open(app);
    #[cfg(not(feature = "player-dev"))]
    player_window::open(app, player_window::Opened::File);
    true
}

/// A video was opened: a later signed-out launch goes to the Player, and
/// `focus` learns of it (§3).
fn note_video_opened(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let _ = state.update_settings(|s| {
        s.played_video = true;
        s.focus = Some(welcome::focus_after_video(s.focus));
    });
}

/// Whether a file opened Lexpad (or arrived) since it started: the first
/// window then stays shut (`welcome::first_window`, state B).
static MEDIA_OPENED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The argument the login item starts Lexpad with (Start window spec §2, F).
pub const LOGIN_FLAG: &str = "--login";

/// How this process was started, for the first window's decision.
#[derive(Clone, Copy)]
struct Started {
    at_login: bool,
    player_flag: bool,
}
static STARTED: std::sync::OnceLock<Started> = std::sync::OnceLock::new();

/// What the launch table needs to know now (`welcome::first_window`).
fn launch_facts(app: &AppHandle, started: Started) -> welcome::Launch {
    let signed_in = app
        .try_state::<AppState>()
        .map(|s| tauri::async_runtime::block_on(s.api.user()).is_some())
        .unwrap_or(false);
    let (played, open_on_launch, open_at, last_place) = app
        .try_state::<AppState>()
        .and_then(|s| {
            s.settings
                .lock()
                .ok()
                .map(|s| (s.played_video, s.open_on_launch, s.open_at, s.last_place))
        })
        .unwrap_or_default();
    // An install from before `played_video` knows its videos from the
    // Player's own list of recent ones.
    let played = played
        || app
            .try_state::<Arc<media::MediaState>>()
            .is_some_and(|m| !m.resume.recent(1).is_empty());
    welcome::Launch {
        at_login: started.at_login,
        player_flag: started.player_flag,
        opened_file: MEDIA_OPENED.load(std::sync::atomic::Ordering::SeqCst),
        signed_in,
        played_video: played,
        open_on_launch,
        open_at,
        last_place,
    }
}

/// Opens what the launch table says.
fn open_first(app: &AppHandle, first: welcome::FirstWindow) {
    match first {
        welcome::FirstWindow::Nothing => {}
        welcome::FirstWindow::Start => welcome::open(app),
        welcome::FirstWindow::Player => player_window::open(app, player_window::Opened::Launch),
        welcome::FirstWindow::Learn => open_web(app, None),
    }
}

/// Lexpad opened again while running (the Dock icon, a second launch, the
/// Start menu): the same table as a launch, as it stands now.
/// The facts are read off the main thread (the session read may wait on a
/// token refresh); the window is opened from there as usual.
pub fn open_again(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let facts = launch_facts(
            &app,
            Started {
                at_login: false,
                player_flag: false,
            },
        );
        // A video opened earlier in this run does not keep Lexpad shut now.
        let facts = welcome::Launch {
            opened_file: false,
            ..facts
        };
        open_first(&app, welcome::first_window(facts));
    });
}

/// The first window, once the app is ready and the system has had
/// `FIRST_WINDOW_GRACE` to hand over a video that opened Lexpad (macOS sends
/// it as `RunEvent::Opened` after `setup`). Off the main thread: the session
/// is read from the credential store.
fn decide_first_window(app: &AppHandle) {
    let Some(started) = STARTED.get().copied() else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(welcome::FIRST_WINDOW_GRACE);
        let facts = launch_facts(&app, started);
        let first = welcome::first_window(facts);
        // Asked again on the main thread, where a late `RunEvent::Opened`
        // is handled: a video that arrived meanwhile is the only window.
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if !MEDIA_OPENED.load(std::sync::atomic::Ordering::SeqCst) {
                open_first(&handle, first);
            }
        });
    });
}

/// Files the system handed over before the app had set up (macOS can send
/// `RunEvent::Opened` that early); `setup` plays them.
static EARLY_FILES: Mutex<Vec<std::path::PathBuf>> = Mutex::new(Vec::new());

/// Lexpad is quitting: no hint, no window, from here on.
static EXITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// One hint at a time: two windows closing together never show it twice.
static HINT_IN_FLIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The first time the last of Lexpad's windows closes, once per install:
/// where Lexpad went (Start window spec §6). A moment later, so a window
/// that replaces another (the Start window giving way to the Player) counts.
fn after_window_closed(app: &AppHandle) {
    if EXITING.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let shown = app
        .try_state::<AppState>()
        .and_then(|s| s.settings.lock().ok().map(|s| s.tray_hint_shown))
        .unwrap_or(true);
    if shown {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        let open = [
            SETTINGS,
            main_window::LABEL,
            player_window::LABEL,
            welcome::LABEL,
        ]
        .iter()
        .any(|l| {
            app.get_webview_window(l)
                .and_then(|w| w.is_visible().ok())
                .unwrap_or(false)
        });
        if EXITING.load(std::sync::atomic::Ordering::SeqCst)
            || HINT_IN_FLIGHT.swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return;
        }
        let state = app.state::<AppState>();
        let due = state
            .settings
            .lock()
            .map(|s| {
                settings::tray_hint_due(&s, open, EXITING.load(std::sync::atomic::Ordering::SeqCst))
            })
            .unwrap_or(false);
        // Marked shown only once it really showed (no tray icon yet, or
        // notifications off on Windows: it waits for the next close).
        if due && tray::still_here(&app) {
            let _ = state.update_settings(|s| s.tray_hint_shown = true);
        }
        HINT_IN_FLIGHT.store(false, std::sync::atomic::Ordering::SeqCst);
    });
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
    // test and the player's development page run beside an installed copy,
    // so they are not "a second launch".
    if !smoke && !cfg!(feature = "player-dev") {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // "Open with Lexpad" on Windows starts a second Lexpad with the
            // file; it hands the file here and quits.
            if open_media(app, &media::files_in_args(&args)) {
                return;
            }
            // "Lexpad Player" in the Start menu, or a pinned Player's taskbar
            // button (Windows), starts Lexpad with --player.
            if args.iter().any(|a| a == player_window::PLAYER_FLAG) {
                player_window::open(app, player_window::Opened::Launch);
            } else {
                open_again(app);
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
        // The login item starts Lexpad with --login, so a start at login
        // opens no window and a start by hand does (§2, states C and F).
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![LOGIN_FLAG]),
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
            let first_run_here = first_run && !smoke;
            let store = session_store(smoke);
            let shortcut = settings.shortcut.clone();
            let start_on_login = settings.start_on_login;
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
            // Only when the learner turned it on (never on a first run: no
            // "Background Items Added" before they did anything, §6). Applied
            // again each start, so the item carries --login after an update.
            // Only when the learner asked, or to take away a login item an
            // old install left behind (settings::login_item_action).
            if let Some(on) = settings::login_item_action(start_on_login, first_run_here) {
                apply_start_on_login(&handle, on);
            }
            #[cfg(target_os = "macos")]
            services_macos::register(&handle);
            notify::init(&handle);
            notify::start(&handle);
            // Words saved while signed out or offline, sent when signed in.
            commands::flush_words(&handle);

            // Started by "Open with Lexpad" (Windows passes the file here, a
            // Mac may have handed it over before setup): straight into the
            // Player, and nothing else (§2, state B).
            let mut files = media::files_in_args(&std::env::args().collect::<Vec<_>>());
            files.extend(
                EARLY_FILES
                    .lock()
                    .map(|mut f| std::mem::take(&mut *f))
                    .unwrap_or_default(),
            );
            let opened = open_media(&handle, &files);
            // "Lexpad Player" in the Start menu or a pinned Player's taskbar
            // button (Windows) starts Lexpad with --player (state G).
            let player_flag = std::env::args().any(|a| a == player_window::PLAYER_FLAG);
            if player_flag && !opened {
                player_window::open(&handle, player_window::Opened::Launch);
            }
            #[cfg(windows)]
            let store_login = msix::started_at_login();
            #[cfg(not(windows))]
            let store_login = false;
            // Every other first window is decided once the app is ready
            // (decide_first_window): the Start window, the Player, Lexpad's
            // window or nothing, by the launch table.
            #[cfg(not(feature = "player-dev"))]
            let _ = STARTED.set(Started {
                at_login: store_login || std::env::args().any(|a| a == LOGIN_FLAG),
                player_flag,
            });
            #[cfg(feature = "player-dev")]
            {
                let _ = store_login;
                if !opened {
                    player_dev::open(&handle);
                }
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
                after_window_closed(window.app_handle());
            }
            // Lexpad's window is let go when closed; where it was is kept.
            (main_window::LABEL, WindowEvent::CloseRequested { .. }) => {
                main_window::remember(window.app_handle());
            }
            (main_window::LABEL, WindowEvent::Destroyed) => {
                refresh_dock(window.app_handle(), false);
                after_window_closed(window.app_handle());
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
                after_window_closed(window.app_handle());
            }
            // Closing the Start window sets nothing and opens nothing (§3).
            (welcome::LABEL, WindowEvent::Destroyed) => {
                refresh_dock(window.app_handle(), false);
                after_window_closed(window.app_handle());
            }
            // A video dropped on the Start window plays; anything else is
            // refused there, with the reason (§5.4).
            (welcome::LABEL, WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. })) => {
                welcome::dropped(window.app_handle(), paths);
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
            player_window::player_context_menu,
            player_window::player_copy_text,
            welcome::welcome_state,
            welcome::start_open_video,
            welcome::start_signed_in,
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
                    EXITING.store(true, std::sync::atomic::Ordering::SeqCst);
                    main_window::remember(app);
                    player_window::remember(app);
                }
            }
            // The first window, by the launch table, after the grace for a
            // video the system is still handing over.
            tauri::RunEvent::Ready => decide_first_window(app),
            // A click on the Dock icon (macOS): as a launch, as things stand.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => open_again(app),
            // Finder's "Open with Lexpad" (macOS sends the files as URLs).
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Opened { urls } => {
                let paths: Vec<_> = urls.iter().filter_map(|u| u.to_file_path().ok()).collect();
                // Before setup there is no engine yet: setup plays them.
                if app.try_state::<AppState>().is_none() {
                    if let Ok(mut early) = EARLY_FILES.lock() {
                        early.extend(paths);
                    }
                    return;
                }
                open_media(app, &paths);
            }
            _ => {}
        });
}
