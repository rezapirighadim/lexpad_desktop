//! The Start window (spec `uiux/specs/2026-10-26-desktop-first-run.md`,
//! direction A of round 2 with round 3's film frame): what Lexpad does in one
//! real picture, and the two things to do now — **Open a video…** (no
//! account) and **Continue with Google / Apple** (the notebook and
//! practice). No question: what the learner does sets `focus` (§3).
//!
//! When Lexpad opens which window (§2, the launch table A–G) is decided here
//! by `first_window`, a pure function with a test per row. A video that
//! opened Lexpad just plays: the decision waits `FIRST_WINDOW_GRACE` after
//! the app is ready (macOS hands launch documents over as `RunEvent::Opened`
//! after `setup`), and a video arriving while the Start window is up closes
//! it (`crate::open_media`).
//!
//! A small page of this app (`src/welcome.html`), like Settings, because it
//! must work signed out and before the web app is loaded. Its files keep the
//! old name, "welcome".

use std::path::PathBuf;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt as _;

use crate::media::files::{self, Kind};
use crate::placement::{self, Area};
use crate::settings::{Focus, OpenAt, Place};
use crate::state::AppState;

pub const LABEL: &str = "welcome";
const TITLE: &str = "Lexpad";
/// The window (round 3): 1060×620, not resizable, centred on the pointer's
/// monitor, cut to the work area of a small screen (1280×720 keeps it whole).
const SIZE: (f64, f64) = (1060.0, 620.0);
/// How long after the app is ready the first window waits for a video the
/// system is still handing over. macOS delivers launch documents within
/// about 200 ms; 600 ms keeps a margin without a visible wait (§2).
pub const FIRST_WINDOW_GRACE: Duration = Duration::from_millis(600);
/// The event the page hears when a drop is refused, with the reason.
pub const REFUSED_EVENT: &str = "start:refused";

/// What a launch knows when it decides which window to open (§2).
#[derive(Clone, Copy, Debug, Default)]
pub struct Launch {
    /// Started by the system at login (`--login`, the autostart's argument).
    pub at_login: bool,
    /// `--player`: the Start menu's "Lexpad Player", a pinned Player button.
    pub player_flag: bool,
    /// A video (or subtitles) opened Lexpad; the Player already has it.
    pub opened_file: bool,
    pub signed_in: bool,
    /// A video was ever opened on this computer.
    pub played_video: bool,
    /// Settings: open Lexpad's window when the app starts.
    pub open_on_launch: bool,
    pub open_at: OpenAt,
    pub last_place: Place,
}

/// The window a launch opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstWindow {
    /// Nothing: the menu-bar / tray icon only.
    Nothing,
    Start,
    Player,
    Learn,
}

/// The launch table (§2):
/// B a video opened Lexpad → the Player has it, nothing more;
/// G `--player` → the Player;
/// F at login → nothing (or Lexpad's window when Settings asks for it);
/// C signed in → where it was left, or "When I open Lexpad";
/// E signed out, a video was played before → the Player;
/// A / D signed out otherwise → the Start window. Never Settings.
pub fn first_window(l: Launch) -> FirstWindow {
    if l.opened_file {
        return FirstWindow::Nothing;
    }
    if l.player_flag {
        return FirstWindow::Player;
    }
    if l.at_login {
        return if l.open_on_launch && l.signed_in {
            FirstWindow::Learn
        } else {
            FirstWindow::Nothing
        };
    }
    if l.signed_in {
        return match crate::app_menu::launch_place(l.open_at, l.last_place) {
            Place::Player => FirstWindow::Player,
            Place::Learn => FirstWindow::Learn,
        };
    }
    if l.played_video {
        FirstWindow::Player
    } else {
        FirstWindow::Start
    }
}

/// `focus` after a video was opened (§3): watch, or both after learn.
pub fn focus_after_video(now: Option<Focus>) -> Focus {
    match now {
        None | Some(Focus::Watch) => Focus::Watch,
        Some(Focus::Learn) | Some(Focus::Both) => Focus::Both,
    }
}

/// `focus` after signing in from the Start window (§3): learn, or both after watch.
pub fn focus_after_sign_in(now: Option<Focus>) -> Focus {
    match now {
        None | Some(Focus::Learn) => Focus::Learn,
        Some(Focus::Watch) | Some(Focus::Both) => Focus::Both,
    }
}

/// What a set of dropped or picked files is, for the Start window (§5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dropped {
    /// At least one video: it plays (subtitles beside it come too).
    Video,
    /// Subtitles alone: a subtitle needs its video.
    SubtitlesOnly,
    /// Nothing Lexpad plays.
    NotVideo,
}

pub fn classify(paths: &[PathBuf]) -> Dropped {
    let kinds: Vec<Kind> = paths.iter().map(|p| files::kind_of(p)).collect();
    if kinds.contains(&Kind::Video) {
        Dropped::Video
    } else if kinds
        .iter()
        .any(|k| matches!(k, Kind::Subtitle | Kind::UnsupportedSubtitle))
    {
        Dropped::SubtitlesOnly
    } else {
        Dropped::NotVideo
    }
}

impl Dropped {
    fn word(self) -> &'static str {
        match self {
            Dropped::Video => "video",
            Dropped::SubtitlesOnly => "subOnly",
            Dropped::NotVideo => "notVideo",
        }
    }
}

/// Opens the Start window, or brings it forward. It counts as seen from now
/// on: the Player never asks for a sign-in of its own after it (§7).
pub fn open(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let seen = state.settings.lock().map(|s| s.start_seen).unwrap_or(true);
        if !seen {
            let _ = state.update_settings(|s| s.start_seen = true);
        }
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        crate::refresh_dock(&handle, true);
        if let Some(win) = handle.get_webview_window(LABEL) {
            let _ = win.show();
            let _ = win.set_focus();
            return;
        }
        if let Err(e) = build(&handle) {
            log::warn!("could not open the Start window: {e}");
            crate::refresh_dock(&handle, false);
        }
    });
}

/// Closes the Start window if it is open (a video arrived, or sign-in finished).
pub fn close(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(win) = handle.get_webview_window(LABEL) {
            let _ = win.destroy();
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

/// The Start window's place: centred in the work area of the monitor under the pointer.
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

/// Files dropped on the Start window (§5.4): a video plays (and the Start
/// window goes); anything else is refused, and the page says why.
pub fn dropped(app: &AppHandle, paths: &[PathBuf]) {
    let kind = classify(paths);
    if kind == Dropped::Video && crate::open_media(app, paths) {
        return;
    }
    let _ = app.emit_to(LABEL, REFUSED_EVENT, kind.word());
}

/// What the Start window needs to draw: this computer and whether it is signed in.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WelcomeState {
    os: &'static str,
    signed_in: bool,
}

#[tauri::command]
pub async fn welcome_state(app: AppHandle) -> WelcomeState {
    let signed_in = app.state::<AppState>().api.user().await.is_some();
    WelcomeState {
        os: if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(windows) {
            "windows"
        } else {
            "linux"
        },
        signed_in,
    }
}

/// "Open a video…" (§4): the system's open dialog over the Start window,
/// videos and subtitles, several at once. A video picked plays in the Player
/// and the Start window goes ("opened"); "cancelled", or "subOnly" /
/// "notVideo" when nothing playable was picked.
#[tauri::command]
pub async fn start_open_video(app: AppHandle) -> Result<String, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut exts: Vec<&str> = files::VIDEO_EXTS.to_vec();
    exts.extend_from_slice(files::SUBTITLE_EXTS);
    let mut dialog = app
        .dialog()
        .file()
        .add_filter("Videos and subtitles", &exts)
        .add_filter("Videos", files::VIDEO_EXTS)
        .add_filter("Subtitles", files::SUBTITLE_EXTS);
    if let Some(win) = app.get_webview_window(LABEL) {
        dialog = dialog.set_parent(&win);
    }
    dialog.pick_files(move |picked| {
        let _ = tx.send(picked);
    });
    let picked = rx.await.map_err(|_| "cancelled")?.unwrap_or_default();
    let paths: Vec<PathBuf> = picked
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .collect();
    if paths.is_empty() {
        return Ok("cancelled".into());
    }
    let kind = classify(&paths);
    if kind == Dropped::Video && crate::open_media(&app, &paths) {
        return Ok("opened".into());
    }
    Ok(kind.word().into())
}

/// Whether a sign-in that just finished must open Lexpad's window itself:
/// it was started from the Start window, which the learner closed while the
/// browser was still open (the page is gone and cannot ask).
pub fn finish_for_closed_start(caller: &str, start_open: bool) -> bool {
    caller == LABEL && !start_open
}

/// Sign-in from the Start window finished (§4): `focus` learns of it, the
/// learning window opens (its onboarding or Today), and the Start window goes.
#[tauri::command]
pub fn start_signed_in(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    state.update_settings(|s| {
        s.focus = Some(focus_after_sign_in(s.focus));
        s.last_place = Place::Learn;
    })?;
    crate::main_window::open(&app, None);
    close(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::Screen;

    fn base() -> Launch {
        Launch::default()
    }

    #[test]
    fn a_first_launch_signed_out_opens_the_start_window() {
        // A, and D: a later launch, still signed out, never played a video.
        assert_eq!(first_window(base()), FirstWindow::Start);
    }

    #[test]
    fn a_video_that_opened_lexpad_is_the_only_window() {
        // B, signed in or out, whatever the settings say.
        for signed_in in [false, true] {
            let l = Launch {
                opened_file: true,
                signed_in,
                open_on_launch: true,
                player_flag: true,
                ..base()
            };
            assert_eq!(first_window(l), FirstWindow::Nothing);
        }
    }

    #[test]
    fn signed_in_opens_where_it_was_left_never_the_start_window() {
        // C
        let l = Launch {
            signed_in: true,
            last_place: Place::Player,
            ..base()
        };
        assert_eq!(first_window(l), FirstWindow::Player);
        let l = Launch {
            signed_in: true,
            last_place: Place::Learn,
            ..base()
        };
        assert_eq!(first_window(l), FirstWindow::Learn);
        let l = Launch {
            signed_in: true,
            last_place: Place::Player,
            open_at: OpenAt::Today,
            ..base()
        };
        assert_eq!(first_window(l), FirstWindow::Learn);
    }

    #[test]
    fn signed_out_after_a_video_opens_the_player() {
        // E
        let l = Launch {
            played_video: true,
            ..base()
        };
        assert_eq!(first_window(l), FirstWindow::Player);
    }

    #[test]
    fn at_login_nothing_opens_unless_settings_ask_for_lexpad() {
        // F
        let l = Launch {
            at_login: true,
            ..base()
        };
        assert_eq!(first_window(l), FirstWindow::Nothing);
        let l = Launch {
            at_login: true,
            signed_in: true,
            ..base()
        };
        assert_eq!(first_window(l), FirstWindow::Nothing);
        let l = Launch {
            at_login: true,
            signed_in: true,
            open_on_launch: true,
            ..base()
        };
        assert_eq!(first_window(l), FirstWindow::Learn);
    }

    #[test]
    fn the_player_flag_opens_the_player() {
        // G
        for signed_in in [false, true] {
            let l = Launch {
                player_flag: true,
                signed_in,
                ..base()
            };
            assert_eq!(first_window(l), FirstWindow::Player);
        }
    }

    #[test]
    fn a_sign_in_from_a_closed_start_window_still_opens_lexpad() {
        assert!(finish_for_closed_start(LABEL, false));
        // The page is still there: it asks itself.
        assert!(!finish_for_closed_start(LABEL, true));
        // Signed in from elsewhere (Settings, the card): nothing more.
        assert!(!finish_for_closed_start("settings", false));
    }

    #[test]
    fn what_the_learner_does_sets_the_focus() {
        assert_eq!(focus_after_video(None), Focus::Watch);
        assert_eq!(focus_after_video(Some(Focus::Learn)), Focus::Both);
        assert_eq!(focus_after_video(Some(Focus::Watch)), Focus::Watch);
        assert_eq!(focus_after_sign_in(None), Focus::Learn);
        assert_eq!(focus_after_sign_in(Some(Focus::Watch)), Focus::Both);
        assert_eq!(focus_after_sign_in(Some(Focus::Both)), Focus::Both);
    }

    #[test]
    fn a_drop_is_a_video_subtitles_alone_or_something_else() {
        let p = |names: &[&str]| names.iter().map(PathBuf::from).collect::<Vec<_>>();
        assert_eq!(classify(&p(&["a.mkv", "a.srt"])), Dropped::Video);
        assert_eq!(classify(&p(&["Film.MP4"])), Dropped::Video);
        assert_eq!(classify(&p(&["a.srt"])), Dropped::SubtitlesOnly);
        assert_eq!(classify(&p(&["a.sup"])), Dropped::SubtitlesOnly);
        assert_eq!(classify(&p(&["notes.pdf"])), Dropped::NotVideo);
        assert_eq!(classify(&[]), Dropped::NotVideo);
    }

    #[test]
    fn the_start_window_is_centred_and_fits_a_small_screen() {
        let s = Screen {
            area: Area::new(0.0, 0.0, 1440.0, 900.0),
            work: Area::new(0.0, 25.0, 1440.0, 800.0),
            scale: 2.0,
        };
        let r = centred(&[s], Some((10.0, 10.0)), true).unwrap();
        assert_eq!((r.w, r.h), (1060.0, 620.0));
        assert!(s.work.holds(&r));
        // A 1280×720 screen with a taskbar keeps the whole window.
        let hd = Screen {
            area: Area::new(0.0, 0.0, 1280.0, 720.0),
            work: Area::new(0.0, 0.0, 1280.0, 672.0),
            scale: 1.0,
        };
        let r = centred(&[hd], None, true).unwrap();
        assert_eq!((r.w, r.h), (1060.0, 620.0));
        assert!(hd.work.holds(&r));
        let tiny = Screen {
            area: Area::new(0.0, 0.0, 800.0, 500.0),
            work: Area::new(0.0, 0.0, 800.0, 500.0),
            scale: 1.0,
        };
        let r = centred(&[tiny], None, true).unwrap();
        assert_eq!((r.w, r.h), (800.0, 500.0));
    }
}
