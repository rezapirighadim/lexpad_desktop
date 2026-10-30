//! Embedded text subtitles out of a video ("soft subs"): every text track
//! of the file in one FFmpeg pass that copies the subtitle packets and skips
//! the picture and the sound (`-discard`, nothing is decoded), each to its
//! own file in a temporary folder, then kept in memory per file (its path,
//! size and modified time), so choosing another track of the same film, or
//! opening it again, reads nothing from the disk.
//!
//! One pass for all of them because FFmpeg has to walk the whole container
//! to find a track's packets: on a slow drive, one walk instead of one per
//! track. If that pass fails (a broken track can stop the muxer), the track
//! asked for is extracted alone, and what it gave is kept even when FFmpeg
//! stopped partway, as long as it holds something.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use super::probe::{MediaInfo, SubtitleKind, SubtitleStream};
use super::subs::Format;

/// What identifies a file's contents: its path, size and modified time.
/// A file id is per grant; this survives opening the same film again.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileKey {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
}

impl FileKey {
    pub fn of(path: &Path) -> Result<FileKey, String> {
        let meta = std::fs::metadata(path).map_err(|_| "file_gone")?;
        Ok(FileKey {
            path: path.to_owned(),
            size: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

/// How a track leaves the container: FFmpeg's codec (`copy` keeps the
/// bytes as they are, so an old encoding reaches the detector untouched)
/// and its muxer, and the file's extension. `None` for a track that is
/// not text.
pub fn output_for(st: &SubtitleStream) -> Option<(&'static str, &'static str, &'static str)> {
    if st.kind != SubtitleKind::Text {
        return None;
    }
    match (st.codec.as_str(), st.extract_as) {
        ("subrip" | "srt", _) => Some(("copy", "srt", "srt")),
        ("ass" | "ssa", _) => Some(("copy", "ass", "ass")),
        ("webvtt", _) => Some(("copy", "webvtt", "vtt")),
        // mov_text and the other text codecs FFmpeg reads: written as SubRip.
        (_, Some(Format::Srt)) => Some(("srt", "srt", "srt")),
        _ => None,
    }
}

/// The text tracks one pass extracts: every text track of the file.
pub fn text_tracks(info: &MediaInfo) -> Vec<&SubtitleStream> {
    info.subtitles
        .iter()
        .filter(|s| output_for(s).is_some())
        .collect()
}

/// Where one track goes in the pass's folder.
pub fn out_path(dir: &Path, st: &SubtitleStream) -> Option<PathBuf> {
    let (_, _, ext) = output_for(st)?;
    Some(dir.join(format!("{}.{ext}", st.index)))
}

/// FFmpeg's arguments for one pass: the input with its picture and sound
/// discarded, then one output per track (`-map 0:<index>`), and the
/// progress on standard output.
pub fn args(input: &str, info: &MediaInfo, tracks: &[&SubtitleStream], dir: &Path) -> Vec<String> {
    let mut a: Vec<String> = [
        "-hide_banner",
        "-nostdin",
        "-nostats",
        "-loglevel",
        "error",
        "-y",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    // Skipping a stream's packets in the demuxer, so they are not even
    // handed on; a specifier that matches nothing is an error, so only
    // what the file has.
    if !info.video.is_empty() {
        a.extend(["-discard:v".into(), "all".into()]);
    }
    if !info.audio.is_empty() {
        a.extend(["-discard:a".into(), "all".into()]);
    }
    a.extend(["-i".into(), input.to_owned()]);
    for st in tracks {
        let (Some((codec, muxer, _)), Some(out)) = (output_for(st), out_path(dir, st)) else {
            continue;
        };
        a.extend([
            "-map".into(),
            format!("0:{}", st.index),
            "-c:s".into(),
            codec.into(),
            "-f".into(),
            muxer.into(),
            out.to_string_lossy().into_owned(),
        ]);
    }
    a.extend(["-progress".into(), "pipe:1".into()]);
    a
}

/// How far a pass is, from the position FFmpeg reports and the film's length.
pub fn fraction(seconds: f64, duration: Option<f64>) -> Option<f64> {
    let d = duration.filter(|d| *d > 0.0)?;
    Some((seconds / d).clamp(0.0, 1.0))
}

/// The extracted tracks, by file and stream, the oldest dropped past
/// `MAX_TRACKS` (a track is tens of kilobytes; this bounds a long session).
#[derive(Default)]
pub struct Cache {
    map: HashMap<(FileKey, usize), Arc<Vec<u8>>>,
    order: VecDeque<(FileKey, usize)>,
}

/// The most tracks kept in memory.
const MAX_TRACKS: usize = 64;

impl Cache {
    pub fn get(&self, key: &FileKey, index: usize) -> Option<Arc<Vec<u8>>> {
        self.map.get(&(key.clone(), index)).cloned()
    }

    pub fn put(&mut self, key: &FileKey, index: usize, bytes: Arc<Vec<u8>>) {
        let k = (key.clone(), index);
        if self.map.insert(k.clone(), bytes).is_none() {
            self.order.push_back(k);
        }
        while self.order.len() > MAX_TRACKS {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }
}

/// A temporary folder for one pass, removed when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new() -> Result<Scratch, String> {
        let dir =
            std::env::temp_dir().join(format!("lexpad-subs-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("temp: {e}"))?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::probe;
    use serde_json::json;

    fn info() -> MediaInfo {
        probe::parse(&json!({
            "format": {"format_name": "matroska,webm", "duration": "100.0"},
            "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "hevc", "width": 1280, "height": 720},
                {"index": 1, "codec_type": "audio", "codec_name": "aac"},
                {"index": 2, "codec_type": "subtitle", "codec_name": "subrip", "tags": {"language": "per"}},
                {"index": 3, "codec_type": "subtitle", "codec_name": "ass", "tags": {"language": "eng"}},
                {"index": 4, "codec_type": "subtitle", "codec_name": "hdmv_pgs_subtitle"},
                {"index": 5, "codec_type": "subtitle", "codec_name": "mov_text"},
                {"index": 6, "codec_type": "subtitle", "codec_name": "webvtt"},
                {"index": 7, "codec_type": "subtitle", "codec_name": "something_new"}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn only_text_tracks_are_extracted_each_in_its_own_format() {
        let i = info();
        let tracks = text_tracks(&i);
        let got: Vec<(usize, &str, &str)> = tracks
            .iter()
            .map(|s| {
                let (c, m, _) = output_for(s).unwrap();
                (s.index, c, m)
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (2, "copy", "srt"),
                (3, "copy", "ass"),
                (5, "srt", "srt"),
                (6, "copy", "webvtt"),
            ]
        );
        // Pictures and codecs we do not know are never handed to FFmpeg.
        assert!(output_for(&i.subtitles[2]).is_none());
        assert!(output_for(&i.subtitles[5]).is_none());
    }

    #[test]
    fn one_pass_maps_every_track_and_skips_picture_and_sound() {
        let i = info();
        let tracks = text_tracks(&i);
        let dir = Path::new("/tmp/x");
        let a = args("/films/a.mkv", &i, &tracks, dir);
        let s = a.join(" ");
        assert!(
            s.contains("-discard:v all -discard:a all -i /films/a.mkv"),
            "{s}"
        );
        // The output files are joined with the system's separator (a backslash on Windows).
        let out = |name: &str| dir.join(name).display().to_string();
        let want = [
            format!("-map 0:2 -c:s copy -f srt {}", out("2.srt")),
            format!("-map 0:3 -c:s copy -f ass {}", out("3.ass")),
            format!("-map 0:5 -c:s srt -f srt {}", out("5.srt")),
            format!("-map 0:6 -c:s copy -f webvtt {}", out("6.vtt")),
        ];
        for w in &want {
            assert!(s.contains(w.as_str()), "{w} in {s}");
        }
        assert!(!s.contains("0:4"), "no picture track: {s}");
        assert!(s.ends_with("-progress pipe:1"), "{s}");
        // A file of subtitles alone has nothing to discard.
        let only = probe::parse(&json!({
            "format": {"format_name": "matroska,webm"},
            "streams": [{"index": 0, "codec_type": "subtitle", "codec_name": "subrip"}]
        }))
        .unwrap();
        let a = args("/a.mkv", &only, &text_tracks(&only), dir).join(" ");
        assert!(!a.contains("-discard"), "{a}");
    }

    #[test]
    fn progress_is_a_clamped_fraction_of_the_length() {
        assert_eq!(fraction(50.0, Some(100.0)), Some(0.5));
        assert_eq!(fraction(150.0, Some(100.0)), Some(1.0));
        assert_eq!(fraction(5.0, None), None);
        assert_eq!(fraction(5.0, Some(0.0)), None);
    }

    #[test]
    fn the_cache_keeps_the_newest_tracks() {
        let mut c = Cache::default();
        let k = |n: u64| FileKey {
            path: PathBuf::from("/a.mkv"),
            size: n,
            modified: None,
        };
        for n in 0..(MAX_TRACKS as u64 + 5) {
            c.put(&k(n), 2, Arc::new(vec![1]));
        }
        assert_eq!(c.len(), MAX_TRACKS);
        assert!(c.get(&k(0), 2).is_none(), "the oldest went");
        assert!(c.get(&k(MAX_TRACKS as u64 + 4), 2).is_some());
        // The same film changed on disk (another size) is another key.
        assert!(c.get(&k(MAX_TRACKS as u64 + 4), 3).is_none());
    }
}
