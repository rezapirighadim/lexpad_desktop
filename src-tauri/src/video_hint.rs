//! The shortcut card's hint when a video app is in front (spec
//! `uiux/2026-10-31-player-next/spec.md` A5): "Watching in VLC? Open the film
//! in Lexpad Player and click any word." Nothing can be selected in a video
//! app, so the card would only offer typing; the Player lets the learner
//! click the word instead.
//!
//! Shown at most once per app every `PER_APP_DAYS`, `MAX_TOTAL` times in all
//! on this computer, and never signed out. The core decides and records in
//! one step (`claim`), so two cards opened at once cannot both count.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Days before the same app's hint may show again (spec A5). A product rule
/// about this app's own card, not a value any server or flag holds.
pub const PER_APP_DAYS: i64 = 14;
/// How many hints in all on this computer (spec A5).
pub const MAX_TOTAL: u32 = 3;

/// The video apps, by the name the card says, and every display name the
/// systems report for them: macOS `localizedName`, Windows the exe's
/// FileDescription (`capture/windows.rs`). Compared without case.
const APPS: &[(&str, &[&str])] = &[
    ("VLC", &["VLC", "VLC media player"]),
    ("IINA", &["IINA"]),
    ("QuickTime Player", &["QuickTime Player"]),
    ("mpv", &["mpv", "mpv.net"]),
    ("Infuse", &["Infuse", "Infuse 7", "Infuse 8"]),
    // The Store app is "Movies & TV" (US), "Films & TV" (UK); its process
    // describes itself as Video.UI.
    ("Movies & TV", &["Movies & TV", "Films & TV", "Video.UI"]),
    (
        "Media Player",
        &[
            "Media Player",
            "Microsoft.Media.Player",
            "Windows Media Player",
            "Windows Media Player Legacy",
        ],
    ),
    ("PotPlayer", &["PotPlayer", "Daum PotPlayer", "PotPlayer64"]),
    ("MPC-HC", &["MPC-HC", "MPC-HC x64", "MPC-BE", "MPC-BE x64"]),
    ("KMPlayer", &["KMPlayer", "The KMPlayer", "KMPlayer 64X"]),
    ("GOM Player", &["GOM Player", "GOMPlayer"]),
];

/// The video app `display_name` is, by the name the card uses.
pub fn video_app(display_name: &str) -> Option<&'static str> {
    let name = display_name.trim();
    APPS.iter()
        .find(|(_, names)| names.iter().any(|n| n.eq_ignore_ascii_case(name)))
        .map(|(app, _)| *app)
}

/// What this computer remembers of the hint, in `settings.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct VideoHint {
    /// When each app's hint last showed (RFC 3339), by the card's name.
    pub shown: HashMap<String, String>,
    /// How many hints have shown in all.
    pub total: u32,
}

/// Whether the hint for `app` is due now, and if so records it as shown.
/// Returns the name the card says.
pub fn claim(
    memory: &mut VideoHint,
    display_name: &str,
    signed_in: bool,
    now: DateTime<Utc>,
) -> Option<&'static str> {
    if !signed_in || memory.total >= MAX_TOTAL {
        return None;
    }
    let app = video_app(display_name)?;
    if let Some(last) = memory
        .shown
        .get(app)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
    {
        if now - last.with_timezone(&Utc) < Duration::days(PER_APP_DAYS) {
            return None;
        }
    }
    memory.shown.insert(app.to_string(), now.to_rfc3339());
    memory.total += 1;
    Some(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
            + Duration::days(day)
    }

    #[test]
    fn the_video_apps_are_known_by_every_name_the_systems_give() {
        assert_eq!(video_app("VLC"), Some("VLC"));
        assert_eq!(video_app("vlc media player"), Some("VLC"));
        assert_eq!(video_app(" IINA "), Some("IINA"));
        assert_eq!(video_app("quicktime player"), Some("QuickTime Player"));
        assert_eq!(video_app("Films & TV"), Some("Movies & TV"));
        assert_eq!(video_app("Windows Media Player"), Some("Media Player"));
        assert_eq!(video_app("The KMPlayer"), Some("KMPlayer"));
        assert_eq!(video_app("GOM Player"), Some("GOM Player"));
        // Exact names only: a browser playing a film is not a video app.
        assert_eq!(video_app("Safari"), None);
        assert_eq!(video_app("VLC Remote Helper"), None);
        assert_eq!(video_app(""), None);
    }

    #[test]
    fn once_per_app_every_14_days() {
        let mut m = VideoHint::default();
        assert_eq!(claim(&mut m, "VLC", true, at(0)), Some("VLC"));
        assert_eq!(claim(&mut m, "VLC media player", true, at(13)), None);
        assert_eq!(claim(&mut m, "VLC", true, at(14)), Some("VLC"));
        assert_eq!(m.total, 2);
    }

    #[test]
    fn three_in_all_then_never_again() {
        let mut m = VideoHint::default();
        assert!(claim(&mut m, "VLC", true, at(0)).is_some());
        assert!(claim(&mut m, "IINA", true, at(0)).is_some());
        assert!(claim(&mut m, "mpv", true, at(1)).is_some());
        assert_eq!(claim(&mut m, "Infuse", true, at(2)), None);
        assert_eq!(claim(&mut m, "VLC", true, at(60)), None);
        assert_eq!(m.total, 3);
    }

    #[test]
    fn never_signed_out_and_never_for_other_apps() {
        let mut m = VideoHint::default();
        assert_eq!(claim(&mut m, "VLC", false, at(0)), None);
        assert_eq!(claim(&mut m, "TextEdit", true, at(0)), None);
        assert_eq!(m, VideoHint::default());
    }

    #[test]
    fn an_unreadable_date_does_not_block_the_hint() {
        let mut m = VideoHint::default();
        m.shown.insert("VLC".into(), "yesterday".into());
        assert_eq!(claim(&mut m, "VLC", true, at(0)), Some("VLC"));
    }

    #[test]
    fn an_old_settings_file_reads_as_never_shown() {
        let m: VideoHint = serde_json::from_str("{}").unwrap();
        assert_eq!(m, VideoHint::default());
    }
}
