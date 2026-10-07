//! The menu-bar / tray panel: a small rounded window that opens under the
//! menu-bar icon (macOS) or above the tray icon (Windows) on a left click.
//! It holds the "Add a word" box, the shortcut, the last words added from
//! this computer, the default notebook, and Open Lexpad / Settings / Quit.
//! A click elsewhere or Esc puts it away, like a menu.
//!
//! Placement works from the icon's own rectangle, so it is right on any
//! monitor, with the taskbar on any edge, and under a notched menu bar (the
//! icon's bottom is the bar's bottom, however tall the bar is).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::capture;

pub const LABEL: &str = "panel";
/// The panel's width, in points; the height follows the content (fit_panel).
pub const WIDTH: f64 = 340.0;
pub const MIN_HEIGHT: f64 = 160.0;
pub const MAX_HEIGHT: f64 = 640.0;
/// Between the icon and the panel, in points: close enough to read as the
/// icon's own, as a system menu does.
const GAP: f64 = 6.0;
/// A click on the icon that closes the panel first takes its focus away, and
/// the blur hides it; the click itself must not open it again. A click within
/// this long of a blur is that same click. Long enough for the slowest
/// round trip from the blur to the click event, short enough that a person
/// clicking twice on purpose is not ignored.
const REOPEN_GUARD: Duration = Duration::from_millis(300);

use crate::placement::{self, Side};
pub use crate::placement::{Area, Screen};

/// The screen the icon is on, as an index into `screens`, and that screen
/// with the icon in the units placement works in.
///
/// macOS reports every rectangle as points times the scale of the screen it
/// is on, so a rectangle only makes sense divided by its own screen's scale:
/// each screen is tried in its own points. Windows reports one physical
/// space for the whole desktop, so `logical` is false and nothing is divided.
pub fn screen_for(screens: &[Screen], icon: Area, logical: bool) -> Option<(usize, Screen, Area)> {
    let unit = |s: &Screen| if logical { 1.0 / s.scale } else { 1.0 };
    let in_units = |s: &Screen| Screen {
        area: s.area.scaled(unit(s)),
        work: s.work.scaled(unit(s)),
        scale: s.scale,
    };
    screens
        .iter()
        .enumerate()
        .find(|(_, s)| in_units(s).area.contains(icon.scaled(unit(s)).centre()))
        .map(|(i, s)| (i, in_units(s), icon.scaled(unit(s))))
}

/// Where the panel's top-left corner goes for an icon, all in one unit.
///
/// The icon is outside the work area on the side its bar is on: above it
/// for the macOS menu bar or a top taskbar, below it for the usual Windows
/// taskbar, left or right of it for a taskbar on a side. The panel opens
/// from that edge, centred on the icon, and is kept inside the work area.
/// An icon inside the work area (a Windows overflow flyout) opens the panel
/// below it in the top half of the screen and above it in the bottom half.
pub fn place(icon: Area, w: f64, h: f64, screen: &Screen, gap: f64) -> (f64, f64) {
    let work = screen.work;
    let h = h.min(work.h);
    let (cx, cy) = icon.centre();
    let clamp_x = |x: f64| x.min(work.right() - w).max(work.x);
    let clamp_y = |y: f64| y.min(work.bottom() - h).max(work.y);
    let below = || {
        (
            clamp_x(cx - w / 2.0),
            clamp_y(icon.bottom().max(work.y) + gap),
        )
    };
    let above = || {
        (
            clamp_x(cx - w / 2.0),
            clamp_y(icon.y.min(work.bottom()) - gap - h),
        )
    };
    if cy < work.y {
        below()
    } else if cy >= work.bottom() {
        above()
    } else if cx < work.x {
        (
            clamp_x(icon.right().max(work.x) + gap),
            clamp_y(cy - h / 2.0),
        )
    } else if cx >= work.right() {
        (
            clamp_x(icon.x.min(work.right()) - gap - w),
            clamp_y(cy - h / 2.0),
        )
    } else if cy < screen.area.centre().1 {
        below()
    } else {
        above()
    }
}

/// Whether a click on the icon is the one that just closed the panel.
pub fn is_closing_click(hidden_at: Option<Instant>, now: Instant) -> bool {
    hidden_at.is_some_and(|t| now.saturating_duration_since(t) < REOPEN_GUARD)
}

struct Shown {
    hidden_at: Option<Instant>,
    /// The panel's height in points, as the page last asked for it.
    height: f64,
    /// The icon's rectangle in placement units, where the panel last opened:
    /// the card for a typed word opens from the same place.
    icon: Option<capture::Rect>,
    /// The screen the panel is on (placement units), and whether it opened
    /// upward from the icon, so it can grow and stay on the icon.
    screen: Option<Screen>,
    side: Side,
}

static SHOWN: Mutex<Shown> = Mutex::new(Shown {
    hidden_at: None,
    height: 420.0,
    icon: None,
    screen: None,
    side: Side::Below,
});

/// Makes the (hidden) panel window. On macOS it is transparent, so the page
/// draws the rounded corners and the system's shadow follows them; on
/// Windows 11 an undecorated window with a shadow gets rounded corners and a
/// hairline border from the system itself.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("panel.html".into()))
        .title("Lexpad")
        .inner_size(WIDTH, 420.0)
        .visible(false)
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .shadow(true);
    #[cfg(target_os = "macos")]
    let builder = builder.transparent(true);
    builder.build()?;
    Ok(())
}

/// A left click on the icon: opens the panel there, or closes it.
pub fn toggle(app: &AppHandle, icon: tauri::Rect) {
    let Some(win) = app.get_webview_window(LABEL) else {
        return;
    };
    if win.is_visible().unwrap_or(false) {
        hide(app, true);
        return;
    }
    let hidden_at = SHOWN.lock().map(|s| s.hidden_at).unwrap_or(None);
    if is_closing_click(hidden_at, Instant::now()) {
        return;
    }
    show(app, icon);
}

pub(crate) fn show(app: &AppHandle, icon: tauri::Rect) {
    let Some(win) = app.get_webview_window(LABEL) else {
        return;
    };
    let pos = icon.position.to_physical::<f64>(1.0);
    let size = icon.size.to_physical::<f64>(1.0);
    let icon = Area::new(pos.x, pos.y, size.width, size.height);
    // As Tauri reports them: each screen in its own scale (see screen_for).
    let screens: Vec<Screen> = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| Screen {
            area: Area::new(
                m.position().x as f64,
                m.position().y as f64,
                m.size().width as f64,
                m.size().height as f64,
            ),
            work: Area::new(
                m.work_area().position.x as f64,
                m.work_area().position.y as f64,
                m.work_area().size.width as f64,
                m.work_area().size.height as f64,
            ),
            scale: m.scale_factor(),
        })
        .collect();
    let height = SHOWN.lock().map(|s| s.height).unwrap_or(420.0);
    if let Some((_, screen, icon_units)) = screen_for(&screens, icon, placement::LOGICAL) {
        let rect = rect_for(&screen, icon_units, height, placement::LOGICAL);
        let _ = placement::apply(&win, rect);
        if let Ok(mut s) = SHOWN.lock() {
            s.icon = Some(capture::Rect {
                x: icon_units.x,
                y: icon_units.y,
                w: icon_units.w,
                h: icon_units.h,
            });
            s.screen = Some(screen);
            s.side = if rect.y < icon_units.y {
                Side::Above
            } else {
                Side::Below
            };
        }
    }
    let _ = win.show();
    let _ = win.set_focus();
    let _ = win.emit("panel:open", ());
}

/// The panel's rectangle for an icon on a screen (placement units), cut to
/// the work area when the panel is taller than it (the page then scrolls).
pub fn rect_for(screen: &Screen, icon: Area, height: f64, logical: bool) -> Area {
    // Points on macOS; on Windows the panel's size in that screen's pixels.
    let unit = screen.unit(logical);
    let (w, h) = (WIDTH * unit, (height * unit).min(screen.work.h));
    let (x, y) = place(icon, w, h, screen, GAP * unit);
    Area::new(x, y, w, h)
}

/// Puts the panel away. With `give_back` (Esc, a button), the app the
/// learner was in gets the keyboard back; a blur already gave it away.
pub fn hide(app: &AppHandle, give_back: bool) {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.hide();
    }
    if let Ok(mut s) = SHOWN.lock() {
        s.hidden_at = Some(Instant::now());
    }
    if give_back {
        crate::popup::give_focus_back(app);
    }
}

/// The icon's rectangle in placement units, from the last time the panel opened.
pub fn last_icon() -> Option<capture::Rect> {
    SHOWN.lock().ok().and_then(|s| s.icon)
}

/// Sizes the panel to its content, cut to the work area, keeping its top
/// edge (or, above a bottom taskbar, its bottom edge) where it was, and
/// inside the work area.
pub fn fit(app: &AppHandle, height: f64) -> tauri::Result<()> {
    let h = height.clamp(MIN_HEIGHT, MAX_HEIGHT);
    let (screen, side) = {
        let mut s = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
        s.height = h;
        (s.screen, s.side)
    };
    let Some(win) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    let (Some(screen), Some(current)) = (screen, placement::window_rect(&win)) else {
        return win.set_size(LogicalSize::new(WIDTH, h));
    };
    let unit = screen.unit(placement::LOGICAL);
    let rect = placement::regrow(
        Area::new(current.x, current.y, WIDTH * unit, current.h),
        side,
        h * unit,
        screen.work,
        0.0,
    );
    placement::apply(&win, rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(x: f64, y: f64, w: f64, h: f64) -> Area {
        Area { x, y, w, h }
    }

    /// A 1512 x 982 point MacBook screen with a notched, 37-point menu bar.
    fn notched_mac() -> Screen {
        Screen {
            area: area(0.0, 0.0, 1512.0, 982.0),
            work: area(0.0, 37.0, 1512.0, 945.0),
            scale: 2.0,
        }
    }

    #[test]
    fn under_a_notched_menu_bar_it_opens_below_the_bar_centred_on_the_icon() {
        let icon = area(1300.0, 0.0, 30.0, 37.0);
        let (x, y) = place(icon, 340.0, 400.0, &notched_mac(), 6.0);
        assert_eq!(y, 37.0 + 6.0);
        assert_eq!(x, 1315.0 - 170.0);
    }

    #[test]
    fn near_the_right_edge_it_stays_on_the_screen() {
        let icon = area(1480.0, 0.0, 30.0, 37.0);
        let (x, _) = place(icon, 340.0, 400.0, &notched_mac(), 6.0);
        assert_eq!(x, 1512.0 - 340.0);
    }

    #[test]
    fn above_a_bottom_taskbar_it_opens_upward() {
        let screen = Screen {
            area: area(0.0, 0.0, 1920.0, 1080.0),
            work: area(0.0, 0.0, 1920.0, 1032.0),
            scale: 1.0,
        };
        let icon = area(1700.0, 1040.0, 32.0, 40.0);
        let (x, y) = place(icon, 340.0, 400.0, &screen, 6.0);
        assert_eq!(y, 1032.0 - 6.0 - 400.0);
        assert_eq!(x, 1716.0 - 170.0);
    }

    #[test]
    fn beside_a_left_taskbar_it_opens_to_the_right() {
        let screen = Screen {
            area: area(0.0, 0.0, 1920.0, 1080.0),
            work: area(48.0, 0.0, 1872.0, 1080.0),
            scale: 1.0,
        };
        let icon = area(8.0, 1000.0, 32.0, 32.0);
        let (x, y) = place(icon, 340.0, 400.0, &screen, 6.0);
        assert_eq!(x, 48.0 + 6.0);
        assert_eq!(y, 1080.0 - 400.0);
    }

    #[test]
    fn an_icon_in_an_overflow_flyout_opens_above_it_in_the_lower_half() {
        let screen = Screen {
            area: area(0.0, 0.0, 1920.0, 1080.0),
            work: area(0.0, 0.0, 1920.0, 1032.0),
            scale: 1.0,
        };
        let icon = area(1600.0, 960.0, 32.0, 32.0);
        let (_, y) = place(icon, 340.0, 400.0, &screen, 6.0);
        assert_eq!(y, 960.0 - 6.0 - 400.0);
    }

    #[test]
    fn on_macos_each_screen_is_read_in_its_own_points() {
        // A Retina laptop at the origin and a 1x display to its right.
        let laptop = Screen {
            area: area(0.0, 0.0, 3024.0, 1964.0),
            work: area(0.0, 74.0, 3024.0, 1890.0),
            scale: 2.0,
        };
        let external = Screen {
            area: area(1512.0, 0.0, 2560.0, 1440.0),
            work: area(1512.0, 25.0, 2560.0, 1415.0),
            scale: 1.0,
        };
        // The icon on the external display: points 3900..3930, scale 1.
        let icon = area(3900.0, 0.0, 30.0, 25.0);
        let (i, screen, icon) = screen_for(&[laptop, external], icon, true).unwrap();
        assert_eq!(i, 1);
        assert_eq!(screen.work.y, 25.0);
        let (x, y) = place(icon, 340.0, 400.0, &screen, 6.0);
        assert_eq!(y, 31.0);
        assert_eq!(x, 1512.0 + 2560.0 - 340.0);

        // The icon on the laptop: points 1300..1330 times 2.
        let icon = area(2600.0, 0.0, 60.0, 74.0);
        let (i, screen, icon) = screen_for(&[laptop, external], icon, true).unwrap();
        assert_eq!(i, 0);
        assert_eq!(icon, area(1300.0, 0.0, 30.0, 37.0));
        assert_eq!(screen.work, area(0.0, 37.0, 1512.0, 945.0));
    }

    #[test]
    fn on_windows_the_desktop_is_one_pixel_space() {
        let left = Screen {
            area: area(-1920.0, 0.0, 1920.0, 1080.0),
            work: area(-1920.0, 0.0, 1920.0, 1032.0),
            scale: 1.0,
        };
        let main = Screen {
            area: area(0.0, 0.0, 2880.0, 1800.0),
            work: area(0.0, 0.0, 2880.0, 1728.0),
            scale: 1.5,
        };
        let icon = area(-300.0, 1040.0, 32.0, 40.0);
        let (i, _, unchanged) = screen_for(&[main, left], icon, false).unwrap();
        assert_eq!(i, 1);
        assert_eq!(unchanged, icon);
        assert!(screen_for(&[main, left], area(9000.0, 0.0, 1.0, 1.0), false).is_none());
    }

    #[test]
    fn a_panel_taller_than_the_work_area_is_cut_to_it_and_stays_inside() {
        // 1366 x 768 at 125 %, a bottom taskbar: 720 work pixels, and a panel
        // of 640 points (800 pixels).
        let screen = Screen {
            area: area(0.0, 0.0, 1366.0, 768.0),
            work: area(0.0, 0.0, 1366.0, 720.0),
            scale: 1.25,
        };
        let icon = area(1200.0, 728.0, 40.0, 40.0);
        let r = rect_for(&screen, icon, 640.0, false);
        assert_eq!(r.h, 720.0);
        assert!(screen.work.holds(&r));
        // Under a notched menu bar on a short screen.
        let mac = Screen {
            area: area(0.0, 0.0, 1280.0, 600.0),
            work: area(0.0, 37.0, 1280.0, 500.0),
            scale: 1.0,
        };
        let r = rect_for(&mac, area(1200.0, 0.0, 30.0, 37.0), 640.0, true);
        assert!(mac.work.holds(&r));
        assert_eq!(r.h, 500.0);
    }

    #[test]
    fn a_panel_that_grows_stays_on_its_icon_and_inside() {
        let screen = Screen {
            area: area(0.0, 0.0, 1920.0, 1080.0),
            work: area(0.0, 0.0, 1920.0, 1032.0),
            scale: 1.0,
        };
        let icon = area(1700.0, 1040.0, 32.0, 40.0);
        let r = rect_for(&screen, icon, 300.0, false);
        let grown = placement::regrow(r, Side::Above, 600.0, screen.work, 0.0);
        assert_eq!(grown.bottom(), r.bottom());
        assert!(screen.work.holds(&grown));
        let huge = placement::regrow(r, Side::Above, 5000.0, screen.work, 0.0);
        assert!(screen.work.holds(&huge));
    }

    #[test]
    fn the_click_that_closed_the_panel_does_not_open_it_again() {
        let now = Instant::now();
        assert!(!is_closing_click(None, now));
        assert!(is_closing_click(Some(now), now + Duration::from_millis(50)));
        assert!(!is_closing_click(Some(now), now + Duration::from_secs(2)));
    }
}
