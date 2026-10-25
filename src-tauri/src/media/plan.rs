//! How a file reaches the webview, decided per file, the same on macOS
//! (WKWebView) and Windows (WebView2): the one stream both play natively is
//! H.264 (8-bit 4:2:0) with AAC in MP4, so every file becomes that.
//!
//! - `Direct`: already that, in an MP4 with one audio track: the file is
//!   served as it is, with byte ranges (instant seeking).
//! - `Remux`: H.264 and AAC in another container (MKV, TS, AVI...), or an
//!   audio track other than the first, copied into fragmented MP4.
//! - `AudioTranscode`: H.264 with AC-3, E-AC-3, DTS, TrueHD, FLAC, Opus,
//!   Vorbis, MP3 or PCM audio: the video copied, the audio to AAC.
//! - `VideoTranscode`: HEVC, VP9, AV1, MPEG-2, MPEG-4 Part 2, ProRes, VC-1,
//!   10-bit or 4:2:2 H.264, interlaced video: to H.264 with the hardware
//!   encoder (`tools::Encoder`), tone-mapped to SDR when it is HDR.
//!
//! HEVC, VP9 and AV1 are transcoded even where one webview could play them
//! (Safari plays HEVC; Chromium plays VP9 and AV1): the owner's rule is that
//! both platforms behave the same, and the transcode is fast (hardware).
//!
//! The streamed modes run FFmpeg from a start time, `-copyts` so the output
//! keeps the file's own timeline, plus `TIMELINE_OFFSET` so nothing is ever
//! negative (B-frames, AAC priming); a seek starts a new run at the new
//! time, and every run lands on the same timeline.

use serde::Serialize;

use super::probe::{AudioStream, Hdr, MediaInfo, VideoStream};
use super::tools::{Caps, Encoder};

/// Seconds added to every streamed timestamp. The first frames of a file
/// have negative decode times (B-frames) and AAC's priming starts before
/// zero; fragmented MP4 cannot hold a negative time, and letting FFmpeg
/// shift each run by what it needs would put runs on different timelines.
/// One fixed second is more than any of these needs.
pub const TIMELINE_OFFSET: f64 = 1.0;

/// Fragment length, in microseconds: how much video must be ready before
/// the page gets any (the seek latency's floor) and how often the
/// transcoder starts a new key frame.
const FRAGMENT_US: u64 = 1_000_000;
/// A key frame every this many seconds in transcoded video.
const KEYFRAME_SECONDS: f64 = 2.0;
/// The biggest frame the transcoder makes: 4K UHD, which both webviews'
/// H.264 decoders play and the hardware encoders make in real time.
const MAX_SDR: (u32, u32) = (3840, 2160);
/// Tone-mapping runs on the CPU (zscale), on both platforms: kept to 1080p
/// so it stays real time on a modest laptop (it is ~200 fps on an M-series
/// Mac, ~60 fps at 4K).
const MAX_HDR: (u32, u32) = (1920, 1080);
/// Bits per pixel per frame for transcoded H.264: generous, since it is
/// local and is watched at full screen (about 7.5 Mb/s at 1080p30).
const BITS_PER_PIXEL: f64 = 0.12;
/// AAC bit rate for transcoded audio, stereo.
const AAC_BITRATE: &str = "192k";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Direct,
    Remux,
    AudioTranscode,
    VideoTranscode,
}

/// What the player asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// The audio stream's index in the file; `None` is the default track.
    pub audio: Option<usize>,
    /// Milliseconds the audio is moved: positive plays it later.
    pub audio_delay_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoPlan {
    pub index: usize,
    pub copy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoder: Option<Encoder>,
    pub width: u32,
    pub height: u32,
    pub tonemap: bool,
    pub deinterlace: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioPlan {
    pub index: usize,
    pub copy: bool,
    pub delay_ms: i64,
    /// Down-mixed to stereo (from more channels).
    pub downmix: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub mode: Mode,
    pub video: VideoPlan,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioPlan>,
    /// For `MediaSource.isTypeSupported` and `addSourceBuffer`.
    pub mime: String,
    /// Seconds to add to the file's time to get the `<video>` element's.
    pub time_offset: f64,
    /// Why this mode, in plain words, for the developer page and logs.
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "reason")]
pub enum Unplayable {
    /// No video stream at all (an audio file, or a subtitle file).
    NoVideo,
    /// Video that needs transcoding and no H.264 encoder works here.
    NoEncoder,
}

fn is_mp4(info: &MediaInfo, ext: &str) -> bool {
    info.container.starts_with("mov,mp4")
        && matches!(ext, "mp4" | "m4v")
        && info.brand.as_deref().map(str::trim) != Some("qt")
}

/// H.264 that both webviews decode: 8-bit 4:2:0, a profile they support,
/// progressive, no bigger than 4K-ish.
fn h264_plays(v: &VideoStream) -> Result<(), String> {
    if v.codec != "h264" {
        return Err(format!("{} video", v.codec));
    }
    let profile = v.profile.as_deref().unwrap_or("");
    if !matches!(
        profile,
        "Baseline" | "Constrained Baseline" | "Main" | "High" | "Extended" | ""
    ) {
        return Err(format!("H.264 {profile}"));
    }
    if v.bit_depth > 8
        || v.pix_fmt
            .as_deref()
            .is_some_and(|p| p != "yuv420p" && p != "yuvj420p")
    {
        return Err(format!(
            "H.264 {} {}-bit",
            v.pix_fmt.as_deref().unwrap_or("?"),
            v.bit_depth
        ));
    }
    if v.interlaced {
        return Err("interlaced H.264".into());
    }
    if v.width > 4096 || v.height > 2304 {
        return Err(format!("H.264 {}x{}", v.width, v.height));
    }
    Ok(())
}

fn aac_plays(a: &AudioStream) -> bool {
    a.codec == "aac"
        && matches!(
            a.profile.as_deref(),
            None | Some("LC") | Some("HE-AAC") | Some("HE-AACv2")
        )
        && a.channels <= 8
}

fn aac_codec(a: &AudioStream) -> &'static str {
    match a.profile.as_deref() {
        Some("HE-AAC") => "mp4a.40.5",
        Some("HE-AACv2") => "mp4a.40.29",
        _ => "mp4a.40.2",
    }
}

/// `avc1.PPCCLL` for a copied H.264 stream.
fn avc_codec(v: &VideoStream) -> String {
    let (profile, constraints) = match v.profile.as_deref() {
        Some("Baseline") => (0x42, 0x00),
        Some("Constrained Baseline") => (0x42, 0x40),
        Some("Main") => (0x4d, 0x40),
        Some("Extended") => (0x58, 0x00),
        _ => (0x64, 0x00),
    };
    let level = v.level.filter(|l| (9..=62).contains(l)).unwrap_or(40);
    format!("avc1.{profile:02x}{constraints:02x}{level:02x}")
}

/// The default audio track: the one flagged default, else the first.
pub fn default_audio(info: &MediaInfo) -> Option<&AudioStream> {
    info.audio
        .iter()
        .find(|a| a.default)
        .or_else(|| info.audio.first())
}

fn even(n: f64) -> u32 {
    ((n / 2.0).round() as u32 * 2).max(2)
}

/// The output size: square pixels, inside `max`, even.
fn out_size(v: &VideoStream, max: (u32, u32)) -> (u32, u32) {
    let (w, h) = (v.width.max(2) as f64, v.height.max(2) as f64);
    let w = w * v.sar.0 as f64 / v.sar.1 as f64;
    let scale = (max.0 as f64 / w).min(max.1 as f64 / h).min(1.0);
    (even(w * scale), even(h * scale))
}

pub fn decide(info: &MediaInfo, ext: &str, opts: Options, caps: &Caps) -> Result<Plan, Unplayable> {
    let video = info
        .video
        .iter()
        .find(|v| v.default)
        .or_else(|| info.video.first())
        .ok_or(Unplayable::NoVideo)?;
    let audio = match opts.audio {
        Some(i) => info
            .audio
            .iter()
            .find(|a| a.index == i)
            .or_else(|| default_audio(info)),
        None => default_audio(info),
    };
    let mut reasons = Vec::new();

    let video_copy = match h264_plays(video) {
        Ok(()) => true,
        Err(why) => {
            reasons.push(format!("video: {why} is transcoded to H.264"));
            false
        }
    };
    let audio_copy = audio.is_none_or(|a| {
        let ok = aac_plays(a) && opts.audio_delay_ms == 0;
        if !aac_plays(a) {
            reasons.push(format!("audio: {} is transcoded to AAC", a.codec));
        } else if opts.audio_delay_ms != 0 {
            reasons.push("audio: moved in time, so re-encoded".into());
        }
        ok
    });
    let first_audio = audio.is_none_or(|a| info.audio.first().map(|f| f.index) == Some(a.index));
    let direct =
        video_copy && audio_copy && is_mp4(info, ext) && info.audio.len() <= 1 && first_audio;
    if video_copy && audio_copy && !direct {
        reasons.push(if !is_mp4(info, ext) {
            format!("container: {} is copied into MP4", info.container)
        } else {
            "audio: a track other than the first, so remuxed".into()
        });
    }
    let mode = if direct {
        Mode::Direct
    } else if !video_copy {
        Mode::VideoTranscode
    } else if !audio_copy {
        Mode::AudioTranscode
    } else {
        Mode::Remux
    };

    let tonemap = !video_copy && matches!(video.hdr, Some(Hdr::Pq | Hdr::Hlg)) && caps.zscale;
    if !video_copy {
        match video.hdr {
            Some(Hdr::Pq | Hdr::Hlg) if tonemap => {
                reasons.push("HDR: tone-mapped to SDR (BT.709)".into())
            }
            Some(Hdr::Pq | Hdr::Hlg) => reasons
                .push("HDR: no tone-mapper in this FFmpeg; colours will look washed out".into()),
            Some(Hdr::DolbyVisionOnly) => reasons
                .push("Dolby Vision without an HDR10 base layer: colours will be wrong".into()),
            None => {}
        }
    }
    let encoder = if video_copy {
        None
    } else {
        Some(caps.h264.ok_or(Unplayable::NoEncoder)?)
    };
    let (width, height) = if video_copy {
        (video.width, video.height)
    } else {
        out_size(video, if tonemap { MAX_HDR } else { MAX_SDR })
    };
    let video_codec = if video_copy {
        avc_codec(video)
    } else {
        // High profile, level 5.1: what the encoders are asked for.
        "avc1.640033".into()
    };
    let audio_plan = audio.map(|a| AudioPlan {
        index: a.index,
        copy: audio_copy,
        delay_ms: opts.audio_delay_ms,
        downmix: !audio_copy && a.channels > 2,
    });
    let codecs = match (&audio_plan, audio) {
        (Some(p), Some(a)) if p.copy => format!("{video_codec},{}", aac_codec(a)),
        (Some(_), _) => format!("{video_codec},mp4a.40.2"),
        (None, _) => video_codec,
    };
    Ok(Plan {
        mode,
        video: VideoPlan {
            index: video.index,
            copy: video_copy,
            encoder,
            width,
            height,
            tonemap,
            deinterlace: !video_copy && video.interlaced,
        },
        audio: audio_plan,
        mime: format!("video/mp4; codecs=\"{codecs}\""),
        time_offset: if mode == Mode::Direct {
            0.0
        } else {
            TIMELINE_OFFSET
        },
        reasons,
    })
}

/// The video filter chain for a transcode.
fn video_filters(plan: &VideoPlan, src: &VideoStream, encoder: Encoder) -> String {
    let mut f: Vec<String> = Vec::new();
    if plan.deinterlace {
        f.push("bwdif=mode=send_frame:deint=interlaced".into());
    }
    if (plan.width, plan.height) != (src.width, src.height) || src.sar != (1, 1) {
        f.push(format!(
            "scale={}:{}:flags=bicubic",
            plan.width, plan.height
        ));
        f.push("setsar=1".into());
    }
    if plan.tonemap {
        // Linear light, BT.709 primaries, Hable's curve, back to BT.709.
        let tin = if src.hdr == Some(Hdr::Hlg) {
            "arib-std-b67"
        } else {
            "smpte2084"
        };
        f.push(format!("zscale=tin={tin}:t=linear:npl=100"));
        f.push("format=gbrpf32le".into());
        f.push("zscale=p=bt709".into());
        f.push("tonemap=tonemap=hable:desat=0".into());
        f.push("zscale=t=bt709:m=bt709:r=tv".into());
    }
    f.push(format!("format={}", encoder.pix_fmt()));
    f.join(",")
}

/// The FFmpeg arguments for a streamed run of `plan` from `start` seconds
/// (the file's own timeline), writing fragmented MP4 to stdout.
pub fn args(plan: &Plan, info: &MediaInfo, path: &str, start: f64, caps: &Caps) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-nostdin", "-nostats", "-loglevel", "error"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let push = |a: &mut Vec<String>, xs: &[&str]| a.extend(xs.iter().map(|s| s.to_string()));
    if !plan.video.copy {
        if let Some(hw) = caps.hwaccel {
            push(&mut a, &["-hwaccel", hw]);
        }
    }
    if info.container == "avi" {
        push(&mut a, &["-fflags", "+genpts"]);
    }
    if start > 0.0 {
        push(&mut a, &["-ss", &format!("{start:.3}")]);
    }
    push(&mut a, &["-copyts", "-start_at_zero", "-i", path]);
    push(&mut a, &["-map", &format!("0:{}", plan.video.index)]);
    if let Some(au) = &plan.audio {
        push(&mut a, &["-map", &format!("0:{}", au.index)]);
    }
    push(
        &mut a,
        &["-sn", "-dn", "-map_metadata", "-1", "-map_chapters", "-1"],
    );

    let src = info.video.iter().find(|v| v.index == plan.video.index);
    match (plan.video.copy, plan.video.encoder, src) {
        (false, Some(enc), Some(src)) => {
            push(&mut a, &["-vf", &video_filters(&plan.video, src, enc)]);
            push(&mut a, &["-c:v", enc.ffmpeg_name()]);
            let fps = src.fps.unwrap_or(30.0).clamp(1.0, 120.0);
            let bitrate =
                (plan.video.width as f64 * plan.video.height as f64 * fps * BITS_PER_PIXEL)
                    .clamp(1.5e6, 60e6) as u64;
            let gop = (fps * KEYFRAME_SECONDS).round().max(1.0) as u64;
            push(
                &mut a,
                &[
                    "-b:v",
                    &bitrate.to_string(),
                    "-maxrate",
                    &(bitrate * 3 / 2).to_string(),
                    "-bufsize",
                    &(bitrate * 2).to_string(),
                    "-g",
                    &gop.to_string(),
                ],
            );
            a.extend(enc.options());
            push(
                &mut a,
                &[
                    "-color_primaries",
                    "bt709",
                    "-color_trc",
                    "bt709",
                    "-colorspace",
                    "bt709",
                ],
            );
        }
        _ => push(&mut a, &["-c:v", "copy"]),
    }
    if let Some(au) = &plan.audio {
        if au.copy {
            push(&mut a, &["-c:a", "copy"]);
            // AAC in MPEG-TS, FLV or a raw .aac file is ADTS-framed; MP4
            // needs it without the ADTS headers.
            if ["mpegts", "flv", "aac", "hls"]
                .iter()
                .any(|c| info.container.split(',').any(|x| x == *c))
            {
                push(&mut a, &["-bsf:a", "aac_adtstoasc"]);
            }
        } else {
            let rate = 48_000u32;
            // aresample's first_pts counts samples at the INPUT's rate (a
            // 44.1 kHz source measured at 48 kHz put a seek's audio 9% late:
            // 13 s at 2:28 in Tears of Steel).
            let in_rate = info
                .audio
                .iter()
                .find(|x| x.index == au.index)
                .and_then(|x| x.sample_rate)
                .filter(|r| *r > 0)
                .unwrap_or(rate);
            // Delay: move the samples, then pad or trim so the run's audio
            // starts exactly at its start (no gap for the player to stall on).
            let mut af = Vec::new();
            if au.delay_ms != 0 {
                af.push(format!(
                    "asetpts=PTS+({:.3})/TB",
                    au.delay_ms as f64 / 1000.0
                ));
            }
            af.push(format!(
                "aresample={rate}:async=1:first_pts={}",
                (start.max(0.0) * in_rate as f64).round() as i64
            ));
            push(&mut a, &["-af", &af.join(",")]);
            push(
                &mut a,
                &["-c:a", "aac", "-b:a", AAC_BITRATE, "-ar", &rate.to_string()],
            );
            if au.downmix {
                push(&mut a, &["-ac", "2"]);
            }
        }
    }
    push(
        &mut a,
        &[
            "-max_muxing_queue_size",
            "4096",
            "-f",
            "mp4",
            "-avoid_negative_ts",
            "disabled",
            "-output_ts_offset",
            &format!("{TIMELINE_OFFSET:.3}"),
            "-movflags",
            "frag_keyframe+empty_moov+default_base_moof+frag_discont",
            "-frag_duration",
            &FRAGMENT_US.to_string(),
            "pipe:1",
        ],
    );
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::probe;
    use serde_json::json;

    fn caps() -> Caps {
        Caps {
            h264: Some(Encoder::VideoToolbox),
            hwaccel: Some("videotoolbox"),
            zscale: true,
        }
    }

    fn info(container: &str, streams: serde_json::Value) -> MediaInfo {
        probe::parse(
            &json!({"format": {"format_name": container, "duration": "60"}, "streams": streams}),
        )
        .unwrap()
    }

    fn h264() -> serde_json::Value {
        json!({"index": 0, "codec_type": "video", "codec_name": "h264", "profile": "High", "level": 40, "pix_fmt": "yuv420p", "width": 1920, "height": 1080, "avg_frame_rate": "30/1"})
    }

    fn aac(i: usize) -> serde_json::Value {
        json!({"index": i, "codec_type": "audio", "codec_name": "aac", "profile": "LC", "channels": 2})
    }

    #[test]
    fn mp4_h264_aac_plays_directly() {
        let i = info("mov,mp4,m4a,3gp,3g2,mj2", json!([h264(), aac(1)]));
        let p = decide(&i, "mp4", Options::default(), &caps()).unwrap();
        assert_eq!(p.mode, Mode::Direct);
        assert_eq!(p.mime, "video/mp4; codecs=\"avc1.640028,mp4a.40.2\"");
        assert_eq!(p.time_offset, 0.0);
    }

    #[test]
    fn mkv_or_a_second_track_or_a_delay_is_streamed() {
        let i = info("matroska,webm", json!([h264(), aac(1)]));
        assert_eq!(
            decide(&i, "mkv", Options::default(), &caps()).unwrap().mode,
            Mode::Remux
        );
        let i = info("mov,mp4,m4a,3gp,3g2,mj2", json!([h264(), aac(1), aac(2)]));
        let p = decide(
            &i,
            "mp4",
            Options {
                audio: Some(2),
                audio_delay_ms: 0,
            },
            &caps(),
        )
        .unwrap();
        assert_eq!(p.mode, Mode::Remux);
        assert_eq!(p.audio.unwrap().index, 2);
        let i = info("mov,mp4,m4a,3gp,3g2,mj2", json!([h264(), aac(1)]));
        let p = decide(
            &i,
            "mp4",
            Options {
                audio: None,
                audio_delay_ms: 250,
            },
            &caps(),
        )
        .unwrap();
        assert_eq!(p.mode, Mode::AudioTranscode);
        assert_eq!(p.time_offset, TIMELINE_OFFSET);
    }

    #[test]
    fn ac3_is_transcoded_and_downmixed() {
        let i = info(
            "matroska,webm",
            json!([h264(), {"index": 1, "codec_type": "audio", "codec_name": "ac3", "channels": 6, "sample_rate": "48000"}]),
        );
        let p = decide(&i, "mkv", Options::default(), &caps()).unwrap();
        assert_eq!(p.mode, Mode::AudioTranscode);
        assert!(p.audio.as_ref().unwrap().downmix);
        let a = args(&p, &i, "/x.mkv", 12.5, &caps()).join(" ");
        assert!(a.contains("-ss 12.500 -copyts -start_at_zero -i /x.mkv"));
        assert!(a.contains("-c:v copy"));
        assert!(a.contains("-c:a aac"));
        assert!(a.contains("first_pts=600000"));
        assert!(a.contains("-ac 2"));
        assert!(!a.contains("-hwaccel"));
    }

    #[test]
    fn a_seek_s_audio_is_counted_at_the_source_rate() {
        let i = info(
            "mov,mp4,m4a,3gp,3g2,mj2",
            json!([h264(), {"index": 1, "codec_type": "audio", "codec_name": "mp3", "channels": 2, "sample_rate": "44100"}]),
        );
        let p = decide(&i, "mov", Options::default(), &caps()).unwrap();
        let a = args(&p, &i, "/x.mov", 147.833, &caps()).join(" ");
        assert!(
            a.contains("aresample=48000:async=1:first_pts=6519435"),
            "{a}"
        );
    }

    #[test]
    fn aac_from_mpeg_ts_loses_its_adts_headers() {
        let i = info("mpegts", json!([h264(), aac(1)]));
        let p = decide(&i, "ts", Options::default(), &caps()).unwrap();
        assert_eq!(p.mode, Mode::Remux);
        assert!(args(&p, &i, "/x.ts", 0.0, &caps())
            .join(" ")
            .contains("-bsf:a aac_adtstoasc"));
    }

    #[test]
    fn hevc_hdr_is_transcoded_tone_mapped_and_capped_at_1080p() {
        let i = info(
            "matroska,webm",
            json!([{"index": 0, "codec_type": "video", "codec_name": "hevc", "profile": "Main 10", "pix_fmt": "yuv420p10le",
                    "width": 3840, "height": 2160, "avg_frame_rate": "24/1", "color_transfer": "smpte2084"}, aac(1)]),
        );
        let p = decide(&i, "mkv", Options::default(), &caps()).unwrap();
        assert_eq!(p.mode, Mode::VideoTranscode);
        assert!(p.video.tonemap);
        assert_eq!((p.video.width, p.video.height), (1920, 1080));
        let a = args(&p, &i, "/x.mkv", 0.0, &caps()).join(" ");
        assert!(a.contains("-hwaccel videotoolbox"));
        assert!(a.contains("tonemap=tonemap=hable"));
        assert!(a.contains("-c:v h264_videotoolbox"));
        assert!(!a.contains("-ss"));
    }

    #[test]
    fn anamorphic_and_interlaced_video() {
        let i = info(
            "mpeg",
            json!([{"index": 0, "codec_type": "video", "codec_name": "mpeg2video", "pix_fmt": "yuv420p",
                    "width": 720, "height": 576, "sample_aspect_ratio": "64:45", "field_order": "tt", "avg_frame_rate": "25/1"}]),
        );
        let p = decide(&i, "mpg", Options::default(), &caps()).unwrap();
        assert_eq!((p.video.width, p.video.height), (1024, 576));
        assert!(p.video.deinterlace);
        assert!(p.audio.is_none());
        assert_eq!(p.mime, "video/mp4; codecs=\"avc1.640033\"");
    }

    #[test]
    fn no_encoder_or_no_video() {
        let i = info(
            "matroska,webm",
            json!([{"index": 0, "codec_type": "video", "codec_name": "vp9", "width": 640, "height": 360}]),
        );
        let none = Caps {
            h264: None,
            hwaccel: None,
            zscale: false,
        };
        assert_eq!(
            decide(&i, "mkv", Options::default(), &none).unwrap_err(),
            Unplayable::NoEncoder
        );
        let i = info(
            "mp3",
            json!([{"index": 0, "codec_type": "audio", "codec_name": "mp3", "channels": 2}]),
        );
        assert_eq!(
            decide(&i, "mp3", Options::default(), &caps()).unwrap_err(),
            Unplayable::NoVideo
        );
    }
}
