//! The video player's media engine. The player's UI is a page (Lexpad's
//! window, written by us); this is what it plays through:
//!
//! - `files`: the files the learner gave (drop, open dialog, "Open with
//!   Lexpad"), by opaque id. Nothing else on disk is ever read.
//! - `tools`: the bundled LGPL FFmpeg and ffprobe, and what they can do here.
//! - `probe`: what is in a file. `plan`: how it reaches the webview, the
//!   same on macOS and Windows (direct, remux, audio or video transcode).
//! - `stream` and `protocol`: the stream itself, over the `lexmedia` scheme.
//! - `subs`: subtitles (embedded and beside the file; encodings; language).
//! - `series`: episodes in a folder, next and previous, matched subtitles.
//! - `resume`: where each video stopped, and its choices.
//! - `default_player`: "Use Lexpad for video files", only when asked.
//! - `commands`: what a window may ask (typed in `src/lib/media/api.ts`).

pub mod commands;
pub mod default_player;
pub mod files;
pub mod plan;
pub mod probe;
pub mod protocol;
pub mod resume;
pub mod series;
pub mod stream;
pub mod subs;
pub mod tools;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use files::{Dropped, Files};
use probe::MediaInfo;
use resume::Resume;
use stream::Session;
use subs::{decode, lang, Format, Parsed};
use tools::Tools;

/// The event a player window gets when files are dropped on it or opened
/// with Lexpad, with a `Dropped`.
pub const FILES_EVENT: &str = "media:files";
/// How long extracting one embedded subtitle track may take (FFmpeg reads
/// the whole file for it).
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(120);
/// The biggest subtitle file read: real ones are well under 1 MB.
const MAX_SUBTITLE_BYTES: u64 = 20 * 1024 * 1024;
/// At most this many sessions at once (one per player; a few for overlap).
const MAX_SESSIONS: usize = 4;

/// An embedded track: (file id, stream index).
type ExtractKey = (String, usize);

pub struct MediaState {
    pub files: Files,
    pub resume: Resume,
    tools: Mutex<Option<Tools>>,
    probes: Mutex<HashMap<String, Arc<MediaInfo>>>,
    sessions: Mutex<Vec<Arc<Session>>>,
    /// Files opened before a player page was listening.
    pending: Mutex<Option<Dropped>>,
    /// Embedded tracks already extracted: (file id, stream index) → bytes.
    extracted: Mutex<HashMap<ExtractKey, Arc<Vec<u8>>>>,
}

impl MediaState {
    pub fn new(resume_file: Option<PathBuf>) -> MediaState {
        MediaState {
            files: Files::default(),
            resume: Resume::load(resume_file),
            tools: Mutex::new(None),
            probes: Mutex::new(HashMap::new()),
            sessions: Mutex::new(Vec::new()),
            pending: Mutex::new(None),
            extracted: Mutex::new(HashMap::new()),
        }
    }

    pub fn tools(&self) -> Result<Tools, String> {
        let mut t = self.tools.lock().map_err(|_| "lock")?;
        if let Some(t) = t.as_ref() {
            return Ok(t.clone());
        }
        let found = Tools::locate()?;
        *t = Some(found.clone());
        Ok(found)
    }

    pub fn session(&self, id: &str) -> Option<Arc<Session>> {
        self.sessions
            .lock()
            .ok()?
            .iter()
            .find(|s| s.id == id)
            .cloned()
    }

    pub fn add_session(&self, s: Arc<Session>) {
        if let Ok(mut list) = self.sessions.lock() {
            while list.len() >= MAX_SESSIONS {
                let old = list.remove(0);
                old.stop();
            }
            list.push(s);
        }
    }

    pub fn close_session(&self, id: &str) {
        if let Ok(mut list) = self.sessions.lock() {
            if let Some(i) = list.iter().position(|s| s.id == id) {
                let s = list.remove(i);
                s.stop();
            }
        }
    }

    pub async fn probe(&self, file_id: &str) -> Result<(PathBuf, Arc<MediaInfo>), String> {
        let path = self.files.path(file_id).ok_or("unknown_file")?;
        if let Some(info) = self.probes.lock().map_err(|_| "lock")?.get(file_id) {
            return Ok((path, info.clone()));
        }
        let tools = self.tools()?;
        let json = tools::probe_json(&tools, &path).await?;
        let info = Arc::new(probe::parse(&json).ok_or("unreadable")?);
        self.probes
            .lock()
            .map_err(|_| "lock")?
            .insert(file_id.to_owned(), info.clone());
        Ok((path, info))
    }

    pub fn take_pending(&self) -> Option<Dropped> {
        self.pending.lock().ok()?.take()
    }

    /// The bytes of an embedded text track, as the container holds them
    /// (copied, so an old encoding reaches the detector untouched), or
    /// converted to SubRip for codecs that are not a file format of their
    /// own (mov_text).
    pub async fn extract(
        &self,
        file_id: &str,
        path: &Path,
        stream: &probe::SubtitleStream,
    ) -> Result<Arc<Vec<u8>>, String> {
        let key = (file_id.to_owned(), stream.index);
        if let Some(b) = self.extracted.lock().map_err(|_| "lock")?.get(&key) {
            return Ok(b.clone());
        }
        let tools = self.tools()?;
        let (codec, fmt) = match (stream.codec.as_str(), stream.extract_as) {
            ("subrip" | "srt", _) => ("copy", "srt"),
            ("ass" | "ssa", _) => ("copy", "ass"),
            ("webvtt", _) => ("copy", "webvtt"),
            (_, Some(Format::Srt)) => ("srt", "srt"),
            _ => return Err("unsupported_subtitle".into()),
        };
        let path_s = path.to_str().ok_or("path")?;
        let map = format!("0:{}", stream.index);
        let out = tools::output(
            &tools.ffmpeg,
            &[
                "-hide_banner",
                "-nostdin",
                "-loglevel",
                "error",
                "-i",
                path_s,
                "-map",
                &map,
                "-c:s",
                codec,
                "-f",
                fmt,
                "pipe:1",
            ],
            EXTRACT_TIMEOUT,
        )
        .await?;
        let out = Arc::new(out);
        self.extracted
            .lock()
            .map_err(|_| "lock")?
            .insert(key, out.clone());
        Ok(out)
    }
}

/// A subtitle track ready for the player.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    /// The page's key for this track (for offsets and choices).
    pub key: String,
    #[serde(flatten)]
    pub parsed: Parsed,
    pub encoding: decode::Decoded,
    pub language: lang::Detection,
    /// Arabic yeh and kaf were written as Persian (an old Windows-1256 file).
    pub persian_letters_fixed: bool,
    /// Punctuation typed at the start of right-to-left lines (for players
    /// that showed them left to right) was put back at their end.
    pub punctuation_moved: bool,
}

/// Bytes of a subtitle (a file or an extracted track) to a `Track`.
pub fn load_track(
    key: String,
    bytes: &[u8],
    format: Option<Format>,
    name_lang: Option<&'static str>,
    track_lang: Option<&'static str>,
) -> Result<Track, String> {
    let decoded = decode::decode(bytes, name_lang.or(track_lang));
    let mut parsed = subs::parse(&decoded.text, format).ok_or("unreadable_subtitle")?;
    if parsed.cues.is_empty() {
        return Err("empty_subtitle".into());
    }
    let sample: String = parsed
        .cues
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let language = lang::detect(&sample, name_lang, track_lang);
    let mut fixed = false;
    if language.best.as_ref().is_some_and(|g| g.code == "fa") {
        for cue in &mut parsed.cues {
            let (t, changed) = decode::persian_letters(&cue.text);
            if changed {
                fixed = true;
                cue.text = t;
                for span in &mut cue.spans {
                    span.text = decode::persian_letters(&span.text).0;
                }
            }
        }
    }
    let rtl = language
        .best
        .as_ref()
        .is_some_and(|g| matches!(g.code, "fa" | "ar" | "he" | "ur" | "ps" | "ku"));
    let mut punctuation_moved = false;
    if rtl {
        for cue in &mut parsed.cues {
            let fixed = subs::rtl_punctuation(&cue.text);
            if fixed != cue.text {
                punctuation_moved = true;
                cue.text = fixed;
                // Only a cue in one style is rewritten span by span; a
                // styled one keeps its spans (its text is fixed above).
                if cue.spans.len() == 1 {
                    cue.spans[0].text = subs::rtl_punctuation(&cue.spans[0].text);
                }
            }
        }
    }
    Ok(Track {
        key,
        parsed,
        punctuation_moved,
        encoding: decoded,
        language,
        persian_letters_fixed: fixed,
    })
}

/// Reads a subtitle file the learner gave.
pub fn read_subtitle_file(path: &Path) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|_| "file_gone")?;
    if meta.len() > MAX_SUBTITLE_BYTES {
        return Err("too_big".into());
    }
    std::fs::read(path).map_err(|_| "file_gone".into())
}

/// Files dropped on a player window: granted, sorted, and sent to it.
pub fn deliver<R: Runtime>(app: &AppHandle<R>, window: &str, paths: &[PathBuf]) {
    let Some(state) = app.try_state::<Arc<MediaState>>() else {
        return;
    };
    let dropped = state.files.grant_all(paths);
    if dropped.videos.is_empty() && dropped.subtitles.is_empty() && dropped.unsupported.is_empty() {
        return;
    }
    let _ = app.emit_to(window, FILES_EVENT, dropped);
}

/// Files opened from the system ("Open with Lexpad", a double-click when
/// the learner chose Lexpad, a second launch with files): granted, kept
/// until the player page asks (`media_take_pending`, at its start), and
/// sent to it if it is already open.
pub fn open_from_system<R: Runtime>(app: &AppHandle<R>, paths: &[PathBuf]) -> bool {
    let Some(state) = app.try_state::<Arc<MediaState>>() else {
        return false;
    };
    let dropped = state.files.grant_all(paths);
    if dropped.videos.is_empty() && dropped.subtitles.is_empty() {
        return false;
    }
    let window = player_window();
    if app.get_webview_window(window).is_some() {
        let _ = app.emit_to(window, FILES_EVENT, dropped);
    } else if let Ok(mut p) = state.pending.lock() {
        *p = Some(dropped);
    }
    true
}

/// The window that plays files opened from the system: the development
/// page when this build has it, else Lexpad's window.
pub fn player_window() -> &'static str {
    if cfg!(feature = "player-dev") {
        "player-dev"
    } else {
        crate::main_window::LABEL
    }
}

/// Paths on a command line that are files to open: everything after the
/// program that is an existing file, not a flag (Windows "Open with").
pub fn files_in_args(args: &[String]) -> Vec<PathBuf> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_1256_persian_track_is_decoded_detected_and_fixed() {
        let srt = "1\r\n00:00:01,000 --> 00:00:02,000\r\nكليدهايم را روي ميز آشپزخانه گذاشتم.\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000\r\nقطار شهر ساعت نه حركت مي‌كند. چطور است؟\r\n";
        let (bytes, _, _) = encoding_rs::WINDOWS_1256.encode(srt);
        let t = load_track("fa".into(), &bytes, Some(Format::Srt), None, Some("fa")).unwrap();
        assert_eq!(t.encoding.encoding, "windows-1256");
        assert_eq!(t.language.best.as_ref().unwrap().code, "fa");
        assert!(t.persian_letters_fixed);
        assert_eq!(
            t.parsed.cues[0].text,
            "کلیدهایم را روی میز آشپزخانه گذاشتم."
        );
    }

    #[test]
    fn args_with_files() {
        let me = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let found = files_in_args(&[
            "app".into(),
            "--flag".into(),
            me.clone(),
            "/no/such/file.mkv".into(),
        ]);
        assert_eq!(found, vec![PathBuf::from(me)]);
    }
}

#[cfg(test)]
mod engine_tests;
