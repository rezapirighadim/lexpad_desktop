//! The add-a-word popup: shown near the selection (or the pointer), always
//! on top, frameless, with the card. Esc or a click elsewhere hides it and
//! hands focus back to the app the word came from.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, LogicalSize, Manager, WebviewWindow};

use crate::capture::{self, Capture};
use crate::placement;
use crate::state::AppState;

pub const LABEL: &str = "popup";
/// The card's width, in points; the height follows the content (fit_popup).
pub const WIDTH: f64 = 360.0;
pub const MIN_HEIGHT: f64 = 120.0;
pub const MAX_HEIGHT: f64 = 620.0;
/// How far the popup sits from the selection or the pointer, in points.
const GAP: f64 = 12.0;
/// How far the popup keeps from the edges of the work area, in points: off
/// the Dock and the taskbar by a hair, as a system menu is.
const MARGIN: f64 = 8.0;

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
    give_focus_back(app);
}

/// Gives the keyboard back to the app that was in front before Lexpad.
/// Windows does that by itself when the window hides.
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
pub fn give_focus_back(app: &AppHandle) {
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

/// Where the popup is and which side of its anchor it opened on, so it can
/// grow without leaving the work area.
struct Shown {
    placed: placement::Placed,
    screen: placement::Screen,
}

static SHOWN: Mutex<Option<Shown>> = Mutex::new(None);

/// The popup's height in points before the card has measured itself.
const FIRST_HEIGHT: f64 = 420.0;

/// Where the popup goes: below the selection when the platform says where it
/// is, otherwise below and to the right of the pointer, on the monitor under
/// it and wholly inside that monitor's work area (never under the menu bar,
/// the Dock or the taskbar). Nothing is remembered from an earlier opening.
fn place(app: &AppHandle, win: &WebviewWindow, capture: &Capture) {
    // An anchor (the selection, or the panel's icon) is in placement units.
    match (capture.anchor, placement::pointer(app)) {
        (Some(r), _) => place_at(app, win, placement::Area::new(r.x, r.y, r.w, r.h), false),
        (None, Some((x, y))) => place_at(app, win, placement::Area::new(x, y, 0.0, 0.0), true),
        (None, None) => {}
    }
}

/// Places the popup by an anchor in placement units; `beside` for a pointer.
pub fn place_at(app: &AppHandle, win: &WebviewWindow, anchor: placement::Area, beside: bool) {
    let screens = placement::screens(app);
    let Some(screen) = placement::screen_at(&screens, anchor.centre()) else {
        return;
    };
    let height = placement::window_rect(win)
        .map(|r| r.h / screen.unit(placement::LOGICAL))
        .filter(|h| *h >= MIN_HEIGHT)
        .unwrap_or(FIRST_HEIGHT);
    let placed = place_on(&screen, anchor, height, beside);
    let _ = placement::apply(win, placed.rect);
    if let Ok(mut s) = SHOWN.lock() {
        *s = Some(Shown { placed, screen });
    }
}

/// The popup's rectangle for an anchor on a screen, `height` points tall.
pub fn place_on(
    screen: &placement::Screen,
    anchor: placement::Area,
    height: f64,
    beside: bool,
) -> placement::Placed {
    let u = screen.unit(placement::LOGICAL);
    placement::near(
        anchor,
        WIDTH * u,
        height.min(MAX_HEIGHT) * u,
        screen.work,
        GAP * u,
        MARGIN * u,
        beside,
    )
}

/// The card measured itself at `height` points: the window takes that
/// height, cut to the work area (the card then scrolls, with its buttons
/// kept in view), and keeps the edge on its anchor's side; a card the
/// learner dragged keeps its top. Either way it stays inside the work area.
pub fn fit(app: &AppHandle, height: f64) -> tauri::Result<()> {
    let Some(win) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    let h = height.clamp(MIN_HEIGHT, MAX_HEIGHT);
    let current = placement::window_rect(&win);
    let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    let (screen, side, current) = match (shown.as_ref(), current) {
        (Some(s), Some(cur)) => {
            let at = s.placed.rect;
            let moved = (cur.x - at.x).abs() > 2.0 || (cur.y - at.y).abs() > 2.0;
            if moved {
                // Dragged: the monitor it is on now, its top kept.
                let screens = placement::screens(app);
                let screen = placement::screen_at(&screens, cur.centre()).unwrap_or(s.screen);
                (screen, placement::Side::Below, cur)
            } else {
                (s.screen, s.placed.side, at)
            }
        }
        (None, Some(cur)) => {
            let screens = placement::screens(app);
            let Some(screen) = placement::screen_at(&screens, cur.centre()) else {
                return win.set_size(LogicalSize::new(WIDTH, h));
            };
            (screen, placement::Side::Below, cur)
        }
        (_, None) => return win.set_size(LogicalSize::new(WIDTH, h)),
    };
    let u = screen.unit(placement::LOGICAL);
    let rect = placement::regrow(
        placement::Area::new(current.x, current.y, WIDTH * u, current.h),
        side,
        h * u,
        screen.work,
        MARGIN * u,
    );
    placement::apply(&win, rect)?;
    *shown = Some(Shown {
        placed: placement::Placed { rect, side },
        screen,
    });
    Ok(())
}
