//! Lexpad's own window: the whole web app (Today, practice, the notebook,
//! Lex, Progress, Settings), from the build bundled in the app (`web/`),
//! working offline from its own IndexedDB like the phone apps.
//!
//! The page holds no session. The core injects `window.__LEXPAD_CONFIG`
//! (platform `desktop`, the API, this app's version and device id) before
//! the page runs, and the page sends every API call back through the core
//! (`api_fetch`, checked by `proxy.rs`). The window may navigate only within
//! the app's own pages; any other address is opened in the system browser
//! (https and mailto only), never inside it. A file the page saves (an
//! export) goes to the Downloads folder.
//!
//! The window is made when it is first opened and destroyed when it is
//! closed, so a Lexpad in the menu bar costs nothing while it is shut; its
//! place on screen is remembered and brought back inside the work area of
//! the monitor it was on (or the nearest one, if that is gone).

use std::sync::Mutex;

use serde_json::json;
use tauri::webview::{DownloadEvent, NewWindowResponse};
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt as _;

use crate::config;
use crate::placement::{self, Area};
use crate::settings::WindowPlace;
use crate::state::AppState;

pub const LABEL: &str = "main";
/// The desktop section of Settings in the web app.
pub const DESKTOP_SETTINGS: &str = "/settings/desktop";
/// The window's size the first time, in points: the web app's two-column
/// layouts have room, and it fits a 1280 x 800 screen.
const FIRST_SIZE: (f64, f64) = (1040.0, 760.0);
/// The smallest it may be made: the web app's phone layout.
const MIN_SIZE: (f64, f64) = (360.0, 520.0);
/// How far a new window keeps from the edges of the work area, in points.
const MARGIN: f64 = 24.0;

/// A page the window should open once its page is listening (a word from
/// the panel, opened before the window had loaded).
static PENDING: Mutex<Option<String>> = Mutex::new(None);

/// Opens the window, or brings it forward, at `path` (an in-app path such
/// as `/words/<id>`) when there is one.
pub fn open(app: &AppHandle, path: Option<String>) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || open_now(&handle, path));
}

fn open_now(app: &AppHandle, path: Option<String>) {
    crate::refresh_dock(app, true);
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        if let Some(p) = path {
            let _ = win.emit_to(LABEL, "main:open", p);
        }
        return;
    }
    if let Ok(mut pending) = PENDING.lock() {
        *pending = path;
    }
    if let Err(e) = build(app) {
        log::warn!("could not open Lexpad's window: {e}");
        crate::refresh_dock(app, false);
    }
}

/// The page asks once, when it starts listening, for a page it should open.
pub fn take_pending() -> Option<String> {
    PENDING.lock().ok().and_then(|mut p| p.take())
}

/// What the page is told before it runs.
fn config_script(device_id: &str) -> String {
    let config = json!({
        "platform": "desktop",
        "apiUrl": config::API_ORIGIN,
        "appVersion": config::VERSION,
        "deviceId": device_id,
    });
    format!("window.__LEXPAD_CONFIG = Object.freeze({config});")
}

/// Whether `url` is one of the app's own pages: the bundled build served by
/// Tauri (`tauri://localhost` on macOS, `http(s)://tauri.localhost` on
/// Windows), or the development server in a debug build.
pub fn is_app_page(url: &Url) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => {
            url.host_str() == Some("tauri.localhost")
                || (cfg!(debug_assertions)
                    && url.host_str() == Some("localhost")
                    && url.port() == Some(1420))
        }
        "about" => url.as_str() == "about:blank",
        _ => false,
    }
}

/// What may be opened outside the app from the window: a web page (https)
/// or an e-mail address (mailto). Nothing else, not even plain http.
pub fn may_open_outside(url: &Url) -> bool {
    matches!(url.scheme(), "https" | "mailto")
        && url.username().is_empty()
        && url.password().is_none()
}

fn open_outside(app: &AppHandle, url: &Url) {
    if may_open_outside(url) {
        if let Err(e) = app.opener().open_url(url.as_str(), None::<&str>) {
            log::warn!("could not open a link: {e}");
        }
    }
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let (device_id, saved) = {
        let s = state
            .settings
            .lock()
            .map_err(|_| tauri::Error::FailedToReceiveMessage)?;
        (s.device_id.clone(), s.main_window)
    };
    let nav = app.clone();
    let popups = app.clone();
    let downloads = app.path().download_dir().ok();
    let win = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Lexpad")
        .inner_size(FIRST_SIZE.0, FIRST_SIZE.1)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .visible(false)
        .initialization_script(config_script(&device_id))
        .on_navigation(move |url| {
            if is_app_page(url) {
                return true;
            }
            open_outside(&nav, url);
            false
        })
        .on_new_window(move |url, _features| {
            open_outside(&popups, &url);
            NewWindowResponse::Deny
        })
        .on_download(move |_webview, event| {
            if let DownloadEvent::Requested { destination, .. } = event {
                let name = destination
                    .file_name()
                    .map(|n| n.to_owned())
                    .unwrap_or_else(|| "Lexpad export".into());
                if let Some(dir) = &downloads {
                    *destination = dir.join(name);
                }
            }
            true
        })
        .build()?;
    restore(app, &win, saved);
    win.show()?;
    win.set_focus()?;
    Ok(())
}

/// The window's frame around its content, in placement units: left, top,
/// and the extra width and height (the title bar; on Windows also the
/// invisible resize borders).
fn frame(win: &WebviewWindow) -> Option<(f64, f64, f64, f64)> {
    let outer = outer_rect(win)?;
    let inner = placement::window_rect(win)?;
    Some((
        inner.x - outer.x,
        inner.y - outer.y,
        outer.w - inner.w,
        outer.h - inner.h,
    ))
}

/// The whole window, frame included, in placement units.
fn outer_rect(win: &WebviewWindow) -> Option<Area> {
    let pos = win.outer_position().ok()?;
    let size = win.outer_size().ok()?;
    let by = if placement::LOGICAL {
        1.0 / win.scale_factor().ok()?
    } else {
        1.0
    };
    Some(
        Area::new(
            pos.x as f64,
            pos.y as f64,
            size.width as f64,
            size.height as f64,
        )
        .scaled(by),
    )
}

/// Where the window goes: where it was, brought inside the work area of the
/// monitor it was on (or the nearest), or, the first time, centred on the
/// monitor under the pointer. The whole frame stays inside, title bar too.
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
    let u = screen.unit(logical);
    let inner = screen.work.inset(MARGIN * u);
    let (w, h) = (
        (FIRST_SIZE.0 * u).min(inner.w),
        (FIRST_SIZE.1 * u).min(inner.h),
    );
    let (cx, cy) = inner.centre();
    Some(Area::new(cx - w / 2.0, cy - h / 2.0, w, h))
}

fn restore(app: &AppHandle, win: &WebviewWindow, saved: Option<WindowPlace>) {
    let screens = placement::screens(app);
    let Some(rect) = initial_place(&screens, placement::pointer(app), saved, placement::LOGICAL)
    else {
        let _ = win.center();
        return;
    };
    let Some((left, top, extra_w, extra_h)) = frame(win) else {
        return;
    };
    let content = Area::new(
        rect.x + left,
        rect.y + top,
        (rect.w - extra_w).max(1.0),
        (rect.h - extra_h).max(1.0),
    );
    if let Err(e) = placement::apply(win, content) {
        log::warn!("could not place Lexpad's window: {e}");
    }
    if saved.is_some_and(|p| p.maximized) {
        let _ = win.maximize();
    }
}

/// Remembers where the window is, before it closes or the app quits.
pub fn remember(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else {
        return;
    };
    let maximized = win.is_maximized().unwrap_or(false);
    let minimized = win.is_minimized().unwrap_or(false);
    if minimized {
        return;
    }
    let Some(rect) = outer_rect(&win) else {
        return;
    };
    let state = app.state::<AppState>();
    let place = WindowPlace {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
        maximized,
    };
    // A maximized window keeps the place it had before, to go back to.
    let keep = state
        .settings
        .lock()
        .ok()
        .and_then(|s| s.main_window)
        .filter(|_| maximized);
    let place = match keep {
        Some(before) => WindowPlace {
            maximized: true,
            ..before
        },
        None => place,
    };
    if let Err(e) = state.update_settings(|s| s.main_window = Some(place)) {
        log::warn!("could not remember the window's place: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::Screen;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn the_window_stays_on_the_apps_own_pages() {
        assert!(is_app_page(&url("tauri://localhost/index.html#/today")));
        assert!(is_app_page(&url("http://tauri.localhost/index.html")));
        assert!(is_app_page(&url("https://tauri.localhost/")));
        assert!(is_app_page(&url("about:blank")));
        assert!(!is_app_page(&url("https://app.lexpad.app/today")));
        assert!(!is_app_page(&url("https://tauri.localhost.evil.example/")));
        assert!(!is_app_page(&url("file:///etc/passwd")));
        assert!(!is_app_page(&url("tauri://evil/")));
    }

    #[test]
    fn only_web_pages_and_mail_leave_for_the_system() {
        assert!(may_open_outside(&url("https://lexpad.app/features")));
        assert!(may_open_outside(&url("mailto:hello@lexpad.app")));
        assert!(!may_open_outside(&url("http://lexpad.app/")));
        assert!(!may_open_outside(&url(
            "file:///Applications/Calculator.app"
        )));
        assert!(!may_open_outside(&url("smb://server/share")));
        assert!(!may_open_outside(&url("https://user:pw@lexpad.app/")));
    }

    #[test]
    fn the_config_is_data_not_code() {
        let script = config_script("d\"; alert(1); \"");
        assert!(script.starts_with("window.__LEXPAD_CONFIG = Object.freeze({"));
        // The id is a JSON string: its quote is escaped.
        assert!(script.contains(r#"d\"; alert(1); \""#));
        assert!(script.contains(r#""platform":"desktop""#));
    }

    fn mac() -> Screen {
        Screen {
            area: Area::new(0.0, 0.0, 1512.0, 982.0),
            work: Area::new(0.0, 37.0, 1512.0, 875.0),
            scale: 2.0,
        }
    }

    #[test]
    fn the_first_time_it_is_centred_on_the_monitor_under_the_pointer_and_fits() {
        let small = Screen {
            area: Area::new(1512.0, 0.0, 1280.0, 720.0),
            work: Area::new(1512.0, 25.0, 1280.0, 695.0),
            scale: 1.0,
        };
        let r = initial_place(&[mac(), small], Some((2000.0, 300.0)), None, true).unwrap();
        assert!(small.work.holds(&r));
        assert_eq!(r.w, 1040.0);
        assert_eq!(r.h, 695.0 - 48.0);
        let r = initial_place(&[mac(), small], Some((100.0, 100.0)), None, true).unwrap();
        assert!(mac().work.holds(&r));
    }

    #[test]
    fn a_remembered_place_comes_back_inside_the_work_area() {
        // Saved on a monitor that is now gone, far to the right.
        let gone = WindowPlace {
            x: 4000.0,
            y: 200.0,
            w: 1200.0,
            h: 900.0,
            maximized: false,
        };
        let r = initial_place(&[mac()], None, Some(gone), true).unwrap();
        assert!(mac().work.holds(&r));
        // Saved over the menu bar and the Dock on this one.
        let over = WindowPlace {
            x: 100.0,
            y: 0.0,
            w: 800.0,
            h: 982.0,
            maximized: false,
        };
        let r = initial_place(&[mac()], None, Some(over), true).unwrap();
        assert!(mac().work.holds(&r));
        assert_eq!(r.y, 37.0);
        // Where it was, when that still fits.
        let fine = WindowPlace {
            x: 200.0,
            y: 100.0,
            w: 900.0,
            h: 700.0,
            maximized: false,
        };
        assert_eq!(
            initial_place(&[mac()], None, Some(fine), true),
            Some(Area::new(200.0, 100.0, 900.0, 700.0))
        );
    }

    #[test]
    fn on_windows_at_150_percent_the_first_size_is_in_pixels() {
        let s = Screen {
            area: Area::new(-2880.0, 0.0, 2880.0, 1800.0),
            work: Area::new(-2880.0, 0.0, 2880.0, 1728.0),
            scale: 1.5,
        };
        let r = initial_place(&[s], Some((-100.0, 100.0)), None, false).unwrap();
        assert_eq!((r.w, r.h), (1560.0, 1140.0));
        assert!(s.work.holds(&r));
    }
}
