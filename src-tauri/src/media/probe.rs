//! What is in a file, from ffprobe's JSON: the container, every stream
//! (codec, profile, size, frame rate, bit depth, HDR, languages, default
//! and forced flags) and the chapters.

use serde::Serialize;
use serde_json::Value;

use super::subs::{lang, Format};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    /// ffprobe's format name, e.g. "matroska,webm" or "mov,mp4,m4a,3gp,3g2,mj2".
    pub container: String,
    /// The major brand of an MP4/MOV ("isom", "mp42", "qt  "), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand: Option<String>,
    /// Seconds; `None` for a stream without a known length.
    pub duration: Option<f64>,
    /// The first timestamp, in seconds. The player's timeline starts at 0
    /// whatever this is (see `plan`).
    pub start_time: f64,
    pub size: u64,
    pub bit_rate: Option<u64>,
    pub video: Vec<VideoStream>,
    pub audio: Vec<AudioStream>,
    pub subtitles: Vec<SubtitleStream>,
    pub chapters: Vec<Chapter>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStream {
    pub index: usize,
    pub codec: String,
    pub profile: Option<String>,
    pub level: Option<i64>,
    pub pix_fmt: Option<String>,
    pub width: u32,
    pub height: u32,
    /// Pixel aspect ratio as (num, den); (1, 1) for square pixels.
    pub sar: (u32, u32),
    pub fps: Option<f64>,
    pub bit_depth: u32,
    pub interlaced: bool,
    /// The stream's own length, when the container says (it may be shorter
    /// than the file's: subtitles or audio running on after the picture).
    pub duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hdr: Option<Hdr>,
    pub default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Hdr {
    /// PQ (SMPTE ST 2084), the HDR10 and HDR10+ transfer.
    Pq,
    /// Hybrid log-gamma (ARIB STD-B67), broadcast HDR.
    Hlg,
    /// Dolby Vision with no HDR10 or HLG base layer that FFmpeg can tone-map
    /// (profile 5): colours will be wrong.
    DolbyVisionOnly,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStream {
    pub index: usize,
    pub codec: String,
    pub profile: Option<String>,
    pub channels: u32,
    pub layout: Option<String>,
    pub sample_rate: Option<u32>,
    /// ISO 639-1, when the tag is a language we know.
    pub language: Option<&'static str>,
    /// The tag as written ("eng", "per"), when it is not one we know.
    pub language_tag: Option<String>,
    pub title: Option<String>,
    pub default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SubtitleKind {
    Text,
    /// Pictures of text (PGS, VobSub, DVB): not supported.
    Image,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleStream {
    pub index: usize,
    pub codec: String,
    pub kind: SubtitleKind,
    /// The format it is extracted to: its own for SubRip, ASS and WebVTT;
    /// SubRip for mov_text and the other text codecs FFmpeg converts.
    pub extract_as: Option<Format>,
    pub language: Option<&'static str>,
    pub language_tag: Option<String>,
    pub title: Option<String>,
    pub default: bool,
    /// "Forced" (only the lines in another language, signs). A track the
    /// container calls forced but that is the only one in its language and
    /// has a full film's lines is taken as a full track (see `FULL_TRACK_EVENTS`).
    pub forced: bool,
    pub hearing_impaired: bool,
    /// How many subtitle events the track holds, when the container says
    /// (Matroska's statistics tags, written by mkvmerge).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<u64>,
}

/// A forced track holds a few lines (signs, a foreign scene); a full one
/// holds hundreds. Releases often flag their only subtitle as forced and
/// default so every player shows it (as VLC does); from this many events,
/// a forced flag on the only track of its language is that, not a signs
/// track. A named constant: it is a property of subtitles, not a setting.
pub const FULL_TRACK_EVENTS: u64 = 150;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub start: f64,
    pub end: f64,
    pub title: Option<String>,
}

fn f(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

fn u(v: &Value) -> Option<u64> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::to_owned).filter(|s| !s.is_empty())
}

fn ratio(v: &Value) -> Option<(u32, u32)> {
    let s = v.as_str()?;
    let (a, b) = s.split_once([':', '/'])?;
    let (a, b): (u32, u32) = (a.parse().ok()?, b.parse().ok()?);
    (a > 0 && b > 0).then_some((a, b))
}

fn tag(stream: &Value, name: &str) -> Option<String> {
    let tags = stream.get("tags")?.as_object()?;
    tags.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .and_then(|(_, v)| s(v))
}

/// Matroska's `NUMBER_OF_FRAMES` statistics tag (also written with a
/// language suffix, `NUMBER_OF_FRAMES-eng`).
fn events(stream: &Value) -> Option<u64> {
    let tags = stream.get("tags")?.as_object()?;
    tags.iter()
        .find(|(k, _)| {
            let k = k.to_ascii_uppercase();
            k == "NUMBER_OF_FRAMES" || k.starts_with("NUMBER_OF_FRAMES-")
        })
        .and_then(|(_, v)| u(v))
}

/// Clears the forced flag of a track that is plainly a full one: the only
/// text track of its language (or the only text track, when untagged), with
/// at least `FULL_TRACK_EVENTS` events.
fn unforce_full_tracks(subs: &mut [SubtitleStream]) {
    let text_lang = |s: &SubtitleStream| {
        (s.kind == SubtitleKind::Text)
            .then(|| s.language.map(str::to_owned).or(s.language_tag.clone()))
    };
    let langs: Vec<Option<Option<String>>> = subs.iter().map(text_lang).collect();
    for (i, st) in subs.iter_mut().enumerate() {
        let Some(own) = &langs[i] else { continue };
        if !st.forced || st.events.unwrap_or(0) < FULL_TRACK_EVENTS {
            continue;
        }
        let alone = langs
            .iter()
            .enumerate()
            .all(|(j, l)| j == i || l.as_ref() != Some(own));
        if alone {
            st.forced = false;
        }
    }
}

fn disposition(stream: &Value, name: &str) -> bool {
    stream
        .get("disposition")
        .and_then(|d| d.get(name))
        .and_then(Value::as_i64)
        == Some(1)
}

fn bit_depth(stream: &Value, pix_fmt: Option<&str>) -> u32 {
    if let Some(b) = stream
        .get("bits_per_raw_sample")
        .and_then(u)
        .filter(|b| *b > 0)
    {
        return b as u32;
    }
    match pix_fmt {
        Some(p) if p.contains("p10") || p.contains("p010") => 10,
        Some(p) if p.contains("p12") => 12,
        _ => 8,
    }
}

fn hdr(stream: &Value) -> Option<Hdr> {
    let dovi = stream
        .get("side_data_list")
        .and_then(Value::as_array)
        .and_then(|l| {
            l.iter().find(|d| {
                d.get("side_data_type")
                    .and_then(Value::as_str)
                    .is_some_and(|t| t.contains("DOVI"))
            })
        });
    match stream.get("color_transfer").and_then(Value::as_str) {
        Some("smpte2084") => Some(Hdr::Pq),
        Some("arib-std-b67") => Some(Hdr::Hlg),
        _ => {
            // Profile 5 has no compatible base layer (compatibility id 0).
            let compat = dovi
                .and_then(|d| d.get("dv_bl_signal_compatibility_id"))
                .and_then(Value::as_i64);
            (dovi.is_some() && compat.unwrap_or(0) == 0).then_some(Hdr::DolbyVisionOnly)
        }
    }
}

fn subtitle_kind(codec: &str) -> (SubtitleKind, Option<Format>) {
    match codec {
        "subrip" | "srt" => (SubtitleKind::Text, Some(Format::Srt)),
        "ass" | "ssa" => (SubtitleKind::Text, Some(Format::Ass)),
        "webvtt" => (SubtitleKind::Text, Some(Format::Vtt)),
        "mov_text" | "text" | "microdvd" | "subviewer" | "subviewer1" | "sami" | "realtext"
        | "mpl2" | "pjs" | "jacosub" | "vplayer" | "stl" | "eia_608" => {
            (SubtitleKind::Text, Some(Format::Srt))
        }
        "hdmv_pgs_subtitle" | "dvd_subtitle" | "dvb_subtitle" | "xsub" | "dvb_teletext" => {
            (SubtitleKind::Image, None)
        }
        _ => (SubtitleKind::Unknown, None),
    }
}

/// Reads ffprobe's `-show_format -show_streams -show_chapters -of json`.
pub fn parse(json: &Value) -> Option<MediaInfo> {
    let format = json.get("format")?;
    let mut info = MediaInfo {
        container: s(format.get("format_name")?)?,
        brand: format
            .get("tags")
            .and_then(|t| t.get("major_brand"))
            .and_then(s),
        duration: format.get("duration").and_then(f).filter(|d| *d > 0.0),
        start_time: format.get("start_time").and_then(f).unwrap_or(0.0),
        size: format.get("size").and_then(u).unwrap_or(0),
        bit_rate: format.get("bit_rate").and_then(u),
        video: Vec::new(),
        audio: Vec::new(),
        subtitles: Vec::new(),
        chapters: Vec::new(),
    };
    let empty = Vec::new();
    for st in json
        .get("streams")
        .and_then(Value::as_array)
        .unwrap_or(&empty)
    {
        let index = st.get("index").and_then(u).unwrap_or(0) as usize;
        let codec = st
            .get("codec_name")
            .and_then(s)
            .unwrap_or_else(|| "unknown".into());
        let language_tag = tag(st, "language");
        let language = language_tag.as_deref().and_then(lang::normalize);
        match st.get("codec_type").and_then(Value::as_str) {
            Some("video") => {
                // Cover art in an MKV or MP4 is a "video" stream of one picture.
                if disposition(st, "attached_pic") {
                    continue;
                }
                let pix_fmt = st.get("pix_fmt").and_then(s);
                let fps = st
                    .get("avg_frame_rate")
                    .and_then(ratio)
                    .or_else(|| st.get("r_frame_rate").and_then(ratio))
                    .map(|(a, b)| a as f64 / b as f64)
                    .filter(|f| *f > 0.0 && *f < 1000.0);
                info.video.push(VideoStream {
                    index,
                    profile: st.get("profile").and_then(s),
                    level: st.get("level").and_then(Value::as_i64),
                    bit_depth: bit_depth(st, pix_fmt.as_deref()),
                    width: st.get("width").and_then(u).unwrap_or(0) as u32,
                    height: st.get("height").and_then(u).unwrap_or(0) as u32,
                    sar: st
                        .get("sample_aspect_ratio")
                        .and_then(ratio)
                        .unwrap_or((1, 1)),
                    fps,
                    interlaced: matches!(
                        st.get("field_order").and_then(Value::as_str),
                        Some("tt" | "bb" | "tb" | "bt")
                    ),
                    hdr: hdr(st),
                    duration: st
                        .get("duration")
                        .and_then(f)
                        .or_else(|| {
                            tag(st, "DURATION").and_then(|d| {
                                super::subs::parse_time(&d).map(|ms| ms as f64 / 1000.0)
                            })
                        })
                        .filter(|d| *d > 0.0),
                    default: disposition(st, "default"),
                    pix_fmt,
                    codec,
                });
            }
            Some("audio") => info.audio.push(AudioStream {
                index,
                profile: st.get("profile").and_then(s),
                channels: st.get("channels").and_then(u).unwrap_or(2) as u32,
                layout: st.get("channel_layout").and_then(s),
                sample_rate: st.get("sample_rate").and_then(u).map(|r| r as u32),
                language,
                language_tag: language_tag.filter(|_| language.is_none()),
                title: tag(st, "title"),
                default: disposition(st, "default"),
                codec,
            }),
            Some("subtitle") => {
                let (kind, extract_as) = subtitle_kind(&codec);
                info.subtitles.push(SubtitleStream {
                    index,
                    kind,
                    extract_as,
                    language,
                    language_tag: language_tag.filter(|_| language.is_none()),
                    title: tag(st, "title"),
                    default: disposition(st, "default"),
                    forced: disposition(st, "forced"),
                    hearing_impaired: disposition(st, "hearing_impaired"),
                    events: events(st),
                    codec,
                });
            }
            _ => {}
        }
    }
    unforce_full_tracks(&mut info.subtitles);
    // The player's length is the picture's: a file whose subtitles or audio
    // run on after the video ends would otherwise seek into nothing.
    if let Some(vd) = info.video.first().and_then(|v| v.duration) {
        info.duration = Some(info.duration.map_or(vd, |d| d.min(vd)));
    }
    for ch in json
        .get("chapters")
        .and_then(Value::as_array)
        .unwrap_or(&empty)
    {
        let (Some(start), Some(end)) = (
            ch.get("start_time").and_then(f),
            ch.get("end_time").and_then(f),
        ) else {
            continue;
        };
        info.chapters.push(Chapter {
            start: (start - info.start_time).max(0.0),
            end: (end - info.start_time).max(0.0),
            title: tag(ch, "title"),
        });
    }
    Some(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_streams_languages_hdr_and_chapters() {
        let j = json!({
            "format": {"format_name": "matroska,webm", "duration": "60.000", "start_time": "0.000", "size": "1000", "bit_rate": "5000"},
            "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "hevc", "profile": "Main 10", "pix_fmt": "yuv420p10le",
                 "width": 3840, "height": 2160, "avg_frame_rate": "24000/1001", "color_transfer": "smpte2084",
                 "disposition": {"default": 1}},
                {"index": 1, "codec_type": "video", "codec_name": "mjpeg", "disposition": {"attached_pic": 1}},
                {"index": 2, "codec_type": "audio", "codec_name": "ac3", "channels": 6, "tags": {"language": "eng", "title": "Surround"}},
                {"index": 3, "codec_type": "audio", "codec_name": "aac", "profile": "LC", "channels": 2, "tags": {"LANGUAGE": "per"}},
                {"index": 4, "codec_type": "subtitle", "codec_name": "subrip", "tags": {"language": "fas"}, "disposition": {"forced": 1}},
                {"index": 5, "codec_type": "subtitle", "codec_name": "hdmv_pgs_subtitle", "tags": {"language": "xx1"}},
                {"index": 6, "codec_type": "video", "codec_name": "h264", "width": 720, "height": 576, "sample_aspect_ratio": "64:45", "field_order": "tt", "pix_fmt": "yuv420p", "tags": {"DURATION": "00:00:20.000000000"}}
            ],
            "chapters": [{"start_time": "0.000", "end_time": "20.000", "tags": {"title": "Opening"}}]
        });
        let info = parse(&j).unwrap();
        assert_eq!(info.video.len(), 2, "cover art is not a video");
        assert_eq!(info.video[1].duration, Some(20.0));
        let v = &info.video[0];
        assert_eq!((v.bit_depth, v.hdr), (10, Some(Hdr::Pq)));
        assert!((v.fps.unwrap() - 23.976).abs() < 0.01);
        assert_eq!(info.video[1].sar, (64, 45));
        assert!(info.video[1].interlaced);
        assert_eq!(info.audio[0].language, Some("en"));
        assert_eq!(info.audio[1].language, Some("fa"));
        assert_eq!(info.subtitles[0].language, Some("fa"));
        assert!(info.subtitles[0].forced);
        assert_eq!(info.subtitles[1].kind, SubtitleKind::Image);
        assert_eq!(info.subtitles[1].language_tag.as_deref(), Some("xx1"));
        assert_eq!(info.chapters[0].title.as_deref(), Some("Opening"));
    }

    #[test]
    fn a_full_track_flagged_forced_is_not_taken_for_signs() {
        // As the ValaMovie / PSA releases ship them: the only subtitle,
        // default and forced, 760 events (mkvmerge's statistics).
        let sub = |index: u64, lang: &str, forced: u64, events: Option<&str>| {
            let mut tags = json!({"language": lang, "title": "ValaMovie.Com"});
            if let Some(e) = events {
                tags["NUMBER_OF_FRAMES"] = json!(e);
            }
            json!({"index": index, "codec_type": "subtitle", "codec_name": "subrip",
                   "tags": tags, "disposition": {"default": 1, "forced": forced}})
        };
        let one = |streams: Vec<Value>| {
            parse(&json!({"format": {"format_name": "matroska,webm"}, "streams": streams})).unwrap()
        };
        let i = one(vec![sub(2, "per", 1, Some("760"))]);
        let s = &i.subtitles[0];
        assert_eq!(
            (s.language, s.default, s.forced, s.events),
            (Some("fa"), true, false, Some(760))
        );
        assert_eq!(s.title.as_deref(), Some("ValaMovie.Com"));
        // A real signs track: few events.
        assert!(one(vec![sub(2, "per", 1, Some("40"))]).subtitles[0].forced);
        // No statistics: the flag stands.
        assert!(one(vec![sub(2, "per", 1, None)]).subtitles[0].forced);
        // A forced track beside a full one of its language stays forced, even a long one.
        let two = one(vec![
            sub(2, "eng", 1, Some("600")),
            sub(3, "eng", 0, Some("1400")),
        ]);
        assert!(two.subtitles[0].forced && !two.subtitles[1].forced);
        // Another language beside it does not count; the suffixed tag name is read too.
        let mut s3 = sub(2, "fas", 1, None);
        s3["tags"]["NUMBER_OF_FRAMES-eng"] = json!("500");
        let mixed = one(vec![s3, sub(3, "eng", 0, Some("900"))]);
        assert!(!mixed.subtitles[0].forced);
        assert_eq!(mixed.subtitles[0].events, Some(500));
    }

    #[test]
    fn no_subtitles_and_unknown_tags() {
        let i = parse(&json!({
            "format": {"format_name": "mov,mp4,m4a,3gp,3g2,mj2"},
            "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "h264", "width": 1, "height": 1},
                {"index": 1, "codec_type": "subtitle", "codec_name": "mov_text", "tags": {"language": "und"}},
                {"index": 2, "codec_type": "subtitle", "codec_name": "dvd_subtitle", "tags": {"language": "ger"}},
                {"index": 3, "codec_type": "subtitle", "tags": {"language": "qaa"}}
            ]
        }))
        .unwrap();
        let s = &i.subtitles;
        assert_eq!(
            (s[0].kind, s[0].extract_as, s[0].language),
            (SubtitleKind::Text, Some(Format::Srt), None)
        );
        assert_eq!(s[0].language_tag.as_deref(), Some("und"));
        assert_eq!(
            (s[1].kind, s[1].language),
            (SubtitleKind::Image, Some("de"))
        );
        assert_eq!(
            (s[2].codec.as_str(), s[2].kind),
            ("unknown", SubtitleKind::Unknown)
        );
        assert_eq!(s[2].language_tag.as_deref(), Some("qaa"));
        let none =
            parse(&json!({"format": {"format_name": "matroska,webm"}, "streams": []})).unwrap();
        assert!(none.subtitles.is_empty());
    }
}
