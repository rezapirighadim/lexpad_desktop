//! The placement smoke test CI runs on macOS and Windows. Compiled only with
//! `--features smoke-test`, which no shipped build has.
//!
//! `lexpad-desktop --smoke-test` starts the app's real windows and tray, but
//! with no saved session (nothing is read from or written to the credential
//! store, nothing goes over the network), no shortcut, no login item and its
//! settings in a temporary folder. It opens the popup at points all over
//! every monitor (corners, edges, the menu bar or taskbar itself, the
//! middle), makes it grow past the work area as a card with meanings does,
//! opens the panel from an icon on each monitor, and prints every final
//! rectangle. It exits 0 when every one is inside its monitor's work area,
//! and 1 otherwise or when there is no monitor at all.

use std::sync::mpsc;
use std::thread::sleep;
use std::time::Duration;

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};

use crate::placement::{self, Area, Screen};
use crate::store::{Saved, SessionStore};
use crate::{panel, popup};

/// How long a window is given to settle after a move, a resize or a show:
/// generous, as CI machines are slow and the result is read back from the
/// window system.
const SETTLE: Duration = Duration::from_millis(400);

pub fn requested() -> bool {
    std::env::args().any(|a| a == "--smoke-test")
}

/// A session store that holds nothing: the smoke test is always signed out.
pub struct Nothing;

impl SessionStore for Nothing {
    fn load(&self) -> Option<Saved> {
        None
    }
    fn save(&self, _saved: &Saved) -> Result<(), String> {
        Err("smoke test".into())
    }
    fn clear(&self) {}
}

/// Runs the checks off the main thread, then exits with their verdict.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let code = run(&app);
        println!("SMOKE exit {code}");
        app.exit(code);
    });
}

fn on_main<R: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(&AppHandle) -> R + Send + 'static,
) -> Option<R> {
    let (tx, rx) = mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(f(&handle));
    })
    .ok()?;
    rx.recv_timeout(Duration::from_secs(20)).ok()
}

fn fmt(a: &Area) -> String {
    format!("{:.0},{:.0} {:.0}x{:.0}", a.x, a.y, a.w, a.h)
}

/// Points all over a screen: its corners, the middle of each edge, the
/// middle, and the corners of its work area.
fn points(s: &Screen) -> Vec<(f64, f64)> {
    let (a, w) = (s.area, s.work);
    let (cx, cy) = a.centre();
    vec![
        (a.x + 1.0, a.y + 1.0),
        (a.right() - 1.0, a.y + 1.0),
        (a.x + 1.0, a.bottom() - 1.0),
        (a.right() - 1.0, a.bottom() - 1.0),
        (cx, a.y + 1.0),
        (cx, a.bottom() - 1.0),
        (a.x + 1.0, cy),
        (a.right() - 1.0, cy),
        (cx, cy),
        (w.x + 1.0, w.bottom() - 1.0),
        (w.right() - 1.0, w.bottom() - 1.0),
    ]
}

/// An icon where the platform puts it: in the menu bar on macOS, in the
/// taskbar on Windows (or, with no taskbar on this monitor, in the corner of
/// its work area, as in the overflow flyout).
fn icon_on(s: &Screen) -> Area {
    let (a, w) = (s.area, s.work);
    let u = s.unit(placement::LOGICAL);
    if placement::LOGICAL {
        Area::new(a.right() - 140.0, a.y, 30.0, (w.y - a.y).max(24.0))
    } else if w.bottom() < a.bottom() {
        Area::new(
            a.right() - 200.0 * u,
            w.bottom(),
            32.0 * u,
            a.bottom() - w.bottom(),
        )
    } else if w.y > a.y {
        Area::new(a.right() - 200.0 * u, a.y, 32.0 * u, w.y - a.y)
    } else {
        Area::new(
            w.right() - 60.0 * u,
            w.bottom() - 60.0 * u,
            32.0 * u,
            32.0 * u,
        )
    }
}

fn check(label: &str, s: &Screen, rect: Option<Area>, failures: &mut u32) {
    match rect {
        Some(r) if s.work.holds(&r) => println!("SMOKE ok   {label} -> {}", fmt(&r)),
        Some(r) => {
            *failures += 1;
            println!(
                "SMOKE FAIL {label} -> {} not inside work {}",
                fmt(&r),
                fmt(&s.work)
            );
        }
        None => {
            *failures += 1;
            println!("SMOKE FAIL {label} -> the window could not be read");
        }
    }
}

fn run(app: &AppHandle) -> i32 {
    // Let the windows and the tray finish starting.
    sleep(Duration::from_secs(3));
    let screens = on_main(app, placement::screens).unwrap_or_default();
    println!("SMOKE monitors {}", screens.len());
    for (i, s) in screens.iter().enumerate() {
        println!(
            "SMOKE monitor {i}: area {} work {} scale {}",
            fmt(&s.area),
            fmt(&s.work),
            s.scale
        );
    }
    if screens.is_empty() {
        println!("SMOKE FAIL no monitor");
        return 1;
    }
    let mut failures = 0;
    for (i, s) in screens.iter().enumerate() {
        for (x, y) in points(s) {
            for height in [popup::MIN_HEIGHT, 420.0, 5000.0] {
                on_main(app, move |a| {
                    if let Some(win) = a.get_webview_window(popup::LABEL) {
                        popup::place_at(a, &win, Area::new(x, y, 0.0, 0.0), true);
                        let _ = win.show();
                    }
                });
                sleep(SETTLE);
                on_main(app, move |a| popup::fit(a, height));
                sleep(SETTLE);
                let rect = on_main(app, |a| {
                    a.get_webview_window(popup::LABEL)
                        .and_then(|w| placement::window_rect(&w))
                })
                .flatten();
                check(
                    &format!("popup monitor {i} pointer {x:.0},{y:.0} grown to {height}"),
                    s,
                    rect,
                    &mut failures,
                );
            }
        }
        on_main(app, |a| {
            if let Some(w) = a.get_webview_window(popup::LABEL) {
                let _ = w.hide();
            }
        });

        // The panel, from an icon on this monitor, as Tauri reports an icon.
        let icon = icon_on(s);
        let raw = icon.scaled(if placement::LOGICAL { s.scale } else { 1.0 });
        let rect = tauri::Rect {
            position: PhysicalPosition::new(raw.x, raw.y).into(),
            size: PhysicalSize::new(raw.w, raw.h).into(),
        };
        for height in [420.0, panel::MAX_HEIGHT] {
            on_main(app, move |a| panel::show(a, rect));
            sleep(SETTLE);
            on_main(app, move |a| panel::fit(a, height));
            sleep(SETTLE);
            let got = on_main(app, |a| {
                a.get_webview_window(panel::LABEL)
                    .and_then(|w| placement::window_rect(&w))
            })
            .flatten();
            check(
                &format!("panel monitor {i} icon {} height {height}", fmt(&icon)),
                s,
                got,
                &mut failures,
            );
            on_main(app, |a| panel::hide(a, false));
            // Past the guard that ignores the click which closed the panel.
            sleep(SETTLE);
        }
    }
    println!("SMOKE failures {failures}");
    i32::from(failures > 0)
}
