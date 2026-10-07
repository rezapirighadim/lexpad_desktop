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

use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, WebviewUrl,
    WebviewWindowBuilder,
};

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

/// A rectangle in one coordinate space (points on macOS, pixels on Windows).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Area {
    fn right(&self) -> f64 {
        self.x + self.w
    }
    fn bottom(&self) -> f64 {
        self.y + self.h
    }
    fn centre(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
    fn scaled(&self, by: f64) -> Area {
        Area {
            x: self.x * by,
            y: self.y * by,
            w: self.w * by,
            h: self.h * by,
        }
    }
}

/// A monitor as the platform reports it: its whole area and its work area
/// (without the menu bar, Dock or taskbar) in physical pixels, and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub area: Area,
    pub work: Area,
    pub scale: f64,
}

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
    /// The icon's rectangle in points, where the panel last opened: the card
    /// for a typed word opens from the same place.
    icon: Option<capture::Rect>,
}

static SHOWN: Mutex<Shown> = Mutex::new(Shown {
    hidden_at: None,
    height: 420.0,
    icon: None,
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

fn show(app: &AppHandle, icon: tauri::Rect) {
    let Some(win) = app.get_webview_window(LABEL) else {
        return;
    };
    let pos = icon.position.to_physical::<f64>(1.0);
    let size = icon.size.to_physical::<f64>(1.0);
    let icon = Area {
        x: pos.x,
        y: pos.y,
        w: size.width,
        h: size.height,
    };
    let screens: Vec<Screen> = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| Screen {
            area: Area {
                x: m.position().x as f64,
                y: m.position().y as f64,
                w: m.size().width as f64,
                h: m.size().height as f64,
            },
            work: Area {
                x: m.work_area().position.x as f64,
                y: m.work_area().position.y as f64,
                w: m.work_area().size.width as f64,
                h: m.work_area().size.height as f64,
            },
            scale: m.scale_factor(),
        })
        .collect();
    let logical = cfg!(target_os = "macos");
    let height = SHOWN.lock().map(|s| s.height).unwrap_or(420.0);
    if let Some((_, screen, icon_units)) = screen_for(&screens, icon, logical) {
        // Points on macOS; on Windows the panel's size in that screen's pixels.
        let unit = if logical { 1.0 } else { screen.scale };
        let (x, y) = place(icon_units, WIDTH * unit, height * unit, &screen, GAP * unit);
        if logical {
            let _ = win.set_position(LogicalPosition::new(x, y));
        } else {
            let _ = win.set_position(PhysicalPosition::new(x, y));
        }
        let points = icon_units.scaled(1.0 / unit);
        if let Ok(mut s) = SHOWN.lock() {
            s.icon = Some(capture::Rect {
                x: points.x,
                y: points.y,
                w: points.w,
                h: points.h,
            });
        }
    }
    let _ = win.show();
    let _ = win.set_focus();
    let _ = win.emit("panel:open", ());
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

/// The icon's rectangle in points, from the last time the panel opened.
pub fn last_icon() -> Option<capture::Rect> {
    SHOWN.lock().ok().and_then(|s| s.icon)
}

/// Sizes the panel to its content and keeps its top edge (or, above a
/// bottom taskbar, its bottom edge) where it was.
pub fn fit(app: &AppHandle, height: f64) -> tauri::Result<()> {
    let h = height.clamp(MIN_HEIGHT, MAX_HEIGHT);
    let old = SHOWN
        .lock()
        .map(|mut s| std::mem::replace(&mut s.height, h))
        .unwrap_or(h);
    let Some(win) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    let scale = win.scale_factor()?;
    // A panel that opened upward grows upward, so it stays on the icon.
    let pos = win.outer_position()?;
    let opened_upward = last_icon().is_some_and(|icon| icon.y * scale > pos.y as f64);
    win.set_size(LogicalSize::new(WIDTH, h))?;
    if opened_upward && (old - h).abs() > f64::EPSILON {
        let y = pos.y as f64 + (old - h) * scale;
        win.set_position(PhysicalPosition::new(pos.x as f64, y))?;
    }
    Ok(())
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
    fn the_click_that_closed_the_panel_does_not_open_it_again() {
        let now = Instant::now();
        assert!(!is_closing_click(None, now));
        assert!(is_closing_click(Some(now), now + Duration::from_millis(50)));
        assert!(!is_closing_click(Some(now), now + Duration::from_secs(2)));
    }
}
