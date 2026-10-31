//! The menu-bar (macOS) / tray (Windows) icon. A left click opens the panel
//! (panel.rs); a right click opens a small native menu as a fallback:
//! Open Lexpad, Open a video…, Lexpad on your phone… / Lexpad everywhere…
//! (signed in only, `everywhere`), Settings…, Quit Lexpad.
//!
//! The icons are drawn by `scripts/icons.py` from the Lexpad mark:
//! - macOS: a template image (black on transparent; the system tints it for
//!   light and dark menu bars) with a 1x and a 2x representation, so it is
//!   sharp on every display.
//! - Windows: the coloured mark from `tray.ico`, at the frame that matches
//!   the system's small-icon size for its display scaling.

use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::{everywhere, panel};

/// Where "Lexpad on your phone…" goes in the right-click menu: after Open
/// Lexpad and Open a video…, above Settings… (uiux spec §6.3).
const EVERYWHERE_AT: usize = 2;

/// The right-click menu and its "Lexpad everywhere" item, which is in the
/// menu only while someone is signed in. Its label follows `everywhere`.
struct EverywhereItem {
    menu: Menu<Wry>,
    item: MenuItem<Wry>,
    shown: Mutex<bool>,
}

/// The status item's slot in points (the size scripts/icons.py draws).
#[cfg(target_os = "macos")]
const TEMPLATE_POINTS: (f64, f64) = (16.0, 18.0);

/// macOS opens a menu-bar item on the press, Windows a tray item on the release.
const OPENS_ON: MouseButtonState = if cfg!(target_os = "macos") {
    MouseButtonState::Down
} else {
    MouseButtonState::Up
};

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Lexpad", true, None::<&str>)?;
    // The Player, straight to the file dialog (spec §2, §24.3).
    let video = MenuItem::with_id(app, "video", "Open a video…", true, None::<&str>)?;
    // Not in the menu until a session is known (`set_everywhere`).
    let phone = MenuItem::with_id(app, "everywhere", everywhere::OFFER, true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Lexpad", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &video,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    app.manage(EverywhereItem {
        menu: menu.clone(),
        item: phone,
        shown: Mutex::new(false),
    });
    let builder = TrayIconBuilder::with_id("main")
        .tooltip(if crate::config::PLAYER_TEST {
            "Lexpad Player Test"
        } else {
            "Lexpad"
        })
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::open_web(app, None),
            "everywhere" => everywhere::open(app),
            "video" => crate::player_window::open(
                app,
                crate::player_window::Opened::Pick,
                Some(crate::player_window::Door::Tray),
            ),
            "settings" => crate::open_desktop_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state,
                rect,
                ..
            } = event
            {
                if button_state == OPENS_ON {
                    panel::toggle(tray.app_handle(), rect);
                }
            }
        });

    #[cfg(target_os = "macos")]
    let builder = builder
        .icon(tauri::image::Image::from_bytes(include_bytes!(
            "../icons/tray-template@2x.png"
        ))?)
        .icon_as_template(true);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.icon(windows_icon()?);

    // The app keeps the icon for its whole life; this handle is only needed
    // on macOS, to give it its 1x and 2x images.
    let _tray = builder.build(app)?;
    #[cfg(target_os = "macos")]
    _tray.with_inner_tray_icon(|inner| {
        if let Some(item) = inner.ns_status_item() {
            set_template_image(&item);
        }
    })?;
    Ok(())
}

/// Puts "Lexpad on your phone…" / "Lexpad everywhere…" in the right-click
/// menu, with the words for `used` (`Some`, signed in), or takes it out
/// (`None`, signed out).
pub fn set_everywhere(app: &AppHandle, used: Option<bool>) {
    let Some(entry) = app.try_state::<EverywhereItem>() else {
        return;
    };
    let Ok(mut shown) = entry.shown.lock() else {
        return;
    };
    let result = match (used, *shown) {
        (Some(used), was) => entry
            .item
            .set_text(everywhere::label(used))
            .and_then(|()| {
                if was {
                    Ok(())
                } else {
                    entry.menu.insert(&entry.item, EVERYWHERE_AT)
                }
            })
            .map(|()| *shown = true),
        (None, true) => entry.menu.remove(&entry.item).map(|()| *shown = false),
        (None, false) => Ok(()),
    };
    if let Err(e) = result {
        log::warn!("could not update the tray menu's Lexpad everywhere item: {e}");
    }
}

/// The one-time hint after the last window closed (Start window spec §6):
/// on macOS the panel hangs from the menu-bar icon with "Lexpad is still
/// here"; on Windows one system message says where the icon is (Windows 11
/// tucks it under the ^ arrow).
/// Says whether it was shown (no icon to hang from, or notifications off on
/// this computer: not shown, and it stays due).
pub fn still_here(app: &AppHandle) -> bool {
    #[cfg(target_os = "macos")]
    {
        let rect = app.tray_by_id("main").and_then(|t| t.rect().ok().flatten());
        let Some(rect) = rect else {
            return false;
        };
        let handle = app.clone();
        app.run_on_main_thread(move || panel::show_hint(&handle, rect))
            .is_ok()
    }
    #[cfg(not(target_os = "macos"))]
    crate::notify::show_app_message(app, "lexpad-still-here", STILL_HERE.0, STILL_HERE.1)
}

/// The Windows toast's title and body (spec §6, mock-up 11).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub const STILL_HERE: (&str, &str) = (
    "Lexpad is still running",
    "Find it in the taskbar corner, under the ^ arrow. Click it to open Lexpad or a video again.",
);

/// Replaces the single image the tray crate makes with one that has a 1x
/// and a 2x representation, marked as a template.
#[cfg(target_os = "macos")]
fn set_template_image(item: &objc2_app_kit::NSStatusItem) {
    use objc2::{AnyThread as _, MainThreadMarker};
    use objc2_app_kit::{NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSData, NSSize};

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(button) = item.button(mtm) else {
        return;
    };
    let (w, h) = TEMPLATE_POINTS;
    let size = NSSize::new(w, h);
    let image = NSImage::initWithSize(NSImage::alloc(), size);
    for png in [
        &include_bytes!("../icons/tray-template.png")[..],
        &include_bytes!("../icons/tray-template@2x.png")[..],
    ] {
        let data = NSData::with_bytes(png);
        if let Some(rep) = NSBitmapImageRep::imageRepWithData(&data) {
            rep.setSize(size);
            image.addRepresentation(&rep);
        }
    }
    image.setTemplate(true);
    button.setImage(Some(&image));
}

/// The tray.ico frame for the system's small-icon size (16 px at 100%,
/// 20 at 125%, 24 at 150%, 32 at 200%).
#[cfg(not(target_os = "macos"))]
fn windows_icon() -> tauri::Result<tauri::image::Image<'static>> {
    let bad = |e: std::io::Error| tauri::Error::InvalidIcon(e);
    let dir = ico::IconDir::read(std::io::Cursor::new(include_bytes!("../icons/tray.ico")))
        .map_err(bad)?;
    let sizes: Vec<u32> = dir.entries().iter().map(|e| e.width()).collect();
    let want = pick_size(&sizes, small_icon_size());
    let entry = dir
        .entries()
        .iter()
        .find(|e| Some(e.width()) == want)
        .ok_or_else(|| bad(std::io::Error::other("tray.ico has no frames")))?;
    let frame = entry.decode().map_err(bad)?;
    Ok(tauri::image::Image::new_owned(
        frame.rgba_data().to_vec(),
        frame.width(),
        frame.height(),
    ))
}

#[cfg(windows)]
fn small_icon_size() -> u32 {
    use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
    use windows::Win32::UI::WindowsAndMessaging::SM_CXSMICON;
    // SAFETY: both only read system metrics.
    let px = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) };
    u32::try_from(px).unwrap_or(16).max(16)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn small_icon_size() -> u32 {
    24
}

/// The smallest frame at least `want` pixels wide, else the largest there is.
/// Scaling a frame down blurs it less than scaling one up.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn pick_size(sizes: &[u32], want: u32) -> Option<u32> {
    sizes
        .iter()
        .copied()
        .filter(|&s| s >= want)
        .min()
        .or_else(|| sizes.iter().copied().max())
}

#[cfg(test)]
mod tests {
    use super::{pick_size, STILL_HERE};

    #[test]
    fn the_windows_hint_says_what_the_spec_says() {
        assert_eq!(STILL_HERE.0, "Lexpad is still running");
        assert_eq!(
            STILL_HERE.1,
            "Find it in the taskbar corner, under the ^ arrow. Click it to open Lexpad or a video again."
        );
    }

    #[test]
    fn the_tray_uses_the_frame_for_the_display_scaling() {
        let sizes = [16, 20, 24, 32, 48];
        assert_eq!(pick_size(&sizes, 16), Some(16));
        assert_eq!(pick_size(&sizes, 20), Some(20));
        assert_eq!(pick_size(&sizes, 28), Some(32));
        assert_eq!(pick_size(&sizes, 64), Some(48));
        assert_eq!(pick_size(&[], 16), None);
    }
}
