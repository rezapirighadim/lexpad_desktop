//! Which window a launch opens (Player spec `uiux/2026-11-08-player/spec.md`
//! B1, owner-approved 8 Nov 2026: QB1 Player home for everybody, QB4 no
//! Start window). Decided by `first_window`, a pure function with a test per
//! row. A video that opened Lexpad just plays: the decision waits
//! `FIRST_WINDOW_GRACE` after the app is ready (macOS hands launch documents
//! over as `RunEvent::Opened` after `setup`).
//!
//! The Start window of 0.3.x (`welcome.html`, desktop first-run spec
//! 2026-10-26) is retired: Player home is the first window now, signed in or
//! out, and its first-run card says what the Start window said.

use std::time::Duration;

use crate::settings::{Focus, OpenAt};

/// How long after the app is ready the first window waits for a video the
/// system is still handing over. macOS delivers launch documents within
/// about 200 ms; 600 ms keeps a margin without a visible wait.
pub const FIRST_WINDOW_GRACE: Duration = Duration::from_millis(600);

/// What a launch knows when it decides which window to open.
#[derive(Clone, Copy, Debug, Default)]
pub struct Launch {
    /// Started by the system at login (`--login`, the autostart's argument).
    pub at_login: bool,
    /// `--player`: the Start menu's "Lexpad Player", a pinned Player button.
    pub player_flag: bool,
    /// A video (or subtitles) opened Lexpad; the Player already has it.
    pub opened_file: bool,
    pub signed_in: bool,
    /// Settings: open Lexpad's window when the app starts.
    pub open_on_launch: bool,
    /// Settings → This computer → "When I open Lexpad".
    pub open_at: OpenAt,
}

/// The window a launch opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstWindow {
    /// Nothing: the menu-bar / tray icon only.
    Nothing,
    /// Player home (or the Player with what it was playing).
    Player,
    /// Lexpad's window, at Today.
    Learn,
}

/// The launch table:
/// B a video opened Lexpad → the Player has it, nothing more;
/// G `--player` → the Player;
/// F at login → nothing (or Lexpad's window when Settings asks for it);
/// otherwise (A, C, D, E) → Player home, unless a signed-in learner chose
/// Today in Settings. Today needs an account, so signed out it is the Player.
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
    if l.signed_in && l.open_at == OpenAt::Today {
        FirstWindow::Learn
    } else {
        FirstWindow::Player
    }
}

/// `focus` after a video was opened: watch, or both after learn.
pub fn focus_after_video(now: Option<Focus>) -> Focus {
    match now {
        None | Some(Focus::Watch) => Focus::Watch,
        Some(Focus::Learn) | Some(Focus::Both) => Focus::Both,
    }
}

/// The caller of a sign-in that Lexpad finished after macOS started it again
/// for `lexpad://auth/callback` (`commands::resume_sign_in`): the window that
/// began it is gone with the Lexpad that was quit. Never a window's label.
pub const RESUMED: &str = "resumed";

/// Whether a sign-in that just finished must open Lexpad's window on its
/// first-run setup: it was started from the Player, the shortcut's card, the
/// menu-bar panel or the small Settings window, or it was finished after a
/// restart (`RESUMED`), and the account has no notebook yet (`notebooks`: how
/// many the API listed, `None` when it could not say).
///
/// The web app's first run (two languages and a first word) is the only
/// place a notebook is made, and only Lexpad's window reaches it. Signed in
/// from one of these, a new account had nowhere to put a word: the card said
/// "Create a notebook in Lexpad first." and nothing opened (admin audit,
/// 9 Nov 2026: a third of the desktop sign-ups ended with no notebook). Since
/// Player home is the first window (0.3.30), most sign-ins start in the
/// Player, so it follows the same rule (owner, 9 Nov 2026). Lexpad's own
/// window already goes there by itself.
pub fn opens_setup_after_sign_in(caller: &str, notebooks: Option<usize>) -> bool {
    matches!(
        caller,
        crate::player_window::LABEL
            | crate::popup::LABEL
            | crate::panel::LABEL
            | crate::SETTINGS
            | RESUMED
    ) && notebooks == Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Launch {
        Launch::default()
    }

    #[test]
    fn every_launch_opens_player_home_signed_in_or_out() {
        // A (first launch), D (later, signed out), E (signed out after a video).
        assert_eq!(first_window(base()), FirstWindow::Player);
        // C: signed in, the default.
        let l = Launch {
            signed_in: true,
            ..base()
        };
        assert_eq!(first_window(l), FirstWindow::Player);
        let l = Launch {
            signed_in: true,
            open_at: OpenAt::Player,
            ..base()
        };
        assert_eq!(first_window(l), FirstWindow::Player);
    }

    #[test]
    fn a_learner_who_chose_today_gets_today() {
        let l = Launch {
            signed_in: true,
            open_at: OpenAt::Today,
            ..base()
        };
        assert_eq!(first_window(l), FirstWindow::Learn);
        // Today needs an account: signed out it is Player home.
        let l = Launch {
            open_at: OpenAt::Today,
            ..base()
        };
        assert_eq!(first_window(l), FirstWindow::Player);
    }

    #[test]
    fn a_video_that_opened_lexpad_is_the_only_window() {
        // B, signed in or out, whatever the settings say.
        for signed_in in [false, true] {
            for open_at in [OpenAt::Today, OpenAt::Player] {
                let l = Launch {
                    opened_file: true,
                    signed_in,
                    open_on_launch: true,
                    player_flag: true,
                    open_at,
                    ..base()
                };
                assert_eq!(first_window(l), FirstWindow::Nothing);
            }
        }
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
        // G, even for a learner who chose Today.
        for signed_in in [false, true] {
            let l = Launch {
                player_flag: true,
                signed_in,
                open_at: OpenAt::Today,
                ..base()
            };
            assert_eq!(first_window(l), FirstWindow::Player);
        }
    }

    #[test]
    fn what_the_learner_does_sets_the_focus() {
        assert_eq!(focus_after_video(None), Focus::Watch);
        assert_eq!(focus_after_video(Some(Focus::Learn)), Focus::Both);
        assert_eq!(focus_after_video(Some(Focus::Watch)), Focus::Watch);
        assert_eq!(focus_after_video(Some(Focus::Both)), Focus::Both);
    }

    #[test]
    fn a_new_account_signed_in_from_the_player_the_card_the_panel_or_settings_opens_setup() {
        for caller in [
            crate::player_window::LABEL,
            crate::popup::LABEL,
            crate::panel::LABEL,
            crate::SETTINGS,
        ] {
            assert!(opens_setup_after_sign_in(caller, Some(0)), "{caller}");
            // An account that already has a notebook goes on where it was.
            assert!(!opens_setup_after_sign_in(caller, Some(1)), "{caller}");
            assert!(!opens_setup_after_sign_in(caller, Some(3)), "{caller}");
            // The notebooks could not be read (offline, a server error): nothing is guessed.
            assert!(!opens_setup_after_sign_in(caller, None), "{caller}");
        }
        // Lexpad's own window reaches the first run by itself.
        assert!(!opens_setup_after_sign_in(
            crate::main_window::LABEL,
            Some(0)
        ));
        // Anything else (a development page) is left alone.
        assert!(!opens_setup_after_sign_in("player-dev", Some(0)));
    }

    #[test]
    fn a_sign_in_finished_after_a_restart_follows_the_same_rule() {
        assert!(opens_setup_after_sign_in(RESUMED, Some(0)));
        assert!(!opens_setup_after_sign_in(RESUMED, Some(1)));
        assert!(!opens_setup_after_sign_in(RESUMED, None));
    }

    #[test]
    fn the_resumed_caller_is_no_window_label() {
        for label in [
            crate::main_window::LABEL,
            crate::player_window::LABEL,
            crate::popup::LABEL,
            crate::panel::LABEL,
            crate::SETTINGS,
        ] {
            assert_ne!(RESUMED, label);
        }
    }
}
