//! Where each video was left, on this computer only (`media-resume.json` in
//! the app's data folder): the position, and the learner's choices for that
//! video (audio track, audio delay, each subtitle's offset), so an episode
//! opens where it stopped and as it was set. Keyed by the file's name and
//! size, not its path, so a moved or renamed folder still resumes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The most videos remembered; the oldest go first.
const MAX_ENTRIES: usize = 2000;
/// Near the start, a position is not worth keeping.
const MIN_POSITION: f64 = 10.0;
/// This close to the end (or past 95%), the video counts as watched and
/// opens from the start next time.
const END_MARGIN: f64 = 30.0;
const WATCHED_SHARE: f64 = 0.95;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Entry {
    pub position: f64,
    pub duration: f64,
    pub watched: bool,
    /// Seconds since 1970, for forgetting the oldest.
    pub updated: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<usize>,
    #[serde(skip_serializing_if = "is_zero")]
    pub audio_delay_ms: i64,
    /// The file's name and where it was, for "Continue watching" (on this
    /// computer only); absent while the learner keeps no recent files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<std::path::PathBuf>,
    /// Subtitle offsets by the track's key (the page's own key for it).
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub subtitle_offsets: HashMap<String, i64>,
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

pub fn key_for(path: &Path, size: u64) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let mut h = Sha256::new();
    h.update(name.as_bytes());
    h.update(size.to_le_bytes());
    h.finalize()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub struct Resume {
    file: Option<PathBuf>,
    entries: Mutex<HashMap<String, Entry>>,
}

impl Resume {
    pub fn load(file: Option<PathBuf>) -> Resume {
        let entries = file
            .as_ref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Resume {
            file,
            entries: Mutex::new(entries),
        }
    }

    /// The remembered files, newest first.
    pub fn recent(&self, max: usize) -> Vec<(String, Entry)> {
        let Ok(entries) = self.entries.lock() else {
            return Vec::new();
        };
        let mut list: Vec<(String, Entry)> = entries
            .iter()
            .filter(|(_, e)| e.path.is_some())
            .map(|(k, e)| (k.clone(), e.clone()))
            .collect();
        list.sort_by_key(|e| std::cmp::Reverse(e.1.updated));
        list.truncate(max);
        list
    }

    /// Forgets that a file was watched recently (its position stays).
    pub fn forget_recent(&self, key: Option<&str>) {
        let keys: Vec<String> = match key {
            Some(k) => vec![k.to_owned()],
            None => self
                .entries
                .lock()
                .map(|e| e.keys().cloned().collect())
                .unwrap_or_default(),
        };
        for k in keys {
            if self.get(&k).is_some_and(|e| e.path.is_some()) {
                self.update(&k, |e| {
                    e.name = None;
                    e.path = None;
                });
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<Entry> {
        self.entries.lock().ok()?.get(key).cloned()
    }

    /// Changes one video's entry and writes the file.
    pub fn update(&self, key: &str, change: impl FnOnce(&mut Entry)) -> Option<Entry> {
        let snapshot = {
            let mut entries = self.entries.lock().ok()?;
            let e = entries.entry(key.to_owned()).or_default();
            change(e);
            e.updated = chrono::Utc::now().timestamp();
            let out = e.clone();
            if entries.len() > MAX_ENTRIES {
                let mut by_age: Vec<(String, i64)> = entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.updated))
                    .collect();
                by_age.sort_by_key(|(_, t)| *t);
                for (k, _) in by_age.into_iter().take(entries.len() - MAX_ENTRIES) {
                    entries.remove(&k);
                }
            }
            (out, serde_json::to_vec(&*entries).ok())
        };
        if let (Some(file), Some(bytes)) = (&self.file, snapshot.1) {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            // Written whole, then renamed, so a crash never leaves half a file.
            let tmp = file.with_extension("json.tmp");
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, file);
            }
        }
        Some(snapshot.0)
    }

    /// The player's position, every few seconds and on pause or close; the
    /// file is remembered for "Continue watching" when `recent` is given.
    pub fn save_position(
        &self,
        key: &str,
        position: f64,
        duration: f64,
        recent: Option<&Path>,
    ) -> Option<Entry> {
        self.update(key, |e| {
            e.duration = duration;
            if let Some(p) = recent {
                e.name = p.file_name().and_then(|n| n.to_str()).map(str::to_owned);
                e.path = Some(p.to_path_buf());
            }
            let near_end = duration > 0.0
                && (position >= duration - END_MARGIN || position >= duration * WATCHED_SHARE);
            if near_end {
                e.watched = true;
                e.position = 0.0;
            } else if position >= MIN_POSITION {
                e.position = position;
            } else {
                e.position = 0.0;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_watched_and_choices() {
        let file =
            std::env::temp_dir().join(format!("lexpad-resume-{}.json", uuid::Uuid::new_v4()));
        let r = Resume::load(Some(file.clone()));
        let k = key_for(Path::new("/x/Show.S01E01.mkv"), 1000);
        assert_eq!(k, key_for(Path::new("/elsewhere/Show.S01E01.mkv"), 1000));
        assert_ne!(k, key_for(Path::new("/x/Show.S01E01.mkv"), 1001));
        r.save_position(&k, 5.0, 1200.0, None);
        assert_eq!(r.get(&k).unwrap().position, 0.0);
        r.save_position(&k, 600.0, 1200.0, Some(Path::new("/x/Show.S01E01.mkv")));
        assert_eq!(r.recent(8)[0].1.name.as_deref(), Some("Show.S01E01.mkv"));
        assert_eq!(r.get(&k).unwrap().position, 600.0);
        r.update(&k, |e| {
            e.audio = Some(2);
            e.subtitle_offsets.insert("fa".into(), -500);
        });
        // Read back from disk.
        let again = Resume::load(Some(file.clone()));
        let e = again.get(&k).unwrap();
        assert_eq!((e.position, e.audio), (600.0, Some(2)));
        assert_eq!(e.subtitle_offsets["fa"], -500);
        again.save_position(&k, 1190.0, 1200.0, None);
        again.forget_recent(None);
        assert!(again.recent(8).is_empty());
        let e = again.get(&k).unwrap();
        assert!(e.watched);
        assert_eq!(e.position, 0.0);
        let _ = std::fs::remove_file(file);
    }
}
