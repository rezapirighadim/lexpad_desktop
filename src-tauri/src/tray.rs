//! The menu-bar (macOS) / tray (Windows) icon. A left click opens the panel
//! (panel.rs); a right click opens a small native menu as a fallback:
//! Open Lexpad, Settings, Quit.
//!
//! The icons are drawn by `scripts/icons.py` from the Lexpad mark:
//! - macOS: a template image (black on transparent; the system tints it for
//!   light and dark menu bars) with a 1x and a 2x representation, so it is
//!   sharp on every display.
//! - Windows: the coloured mark from `tray.ico`, at the frame that matches
//!   the system's small-icon size for its display scaling.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::panel;

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
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Lexpad", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let builder = TrayIconBuilder::with_id("main")
        .tooltip("Lexpad")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::open_web(app, None),
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
    use super::pick_size;

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
