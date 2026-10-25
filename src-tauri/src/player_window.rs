//! The Lexpad Player's window (spec `uiux/specs/2026-10-25-video-player.md`
//! §2, §13, §24.3): its own dark window, the web app's `/player` route from
//! the same bundled build as Lexpad's window, with the same configuration
//! script, navigation lock and session rule (the page holds no session; the
//! core does). It plays what the media engine (`media/`) serves it.
//!
//! Frameless with the system's window buttons: on macOS the traffic lights
//! sit inset at (18, 18) over the page and hide with the player's chrome;
//! on Windows the page draws the caption buttons. Default size 72% of the
//! work area at 16:9, centred on the monitor under the pointer; its place is
//! remembered. The mini player is this same window made small and kept on
//! top (one decoder, one stream, nothing to keep in step), and back again.
//!
//! On Windows the Player has a taskbar button of its own: its own
//! AppUserModelID (`Lexpad.Player`), relaunching `Lexpad.exe --player`.

use std::sync::Mutex;

use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt as _;

use crate::main_window;
use crate::placement::{self, Area};
use crate::settings::WindowPlace;
use crate::state::AppState;

pub const LABEL: &str = "player";
/// The window's title (the page sets the video's title once one plays).
const TITLE: &str = "Lexpad Player";
/// The smallest the window may be (§2): a 16:9 picture still has room.
const MIN_SIZE: (f64, f64) = (480.0, 270.0);
/// The first size: this share of the work area, at 16:9 (§2).
const WORK_SHARE: f64 = 0.72;
/// The mini player (§13): 448×252, 16:9, 16 px from the screen's corner.
const MINI_SIZE: (f64, f64) = (448.0, 252.0);
const MINI_MIN: (f64, f64) = (320.0, 180.0);
const MINI_MARGIN: f64 = 16.0;
/// The Player's own taskbar identity on Windows (§24.3).
#[cfg_attr(not(windows), allow(dead_code))]
pub const APP_USER_MODEL_ID: &str = "Lexpad.Player";
/// The flag a pinned Player's taskbar button starts Lexpad with.
pub const PLAYER_FLAG: &str = "--player";

/// How the window was opened, which the page reads from its address: from
/// Lexpad's own window or the tray ("app": no first-open sign-in), with a
/// file ("file": it plays at once), to show the open dialog ("pick"), or
/// by itself at launch ("launch").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    App,
    // With `player-dev`, files open in the development page instead.
    #[cfg_attr(feature = "player-dev", allow(dead_code))]
    File,
    Pick,
    Launch,
}

impl Opened {
    fn query(self) -> &'static str {
        match self {
            Opened::App => "?from=app",
            Opened::File => "?from=file",
            Opened::Pick => "?from=pick",
            Opened::Launch => "",
        }
    }
}

/// Where the big window was before it became the mini player.
static BEFORE_MINI: Mutex<Option<Area>> = Mutex::new(None);

/// Opens the Player, or brings it forward.
pub fn open(app: &AppHandle, opened: Opened) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || open_now(&handle, opened));
}

fn open_now(app: &AppHandle, opened: Opened) {
    crate::refresh_dock(app, true);
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        if opened == Opened::Pick {
            let _ = win.emit_to(LABEL, "player:menu", "open");
        }
        return;
    }
    if let Err(e) = build(app, opened) {
        log::warn!("could not open the Player: {e}");
        crate::refresh_dock(app, false);
    }
}

/// The script that runs before the page: the app's configuration (as for
/// Lexpad's window) and the Player's route, so the router starts there.
fn init_script(device_id: &str, opened: Opened) -> String {
    format!(
        "{}\nif (!location.hash || location.hash === '#/') history.replaceState(null, '', '#/player{}');",
        main_window::config_script(device_id),
        opened.query()
    )
}

fn build(app: &AppHandle, opened: Opened) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let (device_id, saved) = {
        let s = state
            .settings
            .lock()
            .map_err(|_| tauri::Error::FailedToReceiveMessage)?;
        (s.device_id.clone(), s.player_window)
    };
    let nav = app.clone();
    let popups = app.clone();
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title(TITLE)
        .inner_size(1280.0, 720.0)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .visible(false)
        .background_color(tauri::window::Color(0, 0, 0, 255))
        .initialization_script(init_script(&device_id, opened))
        .on_navigation(move |url| {
            if main_window::is_app_page(url) {
                return true;
            }
            open_outside(&nav, url);
            false
        })
        .on_new_window(move |url, _features| {
            open_outside(&popups, &url);
            NewWindowResponse::Deny
        });
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(18.0, 18.0));
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false);
    let win = builder.build()?;
    restore(app, &win, saved);
    #[cfg(windows)]
    taskbar::own_button(&win);
    win.show()?;
    win.set_focus()?;
    Ok(())
}

fn open_outside(app: &AppHandle, url: &Url) {
    if main_window::may_open_outside(url) {
        if let Err(e) = app.opener().open_url(url.as_str(), None::<&str>) {
            log::warn!("could not open a link: {e}");
        }
    }
}

/// The first place: where it was (kept inside the work area of its monitor),
/// or 72% of the work area under the pointer at 16:9, centred.
pub fn initial_place(
    screens: &[placement::Screen],
    pointer: Option<(f64, f64)>,
    saved: Option<WindowPlace>,
    logical: bool,
) -> Option<Area> {
    if let Some(p) = saved {
        let rect = Area::new(p.x, p.y, p.w, p.h);
        let screen = placement::screen_at(screens, rect.centre())?;
        return Some(placement::clamp(rect, screen.work, 0.0));
    }
    let screen = pointer
        .and_then(|p| placement::screen_at(screens, p))
        .or_else(|| screens.first().copied())?;
    let work = screen.work;
    let mut w = work.w * WORK_SHARE;
    let mut h = w * 9.0 / 16.0;
    if h > work.h * WORK_SHARE {
        h = work.h * WORK_SHARE;
        w = h * 16.0 / 9.0;
    }
    let u = screen.unit(logical);
    let (w, h) = (w.max(MIN_SIZE.0 * u), h.max(MIN_SIZE.1 * u));
    let (cx, cy) = work.centre();
    Some(Area::new(cx - w / 2.0, cy - h / 2.0, w, h))
}

fn restore(app: &AppHandle, win: &WebviewWindow, saved: Option<WindowPlace>) {
    let screens = placement::screens(app);
    let Some(rect) = initial_place(&screens, placement::pointer(app), saved, placement::LOGICAL)
    else {
        let _ = win.center();
        return;
    };
    if let Err(e) = placement::apply(win, rect) {
        log::warn!("could not place the Player: {e}");
    }
    if saved.is_some_and(|p| p.maximized) {
        let _ = win.maximize();
    }
}

/// Remembers where the window is, before it closes or the app quits (not
/// while it is the mini player: the big window's place is kept).
pub fn remember(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else {
        return;
    };
    if win.is_minimized().unwrap_or(false) || win.is_fullscreen().unwrap_or(false) {
        return;
    }
    let mini = BEFORE_MINI.lock().ok().and_then(|b| *b);
    let maximized = win.is_maximized().unwrap_or(false);
    let rect = match mini {
        Some(big) => big,
        None => match placement::window_rect(&win) {
            Some(r) => r,
            None => return,
        },
    };
    let place = WindowPlace {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
        maximized,
    };
    if let Err(e) = app
        .state::<AppState>()
        .update_settings(|s| s.player_window = Some(place))
    {
        log::warn!("could not remember the Player's place: {e}");
    }
}

/// The mini player's place: the bottom-right corner of the work area, 16 px in.
pub fn mini_place(work: Area, unit: f64) -> Area {
    let (w, h) = (MINI_SIZE.0 * unit, MINI_SIZE.1 * unit);
    let m = MINI_MARGIN * unit;
    Area::new(work.right() - w - m, work.bottom() - h - m, w, h)
}

fn screen_of(app: &AppHandle, win: &WebviewWindow) -> Option<placement::Screen> {
    let rect = placement::window_rect(win)?;
    placement::screen_at(&placement::screens(app), rect.centre())
}

/* ------------------------------------------------------------ commands */

/// Lexpad's window, opened or brought forward, at an in-app path (a word's
/// page, a session, Settings → Video player); the path is checked as the
/// web app checks its own (`commands::app_path`).
#[tauri::command]
pub fn open_lexpad(app: AppHandle, path: Option<String>) -> Result<(), String> {
    let path = match path {
        Some(p) => {
            crate::commands::app_path(&p).ok_or("not an app path")?;
            Some(p)
        }
        None => None,
    };
    main_window::open(&app, path);
    Ok(())
}

/// A video the Player should play once it listens (a Continue watching card
/// on the Watch page): the engine's file id, never a path.
static PLAY: Mutex<Option<String>> = Mutex::new(None);

/// The Player, opened or brought forward (the Watch page, ⌘2 / Ctrl+2): to
/// the open dialog with `pick`, or to play `file_id` (an id the engine gave).
#[tauri::command]
pub fn open_player(
    app: AppHandle,
    pick: Option<bool>,
    file_id: Option<String>,
) -> Result<(), String> {
    if let Some(id) = file_id {
        if id.is_empty()
            || id.len() > 64
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err("not a file id".into());
        }
        if app.get_webview_window(LABEL).is_some() {
            let _ = app.emit_to(LABEL, "player:play", id);
        } else if let Ok(mut p) = PLAY.lock() {
            *p = Some(id);
        }
        open(&app, Opened::App);
        return Ok(());
    }
    open(
        &app,
        if pick == Some(true) {
            Opened::Pick
        } else {
            Opened::App
        },
    );
    Ok(())
}

/// The page asks once, when it starts listening, for a video it should play.
#[tauri::command]
pub fn player_take_play() -> Option<String> {
    PLAY.lock().ok().and_then(|mut p| p.take())
}

/// What the Player's card needs: signed in or not, the notebooks, and the
/// one words go to. The same reading as the desktop card's `state`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    connected: bool,
    notebooks: Vec<serde_json::Value>,
    notebook_id: Option<String>,
}

#[tauri::command]
pub async fn player_account(app: AppHandle) -> Result<Account, String> {
    let s = crate::commands::state(app.clone(), app.state::<AppState>()).await?;
    Ok(Account {
        connected: s.connected,
        notebooks: s.notebooks,
        notebook_id: s.notebook_id,
    })
}

/// Full screen: macOS's own space, Windows' borderless screen (§2, §19).
#[tauri::command]
pub fn player_fullscreen(window: WebviewWindow, on: bool) -> Result<(), String> {
    window.set_fullscreen(on).map_err(|e| e.to_string())
}

/// The mini player: small, on top, in the screen's corner (§13).
#[tauri::command]
pub fn player_mini_open(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    if window.is_fullscreen().unwrap_or(false) {
        let _ = window.set_fullscreen(false);
    }
    if let Some(big) = placement::window_rect(&window) {
        if let Ok(mut b) = BEFORE_MINI.lock() {
            b.get_or_insert(big);
        }
    }
    let screen = screen_of(&app, &window).ok_or("no screen")?;
    let unit = screen.unit(placement::LOGICAL);
    let _ = window.set_min_size(Some(tauri::LogicalSize::new(MINI_MIN.0, MINI_MIN.1)));
    placement::apply(&window, mini_place(screen.work, unit)).map_err(|e| e.to_string())?;
    window.set_always_on_top(true).map_err(|e| e.to_string())
}

/// Back to the big window, where it was.
#[tauri::command]
pub fn player_back_to_big(window: WebviewWindow) -> Result<(), String> {
    let _ = window.set_always_on_top(false);
    let _ = window.set_min_size(Some(tauri::LogicalSize::new(MIN_SIZE.0, MIN_SIZE.1)));
    let big = BEFORE_MINI.lock().ok().and_then(|mut b| b.take());
    if let Some(rect) = big {
        placement::apply(&window, rect).map_err(|e| e.to_string())?;
    }
    let _ = window.set_focus();
    Ok(())
}

/// The mini player's close button closes the mini window and the Player.
#[tauri::command]
pub fn player_mini_close(window: WebviewWindow) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

/// The mini player's pin: kept on top or not.
#[tauri::command]
pub fn player_on_top(window: WebviewWindow, on: bool) -> Result<(), String> {
    window.set_always_on_top(on).map_err(|e| e.to_string())
}

/// macOS: the traffic lights come and go with the player's chrome (§2).
#[tauri::command]
pub fn player_buttons(app: AppHandle, window: WebviewWindow, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        let win = window.clone();
        let _ = app.run_on_main_thread(move || mac::buttons(&win, visible));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, window, visible);
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2_app_kit::{NSWindow, NSWindowButton};
    use tauri::WebviewWindow;

    /// Hides or shows the three window buttons (the window's own; nothing else changes).
    pub fn buttons(win: &WebviewWindow, visible: bool) {
        let Ok(ptr) = win.ns_window() else {
            return;
        };
        if ptr.is_null() {
            return;
        }
        // SAFETY: Tauri hands out the window's NSWindow, alive while `win`
        // is; this runs on the main thread (`run_on_main_thread`).
        let ns: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
        for kind in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ] {
            if let Some(button) = ns.standardWindowButton(kind) {
                button.setHidden(!visible);
            }
        }
    }
}

#[cfg(windows)]
mod taskbar {
    use tauri::WebviewWindow;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Storage::EnhancedStorage::{
        PKEY_AppUserModel_ID, PKEY_AppUserModel_RelaunchCommand,
        PKEY_AppUserModel_RelaunchDisplayNameResource,
    };
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::UI::Shell::PropertiesSystem::{
        IPropertyStore, SHGetPropertyStoreForWindow,
    };

    /// Gives the Player's window its own taskbar button (§24.3): its own
    /// AppUserModelID, and what a pinned button starts (`Lexpad.exe --player`).
    pub fn own_button(win: &WebviewWindow) {
        let Ok(handle) = win.hwnd() else {
            return;
        };
        let hwnd = HWND(handle.0 as _);
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let relaunch = format!("\"{exe}\" {}", super::PLAYER_FLAG);
        // SAFETY: a property store for our own top-level window; every value
        // is a string PROPVARIANT that outlives the call.
        unsafe {
            let Ok(store) = SHGetPropertyStoreForWindow::<IPropertyStore>(hwnd) else {
                return;
            };
            let id = PROPVARIANT::from(super::APP_USER_MODEL_ID);
            let cmd = PROPVARIANT::from(relaunch.as_str());
            let name = PROPVARIANT::from(super::TITLE);
            let _ = store.SetValue(&PKEY_AppUserModel_ID, &id);
            let _ = store.SetValue(&PKEY_AppUserModel_RelaunchCommand, &cmd);
            let _ = store.SetValue(&PKEY_AppUserModel_RelaunchDisplayNameResource, &name);
            let _ = store.Commit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::Screen;

    fn screen(x: f64, y: f64, w: f64, h: f64) -> Screen {
        Screen {
            area: Area::new(x, y, w, h),
            work: Area::new(x, y + 25.0, w, h - 25.0 - 70.0),
            scale: 2.0,
        }
    }

    #[test]
    fn the_first_window_is_72_percent_of_the_work_area_at_16_9_centred() {
        let s = screen(0.0, 0.0, 1440.0, 900.0);
        let r = initial_place(&[s], Some((700.0, 400.0)), None, true).unwrap();
        assert!(
            (r.h - s.work.h * WORK_SHARE).abs() < 0.5 || (r.w - s.work.w * WORK_SHARE).abs() < 0.5
        );
        assert!(((r.w / r.h) - 16.0 / 9.0).abs() < 0.01);
        let (cx, cy) = r.centre();
        let (wx, wy) = s.work.centre();
        assert!((cx - wx).abs() < 0.5 && (cy - wy).abs() < 0.5);
        assert!(s.work.holds(&r));
    }

    #[test]
    fn a_remembered_place_comes_back_inside_its_monitor() {
        let s = screen(0.0, 0.0, 1440.0, 900.0);
        let saved = WindowPlace {
            x: 1200.0,
            y: 700.0,
            w: 800.0,
            h: 450.0,
            maximized: false,
        };
        let r = initial_place(&[s], None, Some(saved), true).unwrap();
        assert!(s.work.holds(&r));
        assert_eq!((r.w, r.h), (800.0, 450.0));
    }

    #[test]
    fn the_mini_player_sits_16_points_in_from_the_corner() {
        let work = Area::new(0.0, 25.0, 1440.0, 805.0);
        let r = mini_place(work, 1.0);
        assert_eq!((r.w, r.h), (448.0, 252.0));
        assert_eq!(r.right(), work.right() - 16.0);
        assert_eq!(r.bottom(), work.bottom() - 16.0);
        let px = mini_place(work, 2.0);
        assert_eq!((px.w, px.h), (896.0, 504.0));
    }

    #[test]
    fn the_route_script_starts_the_router_on_the_player() {
        let s = init_script("dev", Opened::Pick);
        assert!(s.contains("window.__LEXPAD_CONFIG"));
        assert!(s.contains("#/player?from=pick"));
        assert!(init_script("dev", Opened::Launch).ends_with("'#/player');"));
    }
}
