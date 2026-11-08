//! "Try a one-minute clip" on Player home (Player spec B1, owner QB2, 8 Nov
//! 2026): one minute of Tears of Steel (CC BY 3.0, `clip/CREDITS.txt`), with
//! two subtitle tracks, English (the language the film is spoken in) and one
//! translation the page picks for the learner (`player_open_clip`).
//!
//! The clip and every translation ship in the app's resources (`clip/`,
//! `bundle.resources` in tauri.conf.json, beside the exe in the MSIX). The
//! engine finds a film's subtitles beside it, so the core stages the video
//! with exactly the two chosen files in a folder of the app's data
//! (`stage`): a learner sees two tracks, never twenty. Nothing is downloaded.

use std::path::{Path, PathBuf};

/// The clip's folder in the resources, and its staging folder in app data.
pub const DIR: &str = "clip";
/// The translations, inside `DIR`.
const SUBS: &str = "subs";
/// The film's file name, which is also its title in the Player.
pub const VIDEO: &str = "Tears of Steel.mp4";
/// The subtitles' names: `<stem>.<language>.srt`.
const STEM: &str = "Tears of Steel";
/// The language the film is spoken in: its subtitle is always one of the two.
pub const SPOKEN: &str = "en";

fn subtitle_name(code: &str) -> String {
    format!("{STEM}.{code}.srt")
}

/// The language codes the clip has subtitles in (the resources' `subs`
/// folder), sorted; empty when this build carries no clip.
pub fn languages(resources: &Path) -> Vec<String> {
    let video = resources.join(DIR).join(VIDEO);
    if !video.is_file() {
        return Vec::new();
    }
    let Ok(dir) = std::fs::read_dir(resources.join(DIR).join(SUBS)) else {
        return Vec::new();
    };
    let prefix = format!("{STEM}.");
    let mut out: Vec<String> = dir
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|name| {
            name.strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix(".srt"))
                .filter(|code| is_code(code))
                .map(str::to_owned)
        })
        .collect();
    out.sort();
    out
}

/// A language code as the files are named: letters and one hyphen (`pt`,
/// `zh-Hant`), so a page's word can never name a path.
fn is_code(code: &str) -> bool {
    let mut parts = code.split('-');
    let first = parts.next().unwrap_or("");
    let ok = |p: &str| (2..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphabetic());
    ok(first) && parts.all(ok) && code.split('-').count() <= 2
}

/// The two subtitle files the clip plays with: English and `second`, or
/// English alone when `second` is English. `None` for a language the clip
/// does not have (the page asks only for those `languages` listed).
pub fn chosen(available: &[String], second: &str) -> Option<Vec<String>> {
    if !available.iter().any(|c| c == SPOKEN) || !available.iter().any(|c| c == second) {
        return None;
    }
    let mut out = vec![SPOKEN.to_owned()];
    if second != SPOKEN {
        out.push(second.to_owned());
    }
    Some(out)
}

/// Copies the clip into `staging` (app data) with only the chosen subtitles
/// beside it, and returns the video's path there. The video is copied once
/// (again only if its size changed, after an update); the subtitles are
/// replaced on every call.
pub fn stage(resources: &Path, staging: &Path, second: &str) -> Result<PathBuf, String> {
    let codes = chosen(&languages(resources), second).ok_or("no such clip language")?;
    let from = resources.join(DIR);
    std::fs::create_dir_all(staging).map_err(|e| e.to_string())?;
    let video = staging.join(VIDEO);
    let source = from.join(VIDEO);
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    if size(&video) != size(&source) {
        let tmp = staging.join(format!("{VIDEO}.part"));
        std::fs::copy(&source, &tmp).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &video).map_err(|e| e.to_string())?;
    }
    if let Ok(dir) = std::fs::read_dir(staging) {
        for e in dir.flatten() {
            if e.path().extension().is_some_and(|x| x == "srt") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    for code in codes {
        let name = subtitle_name(&code);
        std::fs::copy(from.join(SUBS).join(&name), staging.join(&name))
            .map_err(|e| e.to_string())?;
    }
    Ok(video)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The clip as this repository ships it.
    fn shipped() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!("lexpad-clip-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn the_shipped_clip_has_english_and_french_and_the_learners_languages() {
        let langs = languages(&shipped());
        // English is always one track; French is the fallback translation.
        for code in ["en", "fr", "fa", "es", "de", "ja", "zh-Hans", "pt"] {
            assert!(langs.iter().any(|c| c == code), "{code} in {langs:?}");
        }
        // The credit ships with it (CC BY 3.0).
        let credits = std::fs::read_to_string(shipped().join(DIR).join("CREDITS.txt")).unwrap();
        assert!(credits.contains("(CC) Blender Foundation | mango.blender.org"));
        assert!(credits.contains("CC BY 3.0"));
    }

    #[test]
    fn a_build_without_the_clip_offers_none() {
        assert!(languages(&temp()).is_empty());
    }

    #[test]
    fn two_tracks_english_and_one_translation() {
        let a: Vec<String> = ["en", "fa", "fr"].map(String::from).to_vec();
        assert_eq!(chosen(&a, "fa"), Some(vec!["en".into(), "fa".into()]));
        assert_eq!(chosen(&a, "en"), Some(vec!["en".into()]));
        assert_eq!(chosen(&a, "xx"), None);
        // A word that is not a language code never reaches the file system.
        assert_eq!(chosen(&a, "../../etc"), None);
        assert!(!is_code("../x") && !is_code("fa.srt") && !is_code(""));
        assert!(is_code("zh-Hant") && is_code("fa"));
    }

    #[test]
    fn staging_leaves_exactly_the_chosen_subtitles_beside_the_film() {
        let staging = temp();
        let video = stage(&shipped(), &staging, "fa").unwrap();
        assert_eq!(video, staging.join(VIDEO));
        assert_eq!(
            std::fs::metadata(&video).unwrap().len(),
            std::fs::metadata(shipped().join(DIR).join(VIDEO))
                .unwrap()
                .len()
        );
        let names = |dir: &Path| {
            let mut v: Vec<String> = std::fs::read_dir(dir)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().into_string().unwrap())
                .filter(|n| n.ends_with(".srt"))
                .collect();
            v.sort();
            v
        };
        assert_eq!(
            names(&staging),
            ["Tears of Steel.en.srt", "Tears of Steel.fa.srt"]
        );
        // Another language later replaces the translation; the film stays.
        stage(&shipped(), &staging, "fr").unwrap();
        assert_eq!(
            names(&staging),
            ["Tears of Steel.en.srt", "Tears of Steel.fr.srt"]
        );
        assert!(stage(&shipped(), &staging, "xx").is_err());
        std::fs::remove_dir_all(staging).unwrap();
    }

    #[test]
    fn the_clips_subtitles_start_inside_its_minute() {
        // Cut from the official files: the first line is "You're a jerk, Thom."
        // at 0:04, and none runs past the clip's 60 seconds.
        let en = std::fs::read_to_string(shipped().join(DIR).join(SUBS).join(subtitle_name("en")))
            .unwrap();
        assert!(en.starts_with("1\n00:00:04,000 --> 00:00:05,500\nYou're a jerk, Thom."));
        assert!(!en.contains("00:01:0") && !en.contains("00:02:"));
    }
}
