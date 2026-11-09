//! What the Player's line tools need from the core (spec
//! `uiux/2026-11-09-player-line-tools/spec.md` §6.3, §F): Say it back runs
//! the Web Speech API (`webkitSpeechRecognition`) inside the Player's own
//! webview, WKWebView on macOS and WebView2 on Windows. The core answers the
//! webview's microphone request for the Player window, and opens the system's
//! privacy settings when the learner has said no. The Playback menu's line
//! tools are `app_menu`'s.

use serde::Deserialize;
use tauri::webview::{PermissionKind, PermissionResponse};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt as _;

/// The Player window's answer to its page asking for a device or a browser
/// permission (`player_window::build`). The microphone is allowed: the
/// learner chose Say it back, and the system's own prompts and settings
/// (Info.plist's usage strings, Privacy & Security, Windows' microphone
/// setting) still decide. Without this WebKit would draw a second alert of
/// its own after macOS's. Everything else is left to the webview's own rule,
/// as before: the Player asks for nothing else.
pub fn page_permission(kind: PermissionKind) -> PermissionResponse {
    match kind {
        PermissionKind::Microphone => PermissionResponse::Allow,
        _ => PermissionResponse::Default,
    }
}

/// Which privacy setting a Say it back problem row opens (§6.3): the page
/// names one of these, never an address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrivacyPane {
    Microphone,
    Speech,
}

/// System Settings › Privacy & Security › Microphone.
const MAC_MICROPHONE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
/// System Settings › Privacy & Security › Speech Recognition.
const MAC_SPEECH: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_SpeechRecognition";
/// Settings › Privacy & security › Microphone.
const WINDOWS_MICROPHONE: &str = "ms-settings:privacy-microphone";

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn mac_url(pane: PrivacyPane) -> &'static str {
    match pane {
        PrivacyPane::Microphone => MAC_MICROPHONE,
        PrivacyPane::Speech => MAC_SPEECH,
    }
}

/// Windows has no speech permission of its own for WebView2 (Microsoft's
/// online service does the work); only the microphone can be refused.
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_url(pane: PrivacyPane) -> &'static str {
    match pane {
        PrivacyPane::Microphone | PrivacyPane::Speech => WINDOWS_MICROPHONE,
    }
}

/// The settings page for this system, if it has one.
fn privacy_url(pane: PrivacyPane) -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    return Some(mac_url(pane));
    #[cfg(windows)]
    return Some(windows_url(pane));
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = pane;
        None
    }
}

/// Say it back's "Open System Settings" / "Open Settings" (§6.3):
/// `invoke('player_open_privacy', { which: 'microphone' | 'speech' })`.
#[tauri::command]
pub fn player_open_privacy(app: AppHandle, which: PrivacyPane) -> Result<(), String> {
    let url = privacy_url(which).ok_or("unavailable")?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_microphone_is_allowed() {
        assert_eq!(
            page_permission(PermissionKind::Microphone),
            PermissionResponse::Allow
        );
        for kind in [
            PermissionKind::Camera,
            PermissionKind::DisplayCapture,
            PermissionKind::Geolocation,
            PermissionKind::Notifications,
            PermissionKind::ClipboardRead,
            PermissionKind::Other,
        ] {
            assert_eq!(
                page_permission(kind),
                PermissionResponse::Default,
                "{kind:?}"
            );
        }
    }

    #[test]
    fn the_page_names_a_pane_never_an_address() {
        let parse = |s: &str| serde_json::from_str::<PrivacyPane>(s);
        assert_eq!(parse("\"microphone\"").unwrap(), PrivacyPane::Microphone);
        assert_eq!(parse("\"speech\"").unwrap(), PrivacyPane::Speech);
        for bad in [
            "\"camera\"",
            "\"Microphone\"",
            "\"ms-settings:privacy-microphone\"",
            "\"https://lexpad.app\"",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn each_pane_opens_exactly_its_allow_listed_page() {
        assert_eq!(
            mac_url(PrivacyPane::Microphone),
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        );
        assert_eq!(
            mac_url(PrivacyPane::Speech),
            "x-apple.systempreferences:com.apple.preference.security?Privacy_SpeechRecognition"
        );
        assert_eq!(
            windows_url(PrivacyPane::Microphone),
            "ms-settings:privacy-microphone"
        );
        assert_eq!(
            windows_url(PrivacyPane::Speech),
            "ms-settings:privacy-microphone"
        );
        #[cfg(any(target_os = "macos", windows))]
        for pane in [PrivacyPane::Microphone, PrivacyPane::Speech] {
            assert!(privacy_url(pane).is_some());
        }
    }
}
