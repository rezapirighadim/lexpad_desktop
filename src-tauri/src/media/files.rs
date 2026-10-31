//! The files the player may read: only those the learner gave it (dropped
//! on a window, chosen in the open dialog, opened with "Open with Lexpad"),
//! and the videos and subtitle files beside a video they gave (its other
//! episodes and their subtitles). A window never passes a path: it gets an
//! opaque id for each file, and every command takes the id.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;

/// The extensions the player opens as video. Everything FFmpeg reads that
/// people actually have; the rest are refused before probing.
pub const VIDEO_EXTS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "asf", "flv", "f4v", "ts", "m2ts", "mts",
    "mpg", "mpeg", "vob", "ogv", "3gp", "3g2", "divx", "rm", "rmvb", "mxf",
];
/// Text subtitle files the player reads.
pub const SUBTITLE_EXTS: &[&str] = &["srt", "vtt", "webvtt", "ass", "ssa"];
/// Subtitle files it recognises but cannot show: pictures (VobSub's
/// .idx/.sub, Blu-ray's .sup) and formats not read yet (SAMI, MicroDVD).
pub const UNSUPPORTED_SUBTITLE_EXTS: &[&str] = &["idx", "sub", "sup", "smi", "sami"];
/// The most files one drop or one folder scan grants.
const MAX_GRANT: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Video,
    Subtitle,
    UnsupportedSubtitle,
    Other,
}

pub fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub fn kind_of(path: &Path) -> Kind {
    let ext = ext_of(path);
    if VIDEO_EXTS.contains(&ext.as_str()) {
        Kind::Video
    } else if SUBTITLE_EXTS.contains(&ext.as_str()) {
        Kind::Subtitle
    } else if UNSUPPORTED_SUBTITLE_EXTS.contains(&ext.as_str()) {
        Kind::UnsupportedSubtitle
    } else {
        Kind::Other
    }
}

/// A file as a window sees it: an id, its name, what it is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRef {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub size: u64,
}

/// A drop or an open, sorted: the player plays the first video (a new video
/// replaces the current one) and asks about each subtitle.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dropped {
    pub videos: Vec<FileRef>,
    pub subtitles: Vec<FileRef>,
    /// Names only: what was given and cannot be played or shown.
    pub unsupported: Vec<String>,
    /// How they came, for the activity reports (`player.open` via): `drop`
    /// on the Player, `open_with` from outside it (the system's "Open with",
    /// a second launch, a drop on another Lexpad window). None from the
    /// open dialog, which the page asked for itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<&'static str>,
}

#[derive(Default)]
pub struct Files {
    by_id: Mutex<HashMap<String, PathBuf>>,
    by_path: Mutex<HashMap<PathBuf, String>>,
}

fn new_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl Files {
    /// Grants one file (the same path keeps its id) and describes it.
    pub fn grant(&self, path: &Path) -> Option<FileRef> {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() {
            return None;
        }
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let id = {
            let mut by_path = self.by_path.lock().ok()?;
            if let Some(id) = by_path.get(&path) {
                id.clone()
            } else {
                let id = new_id();
                by_path.insert(path.clone(), id.clone());
                self.by_id.lock().ok()?.insert(id.clone(), path.clone());
                id
            }
        };
        Some(FileRef {
            id,
            name: path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_owned(),
            kind: kind_of(&path),
            size: meta.len(),
        })
    }

    pub fn path(&self, id: &str) -> Option<PathBuf> {
        self.by_id.lock().ok()?.get(id).cloned()
    }

    /// Sorts and grants what was dropped or opened. A folder dropped
    /// grants the videos and subtitles directly inside it.
    pub fn grant_all(&self, paths: &[PathBuf]) -> Dropped {
        let mut out = Dropped::default();
        let mut list: Vec<PathBuf> = Vec::new();
        for p in paths {
            if p.is_dir() {
                list.extend(media_in(p));
            } else {
                list.push(p.clone());
            }
        }
        list.sort_by(|a, b| {
            crate::media::series::natural_cmp(
                a.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                b.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            )
        });
        for p in list.into_iter().take(MAX_GRANT) {
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_owned();
            match kind_of(&p) {
                Kind::Video | Kind::Subtitle => match self.grant(&p) {
                    Some(f) if f.kind == Kind::Video => out.videos.push(f),
                    Some(f) => out.subtitles.push(f),
                    None => out.unsupported.push(name),
                },
                _ => out.unsupported.push(name),
            }
        }
        out
    }

    /// The videos and subtitle files beside `video` (and in its `Subs`
    /// folder), granted: for the episode list and matching subtitles.
    pub fn grant_siblings(&self, video: &Path) -> Vec<(PathBuf, FileRef)> {
        let Some(dir) = video.parent() else {
            return Vec::new();
        };
        let mut paths = media_in(dir);
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_lowercase();
                if p.is_dir() && matches!(name.as_str(), "subs" | "sub" | "subtitles" | "subtitle")
                {
                    paths.extend(
                        media_in(&p)
                            .into_iter()
                            .filter(|p| kind_of(p) == Kind::Subtitle),
                    );
                    // Releases put each episode's subtitles in Subs/<episode>/.
                    if let Ok(inner) = std::fs::read_dir(&p) {
                        for d in inner.flatten().filter(|d| d.path().is_dir()) {
                            paths.extend(
                                media_in(&d.path())
                                    .into_iter()
                                    .filter(|p| kind_of(p) == Kind::Subtitle),
                            );
                        }
                    }
                }
            }
        }
        paths
            .into_iter()
            .take(MAX_GRANT)
            .filter_map(|p| self.grant(&p).map(|f| (p, f)))
            .collect()
    }
}

/// Videos and subtitle files directly in `dir`, hidden files left out.
fn media_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && !p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with('.'))
                && matches!(kind_of(p), Kind::Video | Kind::Subtitle)
        })
        .take(MAX_GRANT)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_sorts_and_refuses() {
        let dir = std::env::temp_dir().join(format!("lexpad-files-{}", new_id()));
        std::fs::create_dir_all(dir.join("Subs")).unwrap();
        for n in [
            "b.mkv",
            "a.mp4",
            "a.fa.srt",
            "notes.txt",
            "Subs/a.en.srt",
            ".hidden.mkv",
        ] {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        let files = Files::default();
        let d = files.grant_all(&[
            dir.join("b.mkv"),
            dir.join("a.mp4"),
            dir.join("a.fa.srt"),
            dir.join("notes.txt"),
        ]);
        assert_eq!(
            d.videos.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["a.mp4", "b.mkv"]
        );
        assert_eq!(d.subtitles[0].name, "a.fa.srt");
        assert_eq!(d.unsupported, ["notes.txt"]);
        // The same file keeps its id; an unknown id is nothing.
        let again = files.grant(&dir.join("a.mp4")).unwrap();
        assert_eq!(again.id, d.videos[0].id);
        assert!(files.path("nope").is_none());
        let siblings = files.grant_siblings(&dir.join("a.mp4"));
        let mut names: Vec<_> = siblings.iter().map(|(_, f)| f.name.clone()).collect();
        names.sort();
        assert_eq!(names, ["a.en.srt", "a.fa.srt", "a.mp4", "b.mkv"]);
        // A dropped folder brings its media.
        let d = files.grant_all(std::slice::from_ref(&dir));
        assert_eq!(d.videos.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
