//! What a player window may ask of the media engine. Every command takes
//! file ids from `files` (never a path) and session ids from `media_open`;
//! each is listed in build.rs and granted in capabilities/main.json (and
//! player-dev.json for the development page). Errors are short words the
//! page maps to its own text: `unknown_file`, `ffmpeg_missing`,
//! `unreadable`, `no_video`, `no_encoder`, `unsupported_subtitle`,
//! `unreadable_subtitle`, `empty_subtitle`, `too_big`, `file_gone`,
//! `no_session`, `timeout`. Events: `media:progress` (a run's buffer) and
//! `media:subtitle-progress` (an embedded track being read).

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_dialog::DialogExt as _;

use super::default_player;
use super::files::{self, Dropped, FileRef, Kind};
use super::plan::{self, Mode, Options, Plan};
use super::probe::{MediaInfo, SubtitleKind};
use super::protocol;
use super::resume::{self, Entry};
use super::series;
use super::stream::Session;
use super::subs::{lang, Format};
use super::tools;
use super::{load_track, read_subtitle_file, MediaState, Track};

type Media<'a> = State<'a, Arc<MediaState>>;

fn unplayable(u: plan::Unplayable) -> String {
    match u {
        plan::Unplayable::NoVideo => "no_video".into(),
        plan::Unplayable::NoEncoder => "no_encoder".into(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineDto {
    available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    caps: Option<tools::Caps>,
    licence: &'static str,
    /// The scheme's origin, as the page writes media addresses.
    base_url: &'static str,
}

/// Whether the engine is there, its FFmpeg, and what this machine can do.
#[tauri::command]
pub async fn media_engine(media: Media<'_>) -> Result<EngineDto, String> {
    let Ok(tools) = media.tools() else {
        return Ok(EngineDto {
            available: false,
            version: None,
            caps: None,
            licence: "LGPL-2.1-or-later",
            base_url: protocol::base_url(),
        });
    };
    Ok(EngineDto {
        available: true,
        version: tools::version(&tools).await,
        caps: Some(tools::caps(&tools).await),
        licence: "LGPL-2.1-or-later",
        base_url: protocol::base_url(),
    })
}

/// The system's open dialog, for videos and subtitles (several at once).
#[tauri::command]
pub async fn media_pick<R: Runtime>(
    app: AppHandle<R>,
    media: Media<'_>,
) -> Result<Dropped, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut exts: Vec<&str> = files::VIDEO_EXTS.to_vec();
    exts.extend_from_slice(files::SUBTITLE_EXTS);
    app.dialog()
        .file()
        .add_filter("Videos and subtitles", &exts)
        .add_filter("Videos", files::VIDEO_EXTS)
        .add_filter("Subtitles", files::SUBTITLE_EXTS)
        .pick_files(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = rx.await.map_err(|_| "cancelled")?.unwrap_or_default();
    let paths: Vec<PathBuf> = picked
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .collect();
    Ok(media.files.grant_all(&paths))
}

/// Files opened from the system before the page was listening.
#[tauri::command]
pub fn media_take_pending(media: Media<'_>) -> Option<Dropped> {
    media.take_pending()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeDto {
    info: Arc<MediaInfo>,
    /// How it would play with the default choices...
    #[serde(skip_serializing_if = "Option::is_none")]
    plan: Option<Plan>,
    /// ...or why it cannot.
    #[serde(skip_serializing_if = "Option::is_none")]
    unplayable: Option<plan::Unplayable>,
}

#[tauri::command]
pub async fn media_probe(media: Media<'_>, file_id: String) -> Result<ProbeDto, String> {
    let (path, info) = media.probe(&file_id).await?;
    let tools = media.tools()?;
    let caps = tools::caps(&tools).await;
    let decided = plan::decide(&info, &files::ext_of(&path), Options::default(), &caps);
    let (plan, unplayable) = match decided {
        Ok(p) => (Some(p), None),
        Err(u) => (None, Some(u)),
    };
    Ok(ProbeDto {
        info,
        plan,
        unplayable,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDto {
    /// Pull from here (`?from=<offset>`); absent for `Direct`.
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    /// Where the run starts, in the `<video>` element's seconds.
    start: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackDto {
    session: String,
    file: FileRef,
    mode: Mode,
    /// `Direct`: the `<video src>`. Streamed modes: the first run.
    #[serde(skip_serializing_if = "Option::is_none")]
    src: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run: Option<RunDto>,
    mime: String,
    /// Seconds to add to the file's time for the element's (subtitles,
    /// chapters, resume): 0 for `Direct`.
    time_offset: f64,
    duration: Option<f64>,
    plan: Plan,
    info: Arc<MediaInfo>,
    /// Where to start (the saved position), in the file's seconds.
    resume_at: f64,
    /// The saved choices for this video.
    saved: Entry,
}

fn file_ref(media: &MediaState, id: &str) -> Result<(PathBuf, FileRef), String> {
    let path = media.files.path(id).ok_or("unknown_file")?;
    let f = media.files.grant(&path).ok_or("file_gone")?;
    Ok((path, f))
}

fn run_dto(session: &Session, run: Option<Arc<super::stream::Run>>, offset: f64) -> Option<RunDto> {
    run.map(|r| RunDto {
        url: Some(format!(
            "{}/s/{}/{}",
            protocol::base_url(),
            session.id,
            r.id
        )),
        start: r.start + offset,
    })
}

fn playback(session: &Arc<Session>, file: FileRef, at: f64, saved: Entry) -> PlaybackDto {
    let plan = session.plan();
    let run = session.start_run(at);
    let src =
        (plan.mode == Mode::Direct).then(|| format!("{}/f/{}", protocol::base_url(), session.id));
    PlaybackDto {
        session: session.id.clone(),
        file,
        mode: plan.mode,
        src,
        run: run_dto(session, run, plan.time_offset),
        mime: plan.mime.clone(),
        time_offset: plan.time_offset,
        duration: session.info.duration,
        info: session.info.clone(),
        resume_at: at,
        saved,
        plan,
    }
}

/// While a run converts: how far it has got (the processing screen's
/// "It starts in about 4 seconds", the seek bar's converted part).
pub const PROGRESS_EVENT: &str = "media:progress";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressDto {
    session: String,
    run: u64,
    /// The element's seconds converted so far.
    ready_until: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    eta_seconds: Option<f64>,
}

/// Opens a video: probes it, decides how it plays (with the choices saved
/// for it unless the page gives new ones), and starts its stream at the
/// saved position (or `at`).
#[tauri::command]
pub async fn media_open<R: Runtime>(
    app: AppHandle<R>,
    media: Media<'_>,
    file_id: String,
    audio: Option<usize>,
    audio_delay_ms: Option<i64>,
    at: Option<f64>,
) -> Result<PlaybackDto, String> {
    let (path, file) = file_ref(&media, &file_id)?;
    if file.kind != Kind::Video {
        return Err("no_video".into());
    }
    let (_, info) = media.probe(&file_id).await?;
    let tools = media.tools()?;
    let caps = tools::caps(&tools).await;
    let key = resume::key_for(&path, file.size);
    let saved = media.resume.get(&key).unwrap_or_default();
    let opts = Options {
        audio: audio.or(saved.audio),
        audio_delay_ms: audio_delay_ms.unwrap_or(saved.audio_delay_ms),
    };
    let id = uuid::Uuid::new_v4().simple().to_string();
    let emitter = app.clone();
    let sid = id.clone();
    let session = Arc::new(
        Session::new(id, file_id, path, info, tools, caps, opts)
            .map_err(unplayable)?
            .with_progress(Arc::new(move |run, ready, eta| {
                let _ = emitter.emit(
                    PROGRESS_EVENT,
                    ProgressDto {
                        session: sid.clone(),
                        run,
                        ready_until: ready + plan::TIMELINE_OFFSET,
                        eta_seconds: eta,
                    },
                );
            })),
    );
    media.add_session(session.clone());
    let at = at.unwrap_or(saved.position);
    Ok(playback(&session, file, at, saved))
}

/// A seek outside what the page has: a new run from `at` (the element's
/// seconds). `Direct` sessions seek in the element and never call this.
#[tauri::command]
pub fn media_seek(media: Media<'_>, session: String, at: f64) -> Result<RunDto, String> {
    let s = media.session(&session).ok_or("no_session")?;
    let offset = s.plan().time_offset;
    let run = s.start_run((at - offset).max(0.0)).ok_or("direct")?;
    run_dto(&s, Some(run), offset).ok_or_else(|| "direct".into())
}

/// A new audio track or delay, kept for this video: a new plan (which may
/// change the mode, e.g. `Direct` to `Remux`) and a new run from `at`.
#[tauri::command]
pub fn media_set_audio(
    media: Media<'_>,
    session: String,
    audio: Option<usize>,
    audio_delay_ms: i64,
    at: f64,
) -> Result<PlaybackDto, String> {
    let s = media.session(&session).ok_or("no_session")?;
    let (path, file) = file_ref(&media, &s.file_id)?;
    let old_offset = s.plan().time_offset;
    s.replan(Options {
        audio,
        audio_delay_ms,
    })
    .map_err(unplayable)?;
    let key = resume::key_for(&path, file.size);
    let saved = media
        .resume
        .update(&key, |e| {
            e.audio = audio;
            e.audio_delay_ms = audio_delay_ms;
        })
        .unwrap_or_default();
    Ok(playback(&s, file, (at - old_offset).max(0.0), saved))
}

#[tauri::command]
pub fn media_close(media: Media<'_>, session: String) {
    media.close_session(&session);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedDto {
    key: String,
    stream: usize,
    codec: String,
    supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    default: bool,
    forced: bool,
    hearing_impaired: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalDto {
    key: String,
    file: FileRef,
    /// 2 named after the video, 1 the same episode, 0 only beside it.
    fit: u8,
    hint: lang::NameHint,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitlesDto {
    embedded: Vec<EmbeddedDto>,
    external: Vec<ExternalDto>,
}

/// The subtitles a video has: its own tracks, and the files beside it that
/// belong to it (by name, or by episode), best first.
#[tauri::command]
pub async fn media_subtitles(media: Media<'_>, file_id: String) -> Result<SubtitlesDto, String> {
    let (path, info) = media.probe(&file_id).await?;
    let embedded = info
        .subtitles
        .iter()
        .map(|s| EmbeddedDto {
            key: format!("embedded:{}", s.index),
            stream: s.index,
            codec: s.codec.clone(),
            supported: s.kind == SubtitleKind::Text && s.extract_as.is_some(),
            language: s.language,
            title: s.title.clone(),
            default: s.default,
            forced: s.forced,
            hearing_impaired: s.hearing_impaired,
        })
        .collect();
    let siblings = media.files.grant_siblings(&path);
    let videos = siblings
        .iter()
        .filter(|(_, f)| f.kind == Kind::Video)
        .count();
    let mut external: Vec<ExternalDto> = siblings
        .into_iter()
        .filter(|(_, f)| f.kind == Kind::Subtitle)
        .filter_map(|(p, f)| {
            let fit = series::subtitle_fit(&path, &p);
            // A film alone in its folder: every subtitle there is its own.
            if fit == 0 && videos > 1 {
                return None;
            }
            Some(ExternalDto {
                key: format!("file:{}", f.name),
                hint: lang::from_file_name(&f.name),
                file: f,
                fit,
            })
        })
        .collect();
    external.sort_by(|a, b| {
        b.fit
            .cmp(&a.fit)
            .then_with(|| series::natural_cmp(&a.file.name, &b.file.name))
    });
    Ok(SubtitlesDto { embedded, external })
}

/// The event a player window gets while an embedded track is read out of
/// a big file or one on a slow drive: `{ fileId, stream, fraction }`.
pub const SUBTITLE_PROGRESS_EVENT: &str = "media:subtitle-progress";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubtitleProgressDto {
    file_id: String,
    stream: usize,
    /// 0..1 of the film's length.
    fraction: f64,
}

/// Loads a subtitle: an embedded track (`stream` of the video `fileId`) or
/// a subtitle file (`fileId` alone). Decoded, parsed, its language found.
/// An embedded track is read in the background of the page (the command is
/// async); how far it is comes as `media:subtitle-progress`, at most a few
/// times a second.
#[tauri::command]
pub async fn media_subtitle_load<R: Runtime>(
    app: AppHandle<R>,
    media: Media<'_>,
    file_id: String,
    stream: Option<usize>,
) -> Result<Track, String> {
    let (path, file) = file_ref(&media, &file_id)?;
    match stream {
        Some(index) => {
            let (_, info) = media.probe(&file_id).await?;
            let st = info
                .subtitles
                .iter()
                .find(|s| s.index == index)
                .ok_or("unsupported_subtitle")?;
            if st.kind != SubtitleKind::Text {
                return Err("unsupported_subtitle".into());
            }
            let last = std::sync::Mutex::new((std::time::Instant::now(), -1.0_f64));
            let id = file_id.clone();
            let progress = move |fraction: f64| {
                let Ok(mut l) = last.lock() else { return };
                // A whole percent more, and not more often than every 250 ms.
                if fraction - l.1 < 0.01 || l.0.elapsed() < std::time::Duration::from_millis(250) {
                    return;
                }
                *l = (std::time::Instant::now(), fraction);
                let _ = app.emit(
                    SUBTITLE_PROGRESS_EVENT,
                    SubtitleProgressDto {
                        file_id: id.clone(),
                        stream: index,
                        fraction,
                    },
                );
            };
            let bytes = media.extract(&path, &info, st, &progress).await?;
            load_track(
                format!("embedded:{index}"),
                &bytes,
                st.extract_as,
                None,
                st.language,
            )
        }
        None => {
            if file.kind != Kind::Subtitle {
                return Err("unsupported_subtitle".into());
            }
            let bytes = read_subtitle_file(&path)?;
            let hint = lang::from_file_name(&file.name);
            load_track(
                format!("file:{}", file.name),
                &bytes,
                Format::from_ext(&files::ext_of(&path)),
                hint.lang,
                None,
            )
        }
    }
}

/// Keeps a subtitle's offset for this video (milliseconds, + is later).
#[tauri::command]
pub fn media_subtitle_offset(
    media: Media<'_>,
    file_id: String,
    key: String,
    offset_ms: i64,
) -> Result<(), String> {
    let (path, file) = file_ref(&media, &file_id)?;
    if key.len() > 300 {
        return Err("key".into());
    }
    media
        .resume
        .update(&resume::key_for(&path, file.size), |e| {
            if offset_ms == 0 {
                e.subtitle_offsets.remove(&key);
            } else {
                e.subtitle_offsets.insert(key, offset_ms);
            }
        });
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeDto {
    file: FileRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    season: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    episode: Option<u32>,
    position: f64,
    duration: f64,
    watched: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodesDto {
    items: Vec<EpisodeDto>,
    current: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<FileRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous: Option<FileRef>,
}

/// The episodes of a video's show in its folder, in order, each with where
/// it was left; a film is a list of one.
#[tauri::command]
pub fn media_episodes(media: Media<'_>, file_id: String) -> Result<EpisodesDto, String> {
    let (path, _) = file_ref(&media, &file_id)?;
    let siblings = media.files.grant_siblings(&path);
    let videos: Vec<PathBuf> = siblings
        .iter()
        .filter(|(_, f)| f.kind == Kind::Video)
        .map(|(p, _)| p.clone())
        .collect();
    let canonical = path.canonicalize().unwrap_or(path.clone());
    let order = series::episodes_of(&canonical, &videos);
    let mut items = Vec::new();
    let mut current = 0;
    for p in order {
        let Some(f) = media.files.grant(&p) else {
            continue;
        };
        if f.id == file_id {
            current = items.len();
        }
        let parsed = series::parse(&f.name);
        let e = media
            .resume
            .get(&resume::key_for(&p, f.size))
            .unwrap_or_default();
        items.push(EpisodeDto {
            season: parsed.as_ref().and_then(|x| x.ep.season),
            episode: parsed.as_ref().map(|x| x.ep.episode),
            position: e.position,
            duration: e.duration,
            watched: e.watched,
            file: f,
        });
    }
    let next = items.get(current + 1).map(|e| e.file.clone());
    let previous = current
        .checked_sub(1)
        .and_then(|i| items.get(i))
        .map(|e| e.file.clone());
    Ok(EpisodesDto {
        items,
        current,
        next,
        previous,
    })
}

/// Saves where the video is (the file's seconds), every few seconds and on
/// pause or close.
#[tauri::command]
pub fn media_resume_save(
    media: Media<'_>,
    file_id: String,
    position: f64,
    duration: f64,
) -> Result<Entry, String> {
    let (path, file) = file_ref(&media, &file_id)?;
    if !position.is_finite() || !duration.is_finite() {
        return Err("position".into());
    }
    media
        .resume
        .save_position(
            &resume::key_for(&path, file.size),
            position.max(0.0),
            duration.max(0.0),
            media.prefs.get().remember_recent.then_some(path.as_path()),
        )
        .ok_or_else(|| "lock".into())
}

/// The most entries "Continue watching" shows.
const RECENT_MAX: usize = 8;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentDto {
    /// The resume store's key, for `media_recent_remove`.
    key: String,
    /// The file, granted again, when it is still where it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    file_id: Option<String>,
    name: String,
    position: f64,
    duration: f64,
    watched: bool,
    /// Seconds since 1970.
    updated: i64,
    missing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    season: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    episode: Option<u32>,
    /// A frame at `position` (JPEG over the lexmedia scheme), when the file is there.
    #[serde(skip_serializing_if = "Option::is_none")]
    thumb: Option<String>,
}

/// "Continue watching": the files watched last on this computer, newest first.
#[tauri::command]
pub fn media_recent(media: Media<'_>) -> Vec<RecentDto> {
    media
        .resume
        .recent(RECENT_MAX)
        .into_iter()
        .filter_map(|(key, e)| {
            let path = e.path.clone()?;
            let granted = media.files.grant(&path);
            let name = e.name.clone().unwrap_or_default();
            let parsed = series::parse(&name);
            let file_id = granted.map(|f| f.id);
            Some(RecentDto {
                thumb: file_id
                    .as_ref()
                    .map(|id| super::thumbs::url(id, e.position)),
                missing: file_id.is_none(),
                file_id,
                season: parsed.as_ref().and_then(|p| p.ep.season),
                episode: parsed.as_ref().map(|p| p.ep.episode),
                name,
                position: e.position,
                duration: e.duration,
                watched: e.watched,
                updated: e.updated,
                key,
            })
        })
        .collect()
}

#[tauri::command]
pub fn media_recent_remove(media: Media<'_>, key: String) {
    media.resume.forget_recent(Some(&key));
}

#[tauri::command]
pub fn media_recent_clear(media: Media<'_>) {
    media.resume.forget_recent(None);
}

/// "Remember recent files" (the player's Settings): off stops new entries
/// (positions are still kept, so a file still resumes).
#[tauri::command]
pub fn media_set_remember_recent(media: Media<'_>, on: bool) -> bool {
    media
        .prefs
        .update(|p| p.remember_recent = on)
        .remember_recent
}

#[tauri::command]
pub fn media_prefs(media: Media<'_>) -> super::prefs::Prefs {
    media.prefs.get()
}

/// The development page's benchmark: one line of results to stdout (and to
/// `LEXPAD_PLAYER_BENCH_OUT` when set), read by scripts/player-bench.sh.
/// Does nothing without the `player-dev` feature (only player-dev.json
/// grants it, to a window only that feature makes).
#[tauri::command]
pub fn media_dev_report(report: serde_json::Value) {
    if !cfg!(feature = "player-dev") {
        return;
    }
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let line = serde_json::json!({ "t": t, "report": report }).to_string();
    println!("LEXPAD_BENCH {line}");
    if let Ok(path) = std::env::var("LEXPAD_PLAYER_BENCH_OUT") {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// The development page's benchmark: the files it plays, from
/// `LEXPAD_PLAYER_BENCH` (a folder), granted like a drop. Nothing without
/// the `player-dev` feature.
#[tauri::command]
pub fn media_dev_bench_files(media: Media<'_>) -> Option<Dropped> {
    if !cfg!(feature = "player-dev") {
        return None;
    }
    let dir = std::env::var("LEXPAD_PLAYER_BENCH").ok()?;
    Some(media.files.grant_all(&[PathBuf::from(dir)]))
}

/// Whether Lexpad opens each type by default (`types` empty: every type
/// the player offers), and how `media_default_player_request` works here.
#[tauri::command]
pub fn media_default_player_status<R: Runtime>(
    app: AppHandle<R>,
    types: Option<Vec<String>>,
) -> default_player::Status {
    let types = default_player::known(&types.unwrap_or_default());
    default_player::status(&app.config().identifier, &types)
}

/// "Use Lexpad for video files", after the page asked the learner: on
/// macOS sets Lexpad as the default for `types` and returns the new status;
/// on Windows opens Settings > Default apps at Lexpad (Windows lets only
/// the learner choose there) and returns the status as it is.
#[tauri::command]
pub fn media_default_player_request<R: Runtime>(
    app: AppHandle<R>,
    types: Vec<String>,
) -> Result<default_player::Status, String> {
    let types = default_player::known(&types);
    if types.is_empty() {
        return Err("no_types".into());
    }
    let id = app.config().identifier.clone();
    if cfg!(windows) {
        #[cfg(windows)]
        let packaged = crate::msix::is_packaged();
        #[cfg(not(windows))]
        let packaged = false;
        use tauri_plugin_opener::OpenerExt as _;
        app.opener()
            .open_url(default_player::settings_page(packaged), None::<&str>)
            .map_err(|_| "could_not_open_settings")?;
    } else {
        // What opened each type before, to give it back on request.
        let media = app.state::<Arc<MediaState>>();
        for ext in &types {
            if let Some(prev) =
                default_player::current_handler(ext).filter(|h| !h.eq_ignore_ascii_case(&id))
            {
                media.prefs.update(|p| {
                    p.previous_handlers.insert((*ext).to_owned(), prev);
                });
            }
        }
        let failed = default_player::set_default(&id, &types);
        for (ext, code) in &failed {
            log::warn!("media: could not make Lexpad the default for .{ext} ({code})");
        }
    }
    Ok(default_player::status(&id, &types))
}

/// Gives types back to the app that opened them before Lexpad (macOS; all
/// types Lexpad took when `types` is empty). Windows: Settings > Default
/// apps, where the learner chooses.
#[tauri::command]
pub fn media_default_player_restore<R: Runtime>(
    app: AppHandle<R>,
    media: Media<'_>,
    types: Option<Vec<String>>,
) -> Result<default_player::Status, String> {
    let id = app.config().identifier.clone();
    let asked = default_player::known(&types.unwrap_or_default());
    if cfg!(windows) {
        #[cfg(windows)]
        let packaged = crate::msix::is_packaged();
        #[cfg(not(windows))]
        let packaged = false;
        use tauri_plugin_opener::OpenerExt as _;
        app.opener()
            .open_url(default_player::settings_page(packaged), None::<&str>)
            .map_err(|_| "could_not_open_settings")?;
        return Ok(default_player::status(&id, &asked));
    }
    let previous = media.prefs.get().previous_handlers;
    for ext in &asked {
        let Some(prev) = previous.get(*ext) else {
            continue;
        };
        if default_player::set_default(prev, &[*ext]).is_empty() {
            media.prefs.update(|p| {
                p.previous_handlers.remove(*ext);
            });
        }
    }
    Ok(default_player::status(&id, &asked))
}
