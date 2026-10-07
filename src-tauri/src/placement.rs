//! Where a window goes on screen: the arithmetic, apart from any window, so
//! it is tested with every arrangement of screens we can think of.
//!
//! Every rectangle here is in one unit, the one the platform positions
//! windows in:
//! - **macOS**: global points, top-left origin. Tauri reports a monitor as
//!   points times *that monitor's* scale and the pointer as points times the
//!   *main* screen's scale, so each is divided by its own scale first
//!   ([`screen_in_units`], [`pointer_in_units`]); windows are then moved with
//!   logical positions and sizes.
//! - **Windows**: physical pixels of the virtual desktop (Tauri makes the
//!   process per-monitor DPI aware), which may be negative left of or above
//!   the main monitor; windows are moved with physical positions, and a
//!   window's size in points is multiplied by its monitor's scale.
//!
//! The work area is the monitor without the menu bar and the Dock (macOS:
//! `NSScreen.visibleFrame`, which follows the Dock on any edge and leaves
//! only its thin trigger strip when it hides itself) or without the taskbar
//! on any edge (Windows: `GetMonitorInfoW`'s `rcWork`). Tauri's
//! `Monitor::work_area` reads exactly those two (tauri-runtime-wry's
//! `monitor/macos.rs` and `monitor/windows.rs`).

/// A rectangle in placement units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Area {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn centre(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    pub fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
    /// Whether `inner` lies wholly inside, allowing for rounding to whole pixels.
    #[cfg_attr(not(any(test, feature = "smoke-test")), allow(dead_code))]
    pub fn holds(&self, inner: &Area) -> bool {
        const SLACK: f64 = 1.0;
        inner.x >= self.x - SLACK
            && inner.y >= self.y - SLACK
            && inner.right() <= self.right() + SLACK
            && inner.bottom() <= self.bottom() + SLACK
    }
    pub fn scaled(&self, by: f64) -> Area {
        Area::new(self.x * by, self.y * by, self.w * by, self.h * by)
    }
    /// The area less `by` on every side, or unchanged when it is too small
    /// to lose that much.
    pub fn inset(&self, by: f64) -> Area {
        if self.w <= 2.0 * by || self.h <= 2.0 * by {
            *self
        } else {
            Area::new(
                self.x + by,
                self.y + by,
                self.w - 2.0 * by,
                self.h - 2.0 * by,
            )
        }
    }
    /// How far a point is from the area: 0 inside.
    fn distance(&self, (x, y): (f64, f64)) -> f64 {
        let dx = (self.x - x).max(x - self.right()).max(0.0);
        let dy = (self.y - y).max(y - self.bottom()).max(0.0);
        (dx * dx + dy * dy).sqrt()
    }
}

/// A monitor: its whole area, its work area, and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub area: Area,
    pub work: Area,
    pub scale: f64,
}

impl Screen {
    /// How many placement units one point is on this screen: 1 on macOS,
    /// the scale on Windows.
    pub fn unit(&self, logical: bool) -> f64 {
        if logical {
            1.0
        } else {
            self.scale
        }
    }
}

/// Whether placement works in points (macOS) or physical pixels (Windows).
pub const LOGICAL: bool = cfg!(target_os = "macos");

/// A monitor as Tauri reports it, in placement units.
pub fn screen_in_units(raw: Screen, logical: bool) -> Screen {
    let by = if logical { 1.0 / raw.scale } else { 1.0 };
    Screen {
        area: raw.area.scaled(by),
        work: raw.work.scaled(by),
        scale: raw.scale,
    }
}

/// The pointer as Tauri reports it, in placement units. On macOS that is
/// points times the main screen's scale.
pub fn pointer_in_units((x, y): (f64, f64), main_scale: f64, logical: bool) -> (f64, f64) {
    if logical && main_scale > 0.0 {
        (x / main_scale, y / main_scale)
    } else {
        (x, y)
    }
}

/// The screen a point is on; a point on no screen (a monitor just
/// unplugged, a rounding gap between two) belongs to the nearest one.
pub fn screen_at(screens: &[Screen], point: (f64, f64)) -> Option<Screen> {
    screens
        .iter()
        .find(|s| s.area.contains(point))
        .or_else(|| {
            screens
                .iter()
                .min_by(|a, b| a.area.distance(point).total_cmp(&b.area.distance(point)))
        })
        .copied()
}

/// Which side of its anchor a window opened on. A window that grows keeps
/// that edge where it is: one below keeps its top, one above its bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Below,
    Above,
}

/// A window's rectangle and the side it opened on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub rect: Area,
    pub side: Side,
}

/// Where a `w` x `h` window goes next to `anchor` inside `work`, `margin`
/// clear of its edges.
///
/// With `beside` (the anchor is the pointer) it prefers below and to the
/// right of it, `gap` away, and goes left of it when there is no room on the
/// right. Without (the anchor is a selection or an icon) it lines up with the
/// anchor's left edge, below it. Either way it goes above when there is no
/// room below; when there is room on neither side it goes on the roomier one
/// and slides until it fits. A window taller than the work area is cut to
/// it (its page scrolls), so the whole window is always inside.
pub fn near(
    anchor: Area,
    w: f64,
    h: f64,
    work: Area,
    gap: f64,
    margin: f64,
    beside: bool,
) -> Placed {
    let inner = work.inset(margin);
    let w = w.min(inner.w);
    let h = h.min(inner.h);

    let x = if beside {
        let right = anchor.right() + gap;
        if right + w <= inner.right() {
            right
        } else {
            anchor.x - gap - w
        }
    } else {
        anchor.x
    };

    let below = anchor.bottom() + gap;
    let above = anchor.y - gap - h;
    let (y, side) = if below + h <= inner.bottom() {
        (below, Side::Below)
    } else if above >= inner.y {
        (above, Side::Above)
    } else if inner.bottom() - below >= anchor.y - gap - inner.y {
        (below, Side::Below)
    } else {
        (above, Side::Above)
    };
    Placed {
        rect: clamp(Area::new(x, y, w, h), work, margin),
        side,
    }
}

/// The window at `current` now wants to be `h` tall: it keeps the edge on
/// its anchor's side (its top if it opened below, its bottom if above),
/// is cut to the work area, and slides back inside if it would leave it.
/// A window the learner dragged keeps its top (`Side::Below`).
pub fn regrow(current: Area, side: Side, h: f64, work: Area, margin: f64) -> Area {
    let y = match side {
        Side::Below => current.y,
        Side::Above => current.bottom() - h.min(work.inset(margin).h),
    };
    clamp(Area::new(current.x, y, current.w, h), work, margin)
}

/// The rectangle cut to fit `work` less `margin`, then slid inside it.
pub fn clamp(rect: Area, work: Area, margin: f64) -> Area {
    let inner = work.inset(margin);
    let w = rect.w.min(inner.w);
    let h = rect.h.min(inner.h);
    let x = rect.x.min(inner.right() - w).max(inner.x);
    let y = rect.y.min(inner.bottom() - h).max(inner.y);
    Area::new(x, y, w, h)
}

/* ------------------------------------------------- the live screens */

use tauri::{
    AppHandle, LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize, WebviewWindow,
};

/// Every monitor, in placement units.
pub fn screens(app: &AppHandle) -> Vec<Screen> {
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let work = m.work_area();
            screen_in_units(
                Screen {
                    area: Area::new(
                        m.position().x as f64,
                        m.position().y as f64,
                        m.size().width as f64,
                        m.size().height as f64,
                    ),
                    work: Area::new(
                        work.position.x as f64,
                        work.position.y as f64,
                        work.size.width as f64,
                        work.size.height as f64,
                    ),
                    scale: m.scale_factor(),
                },
                LOGICAL,
            )
        })
        .collect()
}

/// Where the pointer is, in placement units.
pub fn pointer(app: &AppHandle) -> Option<(f64, f64)> {
    let p = app.cursor_position().ok()?;
    let main_scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    Some(pointer_in_units((p.x, p.y), main_scale, LOGICAL))
}

/// The window's rectangle on screen, in placement units.
pub fn window_rect(win: &WebviewWindow) -> Option<Area> {
    let pos = win.outer_position().ok()?;
    let size = win.outer_size().ok()?;
    let by = if LOGICAL {
        1.0 / win.scale_factor().ok()?
    } else {
        1.0
    };
    Some(Area::new(
        pos.x as f64,
        pos.y as f64,
        size.width as f64,
        size.height as f64,
    ))
    .map(|a| a.scaled(by))
}

/// Moves and sizes the window to `rect` (placement units). On Windows the
/// window is moved first: arriving on a monitor with another scale makes
/// the system resize it, and the size set after that is the one that holds.
pub fn apply(win: &WebviewWindow, rect: Area) -> tauri::Result<()> {
    if LOGICAL {
        win.set_size(LogicalSize::new(rect.w, rect.h))?;
        win.set_position(LogicalPosition::new(rect.x, rect.y))?;
    } else {
        win.set_position(PhysicalPosition::new(rect.x.round(), rect.y.round()))?;
        win.set_size(PhysicalSize::new(rect.w.round(), rect.h.round()))?;
        win.set_position(PhysicalPosition::new(rect.x.round(), rect.y.round()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAP: f64 = 12.0;
    const MARGIN: f64 = 8.0;

    fn inside(work: Area, r: Area) {
        assert!(
            work.inset(MARGIN).holds(&r),
            "{r:?} is not inside {work:?} less the margin"
        );
    }

    /// A 1512 x 982 point MacBook with a 37-point notched menu bar and the
    /// Dock at the bottom (70 points).
    const MAC: Area = Area::new(0.0, 0.0, 1512.0, 982.0);
    const MAC_WORK_DOCK_BOTTOM: Area = Area::new(0.0, 37.0, 1512.0, 875.0);
    const MAC_WORK_DOCK_LEFT: Area = Area::new(70.0, 37.0, 1442.0, 945.0);
    const MAC_WORK_DOCK_RIGHT: Area = Area::new(0.0, 37.0, 1442.0, 945.0);
    /// The Dock hidden: only a 4-point strip is kept clear at the bottom.
    const MAC_WORK_DOCK_HIDDEN: Area = Area::new(0.0, 37.0, 1512.0, 941.0);

    fn point(x: f64, y: f64) -> Area {
        Area::new(x, y, 0.0, 0.0)
    }

    #[test]
    fn the_reported_case_a_pointer_near_the_dock_keeps_the_card_above_it() {
        // The pointer low on the screen, just above the Dock; the card is
        // 360 x 420. It must open above the pointer and end above the Dock.
        let p = near(
            point(700.0, 880.0),
            360.0,
            420.0,
            MAC_WORK_DOCK_BOTTOM,
            GAP,
            MARGIN,
            true,
        );
        assert_eq!(p.side, Side::Above);
        inside(MAC_WORK_DOCK_BOTTOM, p.rect);
        assert_eq!(p.rect.bottom(), 880.0 - GAP);
        assert_eq!(p.rect.x, 700.0 + GAP);
    }

    #[test]
    fn below_and_right_of_the_pointer_when_there_is_room() {
        let p = near(
            point(300.0, 200.0),
            360.0,
            420.0,
            MAC_WORK_DOCK_BOTTOM,
            GAP,
            MARGIN,
            true,
        );
        assert_eq!(p.side, Side::Below);
        assert_eq!((p.rect.x, p.rect.y), (312.0, 212.0));
    }

    #[test]
    fn left_of_the_pointer_near_the_right_edge() {
        let p = near(
            point(1400.0, 200.0),
            360.0,
            420.0,
            MAC_WORK_DOCK_BOTTOM,
            GAP,
            MARGIN,
            true,
        );
        assert_eq!(p.rect.x, 1400.0 - GAP - 360.0);
        inside(MAC_WORK_DOCK_BOTTOM, p.rect);
    }

    #[test]
    fn a_dock_on_the_left_or_right_is_never_covered() {
        for work in [MAC_WORK_DOCK_LEFT, MAC_WORK_DOCK_RIGHT] {
            for (x, y) in [(5.0, 500.0), (1505.0, 500.0), (40.0, 960.0), (1490.0, 40.0)] {
                let p = near(point(x, y), 360.0, 420.0, work, GAP, MARGIN, true);
                inside(work, p.rect);
            }
        }
    }

    #[test]
    fn with_the_dock_hidden_the_card_still_stays_clear_of_its_strip() {
        let p = near(
            point(700.0, 975.0),
            360.0,
            420.0,
            MAC_WORK_DOCK_HIDDEN,
            GAP,
            MARGIN,
            true,
        );
        inside(MAC_WORK_DOCK_HIDDEN, p.rect);
        assert_eq!(p.side, Side::Above);
    }

    #[test]
    fn a_pointer_on_the_menu_bar_opens_the_card_below_the_bar() {
        let p = near(
            point(700.0, 10.0),
            360.0,
            420.0,
            MAC_WORK_DOCK_BOTTOM,
            GAP,
            MARGIN,
            true,
        );
        inside(MAC_WORK_DOCK_BOTTOM, p.rect);
        assert!(p.rect.y >= 37.0 + MARGIN);
    }

    #[test]
    fn a_card_taller_than_the_work_area_is_cut_to_it() {
        // A short screen: 1280 x 720 at 125 % on Windows is 1024 x 576 points;
        // the work area in pixels with a 48-pixel taskbar.
        let work = Area::new(0.0, 0.0, 1280.0, 672.0);
        let p = near(point(600.0, 300.0), 450.0, 775.0, work, 15.0, 10.0, true);
        assert_eq!(p.rect.h, 672.0 - 20.0);
        inside_with(work, p.rect, 10.0);
    }

    fn inside_with(work: Area, r: Area, margin: f64) {
        assert!(work.inset(margin).holds(&r), "{r:?} not inside {work:?}");
    }

    #[test]
    fn a_selection_anchor_lines_up_with_its_left_edge() {
        let sel = Area::new(400.0, 300.0, 80.0, 18.0);
        let p = near(sel, 360.0, 300.0, MAC_WORK_DOCK_BOTTOM, GAP, MARGIN, false);
        assert_eq!((p.rect.x, p.rect.y), (400.0, 318.0 + GAP));
        // A selection near the bottom: above it, clear of it.
        let sel = Area::new(400.0, 850.0, 80.0, 18.0);
        let p = near(sel, 360.0, 300.0, MAC_WORK_DOCK_BOTTOM, GAP, MARGIN, false);
        assert_eq!(p.side, Side::Above);
        assert_eq!(p.rect.bottom(), 850.0 - GAP);
    }

    #[test]
    fn no_room_on_either_side_takes_the_roomier_one_and_slides_inside() {
        let work = Area::new(0.0, 0.0, 800.0, 600.0);
        let p = near(point(400.0, 250.0), 360.0, 500.0, work, GAP, MARGIN, true);
        assert_eq!(p.side, Side::Below);
        inside(work, p.rect);
        let p = near(point(400.0, 350.0), 360.0, 500.0, work, GAP, MARGIN, true);
        assert_eq!(p.side, Side::Above);
        inside(work, p.rect);
    }

    #[test]
    fn windows_taskbar_on_each_edge() {
        let full = Area::new(0.0, 0.0, 1920.0, 1080.0);
        let works = [
            Area::new(0.0, 0.0, 1920.0, 1032.0),  // bottom
            Area::new(0.0, 48.0, 1920.0, 1032.0), // top
            Area::new(62.0, 0.0, 1858.0, 1080.0), // left
            Area::new(0.0, 0.0, 1858.0, 1080.0),  // right
        ];
        for work in works {
            for (x, y) in [
                (0.0, 0.0),
                (1919.0, 0.0),
                (0.0, 1079.0),
                (1919.0, 1079.0),
                (960.0, 540.0),
                (960.0, 1060.0),
                (30.0, 540.0),
            ] {
                assert!(full.contains((x, y)));
                let p = near(point(x, y), 540.0, 630.0, work, 18.0, 12.0, true);
                inside_with(work, p.rect, 12.0);
            }
        }
    }

    #[test]
    fn scaled_windows_monitors_at_125_and_150_percent() {
        // 2560 x 1440 at 150 %: the card is 360 x 420 points, 540 x 630 pixels.
        let work = Area::new(0.0, 0.0, 2560.0, 1368.0);
        let s = 1.5;
        let p = near(
            point(2500.0, 1350.0),
            360.0 * s,
            420.0 * s,
            work,
            GAP * s,
            MARGIN * s,
            true,
        );
        inside_with(work, p.rect, MARGIN * s);
        assert_eq!(p.side, Side::Above);
        // 1920 x 1080 at 125 % with a top taskbar.
        let work = Area::new(0.0, 60.0, 1920.0, 1020.0);
        let s = 1.25;
        let p = near(
            point(10.0, 30.0),
            360.0 * s,
            620.0 * s,
            work,
            GAP * s,
            MARGIN * s,
            true,
        );
        inside_with(work, p.rect, MARGIN * s);
    }

    #[test]
    fn two_monitors_one_left_of_and_above_the_main_one() {
        // Windows: the main 1920 x 1080 monitor at 100 %, a 2880 x 1800 one at
        // 150 % to its left and higher up: negative coordinates.
        let main = Screen {
            area: Area::new(0.0, 0.0, 1920.0, 1080.0),
            work: Area::new(0.0, 0.0, 1920.0, 1032.0),
            scale: 1.0,
        };
        let left = Screen {
            area: Area::new(-2880.0, -400.0, 2880.0, 1800.0),
            work: Area::new(-2880.0, -400.0, 2880.0, 1728.0),
            scale: 1.5,
        };
        let screens = [main, left];
        let pointer = (-10.0, 1300.0);
        let s = screen_at(&screens, pointer).unwrap();
        assert_eq!(s, left);
        let u = s.unit(false);
        let p = near(
            point(pointer.0, pointer.1),
            360.0 * u,
            420.0 * u,
            s.work,
            GAP * u,
            MARGIN * u,
            true,
        );
        inside_with(left.work, p.rect, MARGIN * u);
        // On the main monitor, right at its corner.
        let s = screen_at(&screens, (1919.0, 1079.0)).unwrap();
        assert_eq!(s, main);
        let p = near(
            point(1919.0, 1079.0),
            360.0,
            420.0,
            s.work,
            GAP,
            MARGIN,
            true,
        );
        inside(main.work, p.rect);
    }

    #[test]
    fn a_point_on_no_screen_goes_to_the_nearest() {
        let a = Screen {
            area: Area::new(0.0, 0.0, 1000.0, 800.0),
            work: Area::new(0.0, 0.0, 1000.0, 760.0),
            scale: 1.0,
        };
        let b = Screen {
            area: Area::new(1000.0, 0.0, 1000.0, 800.0),
            work: Area::new(1000.0, 0.0, 1000.0, 760.0),
            scale: 1.0,
        };
        assert_eq!(screen_at(&[a, b], (2100.0, 100.0)), Some(b));
        assert_eq!(screen_at(&[a, b], (-50.0, 900.0)), Some(a));
        assert_eq!(screen_at(&[], (0.0, 0.0)), None);
    }

    #[test]
    fn macos_monitors_and_pointer_are_brought_into_points() {
        // A Retina laptop (scale 2) as main screen, a 1x display to its right.
        let laptop = screen_in_units(
            Screen {
                area: Area::new(0.0, 0.0, 3024.0, 1964.0),
                work: Area::new(0.0, 74.0, 3024.0, 1750.0),
                scale: 2.0,
            },
            true,
        );
        let external = screen_in_units(
            Screen {
                area: Area::new(1512.0, 0.0, 2560.0, 1440.0),
                work: Area::new(1512.0, 25.0, 2560.0, 1415.0),
                scale: 1.0,
            },
            true,
        );
        assert_eq!(laptop.work, Area::new(0.0, 37.0, 1512.0, 875.0));
        // The pointer at (3000, 1400) points on the external screen arrives
        // as points times the laptop's (main) scale.
        let p = pointer_in_units((6000.0, 2800.0), 2.0, true);
        assert_eq!(p, (3000.0, 1400.0));
        let s = screen_at(&[laptop, external], p).unwrap();
        assert_eq!(s, external);
        let placed = near(point(p.0, p.1), 360.0, 420.0, s.work, GAP, MARGIN, true);
        inside(external.work, placed.rect);
        // Windows leaves both untouched.
        assert_eq!(
            pointer_in_units((6000.0, 2800.0), 2.0, false),
            (6000.0, 2800.0)
        );
    }

    #[test]
    fn a_card_that_grows_after_its_meanings_load_stays_inside() {
        // Opened small below a pointer low on the screen, then the meanings
        // arrive and it grows to 600: it slides up instead of under the Dock.
        let p = near(
            point(700.0, 600.0),
            360.0,
            120.0,
            MAC_WORK_DOCK_BOTTOM,
            GAP,
            MARGIN,
            true,
        );
        assert_eq!(p.side, Side::Below);
        let grown = regrow(p.rect, p.side, 600.0, MAC_WORK_DOCK_BOTTOM, MARGIN);
        inside(MAC_WORK_DOCK_BOTTOM, grown);
        assert_eq!(grown.h, 600.0);
        assert_eq!(grown.bottom(), MAC_WORK_DOCK_BOTTOM.bottom() - MARGIN);

        // Opened above: it grows upward, keeping its bottom on the pointer.
        let p = near(
            point(700.0, 880.0),
            360.0,
            120.0,
            MAC_WORK_DOCK_BOTTOM,
            GAP,
            MARGIN,
            true,
        );
        assert_eq!(p.side, Side::Above);
        let grown = regrow(p.rect, p.side, 400.0, MAC_WORK_DOCK_BOTTOM, MARGIN);
        assert_eq!(grown.bottom(), p.rect.bottom());
        inside(MAC_WORK_DOCK_BOTTOM, grown);

        // Growing taller than the whole work area: cut to it.
        let grown = regrow(p.rect, p.side, 5000.0, MAC_WORK_DOCK_BOTTOM, MARGIN);
        assert_eq!(grown.h, MAC_WORK_DOCK_BOTTOM.h - 2.0 * MARGIN);
        inside(MAC_WORK_DOCK_BOTTOM, grown);
    }

    #[test]
    fn a_window_dragged_half_off_the_screen_is_brought_back_when_it_grows() {
        let dragged = Area::new(1400.0, 800.0, 360.0, 200.0);
        let r = regrow(dragged, Side::Below, 300.0, MAC_WORK_DOCK_BOTTOM, MARGIN);
        inside(MAC_WORK_DOCK_BOTTOM, r);
    }

    #[test]
    fn clamp_brings_a_remembered_window_back_onto_a_smaller_screen() {
        let remembered = Area::new(2500.0, 1500.0, 1200.0, 900.0);
        let r = clamp(remembered, MAC_WORK_DOCK_BOTTOM, MARGIN);
        inside(MAC_WORK_DOCK_BOTTOM, r);
        assert_eq!(r.w, 1200.0);
        let huge = Area::new(-100.0, -100.0, 4000.0, 3000.0);
        let r = clamp(huge, MAC_WORK_DOCK_BOTTOM, MARGIN);
        assert_eq!(r, MAC_WORK_DOCK_BOTTOM.inset(MARGIN));
    }

    #[test]
    fn every_pointer_on_every_test_screen_gives_a_card_inside_the_work_area() {
        let cases = [
            (MAC, MAC_WORK_DOCK_BOTTOM, 1.0),
            (MAC, MAC_WORK_DOCK_LEFT, 1.0),
            (MAC, MAC_WORK_DOCK_RIGHT, 1.0),
            (MAC, MAC_WORK_DOCK_HIDDEN, 1.0),
            (
                Area::new(0.0, 0.0, 1280.0, 720.0),
                Area::new(0.0, 0.0, 1280.0, 672.0),
                1.25,
            ),
            (
                Area::new(-1920.0, -1080.0, 1920.0, 1080.0),
                Area::new(-1920.0, -1032.0, 1920.0, 1032.0),
                1.5,
            ),
        ];
        for (full, work, s) in cases {
            let mut y = full.y;
            while y < full.bottom() {
                let mut x = full.x;
                while x < full.right() {
                    for h in [120.0, 420.0, 620.0, 2000.0] {
                        let p = near(
                            point(x, y),
                            360.0 * s,
                            h * s,
                            work,
                            GAP * s,
                            MARGIN * s,
                            true,
                        );
                        inside_with(work, p.rect, MARGIN * s);
                        let g = regrow(p.rect, p.side, 900.0 * s, work, MARGIN * s);
                        inside_with(work, g, MARGIN * s);
                    }
                    x += full.w / 23.0;
                }
                y += full.h / 17.0;
            }
        }
    }
}
