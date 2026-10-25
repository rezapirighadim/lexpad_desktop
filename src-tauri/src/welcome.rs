//! The first launch (spec `uiux/specs/2026-10-25-video-player.md` §24.2):
//! "What would you like to do first?" — Watch, Learn or Both, one click —
//! then a soft sign-in for that path. It replaces the first run's "open
//! Settings to say hello", is shown once, and never when Lexpad was first
//! started by opening a video (that video just plays) or already has a
//! session. The answer only picks where Lexpad opens and which tips show;
//! it locks nothing. Escape or closing the window counts as Both.
//!
//! A small page of this app (`src/welcome.html`), like Settings, because it
//! must work signed out and before the web app is loaded.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::placement::{self, Area};
use crate::player_window::{self, Opened};
use crate::settings::{Focus, Place};
use crate::state::AppState;

pub const LABEL: &str = "welcome";
const TITLE: &str = "Welcome to Lexpad";
/// The window (§24.2): 880×560, not resizable, centred on the pointer's monitor.
const SIZE: (f64, f64) = (880.0, 560.0);

/// Opens the welcome, or brings it forward.
pub fn open(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        crate::refresh_dock(&handle, true);
        if let Some(win) = handle.get_webview_window(LABEL) {
            let _ = win.show();
            let _ = win.set_focus();
            return;
        }
        if let Err(e) = build(&handle) {
            log::warn!("could not open the welcome: {e}");
            // Without it, the old hello still greets a new install.
            crate::show_settings(&handle);
        }
    });
}

fn build(app: &AppHandle) -> tauri::Result<()> {
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("welcome.html".into()))
        .title(TITLE)
        .inner_size(SIZE.0, SIZE.1)
        .resizable(false)
        .maximizable(false)
        .visible(false);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(16.0, 16.0));
    let win = builder.build()?;
    let screens = placement::screens(app);
    if let Some(rect) = centred(&screens, placement::pointer(app), placement::LOGICAL) {
        let _ = placement::apply(&win, rect);
    } else {
        let _ = win.center();
    }
    win.show()?;
    win.set_focus()?;
    Ok(())
}

/// The welcome's place: centred in the work area of the monitor under the pointer.
pub fn centred(
    screens: &[placement::Screen],
    pointer: Option<(f64, f64)>,
    logical: bool,
) -> Option<Area> {
    let screen = pointer
        .and_then(|p| placement::screen_at(screens, p))
        .or_else(|| screens.first().copied())?;
    let u = screen.unit(logical);
    let (w, h) = (
        (SIZE.0 * u).min(screen.work.w),
        (SIZE.1 * u).min(screen.work.h),
    );
    let (cx, cy) = screen.work.centre();
    Some(Area::new(cx - w / 2.0, cy - h / 2.0, w, h))
}

/// The first place a choice opens (§24.2, "What the choice sets").
pub fn first_place(focus: Focus) -> Place {
    match focus {
        Focus::Watch => Place::Player,
        Focus::Learn | Focus::Both => Place::Learn,
    }
}

fn parse_focus(word: &str) -> Option<Focus> {
    match word {
        "watch" => Some(Focus::Watch),
        "learn" => Some(Focus::Learn),
        "both" => Some(Focus::Both),
        _ => None,
    }
}

/// What the welcome needs to draw: this computer and whether it is signed in.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WelcomeState {
    os: &'static str,
    signed_in: bool,
    focus: Option<Focus>,
}

#[tauri::command]
pub async fn welcome_state(app: AppHandle) -> WelcomeState {
    let state = app.state::<AppState>();
    let signed_in = state.api.user().await.is_some();
    let focus = state.settings.lock().ok().and_then(|s| s.focus);
    WelcomeState {
        os: if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(windows) {
            "windows"
        } else {
            "linux"
        },
        signed_in,
        focus,
    }
}

/// Step 1's answer: kept in settings.json (and sent to the API with the
/// other onboarding answers by the web app once signed in).
#[tauri::command]
pub fn welcome_choose(app: AppHandle, focus: String) -> Result<(), String> {
    let focus = parse_focus(&focus).ok_or("not a choice")?;
    app.state::<AppState>()
        .update_settings(|s| {
            s.focus = Some(focus);
            s.last_place = first_place(focus);
        })
        .map(|_| ())
}

/// The end of the welcome: the Player ("player") or Lexpad's window
/// ("learn"), and the welcome goes.
#[tauri::command]
pub fn welcome_finish(app: AppHandle, target: String) -> Result<(), String> {
    match target.as_str() {
        "player" => player_window::open(&app, Opened::App),
        "learn" => crate::main_window::open(&app, None),
        _ => return Err("not a place".into()),
    }
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.destroy();
    }
    Ok(())
}

/// Closing the welcome without an answer counts as Both: Lexpad's window opens.
pub fn closed_without_answer(app: &AppHandle) {
    let state = app.state::<AppState>();
    let answered = state
        .settings
        .lock()
        .map(|s| s.focus.is_some())
        .unwrap_or(true);
    if answered {
        return;
    }
    let _ = state.update_settings(|s| {
        s.focus = Some(Focus::Both);
        s.last_place = Place::Learn;
    });
    crate::main_window::open(app, None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::Screen;

    #[test]
    fn each_choice_opens_its_first_place() {
        assert_eq!(first_place(Focus::Watch), Place::Player);
        assert_eq!(first_place(Focus::Learn), Place::Learn);
        assert_eq!(first_place(Focus::Both), Place::Learn);
        assert_eq!(parse_focus("watch"), Some(Focus::Watch));
        assert_eq!(parse_focus("everything"), None);
    }

    #[test]
    fn the_welcome_is_centred_and_fits_a_small_screen() {
        let s = Screen {
            area: Area::new(0.0, 0.0, 1280.0, 800.0),
            work: Area::new(0.0, 25.0, 1280.0, 700.0),
            scale: 2.0,
        };
        let r = centred(&[s], Some((10.0, 10.0)), true).unwrap();
        assert_eq!((r.w, r.h), (880.0, 560.0));
        assert!(s.work.holds(&r));
        let tiny = Screen {
            area: Area::new(0.0, 0.0, 800.0, 500.0),
            work: Area::new(0.0, 0.0, 800.0, 500.0),
            scale: 1.0,
        };
        let r = centred(&[tiny], None, true).unwrap();
        assert_eq!((r.w, r.h), (800.0, 500.0));
    }
}
