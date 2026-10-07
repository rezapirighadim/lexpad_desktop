//! The add-a-word popup: shown near the selection (or the pointer), always
//! on top, frameless, with the card. Esc or a click elsewhere hides it and
//! hands focus back to the app the word came from.

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::capture::{self, Capture};
use crate::state::AppState;

pub const LABEL: &str = "popup";
/// The card's width, in points; the height follows the content (fit_popup).
pub const WIDTH: f64 = 360.0;
pub const MIN_HEIGHT: f64 = 120.0;
pub const MAX_HEIGHT: f64 = 620.0;
/// How far the popup sits from the selection or the pointer, in points.
const GAP: f64 = 12.0;

/// The shortcut was pressed: read the selection off the UI thread, then show
/// the popup with it.
pub fn trigger(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let capture = capture::capture();
        open(&app, capture);
    });
}

/// Shows the popup for a capture (from the shortcut, the Services menu or
/// the tray's "Add a word").
pub fn open(app: &AppHandle, capture: Capture) {
    if let Some(state) = app.try_state::<AppState>() {
        *state.capture.lock().expect("capture lock") = Some(capture.clone());
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(win) = handle.get_webview_window(LABEL) else {
            return;
        };
        place(&handle, &win, &capture);
        let _ = win.show();
        let _ = win.set_focus();
        let _ = win.emit("popup:open", ());
    });
}

/// Hides the popup and gives the keyboard back to the app in front before it.
pub fn hide(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.hide();
    }
    #[cfg(target_os = "macos")]
    {
        // Hiding the (Dock-less) app makes macOS reactivate the previous app,
        // unless the Settings window is what the learner is working in.
        let settings_open = app
            .get_webview_window(crate::SETTINGS)
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false);
        if !settings_open {
            let _ = app.hide();
        }
    }
}

/// Where the popup goes: below the selection when the platform says where it
/// is, otherwise beside the pointer; kept on the screen it is on.
fn place(app: &AppHandle, win: &WebviewWindow, capture: &Capture) {
    let Ok(cursor) = app.cursor_position() else {
        return;
    };
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| win.current_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let scale = monitor.scale_factor();
    let size = win.outer_size().unwrap_or(PhysicalSize::new(
        (WIDTH * scale) as u32,
        (400.0 * scale) as u32,
    ));
    let (w, h) = (size.width as f64, size.height as f64);
    let gap = GAP * scale;

    // An anchor is in points from the top-left of the main screen.
    let (ax, ay, ah) = match capture.anchor {
        Some(r) => (r.x * scale, r.y * scale, r.h * scale),
        None => (cursor.x, cursor.y, 0.0),
    };
    let area = monitor.work_area();
    let (left, top) = (area.position.x as f64, area.position.y as f64);
    let (right, bottom) = (left + area.size.width as f64, top + area.size.height as f64);

    let mut x = ax + if capture.anchor.is_some() { 0.0 } else { gap };
    let mut y = ay + ah + gap;
    if x + w > right {
        x = (ax - w - gap).max(left);
    }
    if y + h > bottom {
        y = (ay - h - gap).max(top);
    }
    let _ = win.set_position(PhysicalPosition::new(x.max(left), y.max(top)));
}
