//! The engine against the real bundled FFmpeg (src-tauri/binaries/, as on
//! this Mac and in CI's Linux job): makes small clips with FFmpeg's own test
//! patterns, probes them, plans them, streams a run and checks it is
//! fragmented MP4 on the file's timeline, extracts an embedded Windows-1256
//! subtitle and seeks. Skipped (with a note) where the sidecars are absent.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::plan::{Mode, Options};
use super::stream::{Pulled, Session};
use super::subs::Format;
use super::tools::{self, Tools};
use super::{load_track, probe};

fn tools_or_skip() -> Option<Tools> {
    match Tools::locate() {
        Ok(t) => Some(t),
        Err(_) => {
            eprintln!(
                "media engine tests skipped: no FFmpeg in src-tauri/binaries (scripts/ffmpeg/)"
            );
            None
        }
    }
}

async fn make(tools: &Tools, dir: &Path, name: &str, args: &[&str]) -> PathBuf {
    let out = dir.join(name);
    let mut a: Vec<&str> = vec!["-hide_banner", "-loglevel", "error", "-y"];
    a.extend_from_slice(args);
    let o = out.to_str().unwrap().to_owned();
    a.push(&o);
    tools::output(&tools.ffmpeg, &a, Duration::from_secs(60))
        .await
        .unwrap_or_else(|e| panic!("making {name}: {e}"));
    out
}

/// The `tfdt` (decode time) of the first fragment of track 1, in seconds,
/// and whether the bytes begin with an init segment.
fn first_fragment(bytes: &[u8]) -> (bool, Option<f64>) {
    let starts_init = bytes.len() > 8 && &bytes[4..8] == b"ftyp";
    let mut timescale = None;
    let find = |needle: &[u8]| bytes.windows(4).position(|w| w == needle);
    if let Some(i) = find(b"mdhd") {
        let v = bytes[i + 4];
        let at = i + 8 + if v == 1 { 16 } else { 8 };
        timescale = Some(u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as f64);
    }
    let tfdt = find(b"tfdt").map(|i| {
        let v = bytes[i + 4];
        if v == 1 {
            u64::from_be_bytes(bytes[i + 8..i + 16].try_into().unwrap()) as f64
        } else {
            u32::from_be_bytes(bytes[i + 8..i + 12].try_into().unwrap()) as f64
        }
    });
    (starts_init, tfdt.zip(timescale).map(|(t, s)| t / s))
}

async fn pull_some(run: &super::stream::Run, want: usize) -> Vec<u8> {
    let mut got = Vec::new();
    let mut from = 0;
    while got.len() < want {
        match run.pull(from, 1 << 20).await {
            Pulled::Bytes { data, next, end } => {
                got.extend(data);
                from = next;
                if end {
                    break;
                }
            }
            Pulled::Wait { next } => from = next,
            Pulled::End { .. } => break,
            Pulled::Failed(e) => panic!("run failed: {e}"),
            Pulled::Gone => panic!("run gone"),
        }
    }
    got
}

#[tokio::test(flavor = "multi_thread")]
async fn streams_extracts_and_seeks_with_the_real_ffmpeg() {
    let Some(tools) = tools_or_skip() else {
        return;
    };
    let caps = tools::caps(&tools).await;
    let dir = std::env::temp_dir().join(format!("lexpad-engine-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let srt = "1\r\n00:00:01,000 --> 00:00:03,000\r\nكليدهايم را روي ميز آشپزخانه گذاشتم.\r\n\r\n2\r\n00:00:04,000 --> 00:00:06,000\r\nقطار شهر ساعت نه حركت مي‌كند. چطور است؟\r\n";
    let (bytes, _, _) = encoding_rs::WINDOWS_1256.encode(srt);
    std::fs::write(dir.join("fa.srt"), &bytes).unwrap();
    // MPEG-4 Part 2 video (FFmpeg's own encoder, in every build) with AC-3
    // audio and the Persian subtitle, in Matroska: a video and an audio
    // transcode, and an embedded old-encoding track.
    let mkv = make(
        &tools,
        &dir,
        "clip.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=25:duration=12",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=12",
            "-i",
            dir.join("fa.srt").to_str().unwrap(),
            "-map",
            "0",
            "-map",
            "1",
            "-map",
            "2",
            "-c:v",
            "mpeg4",
            "-g",
            "25",
            "-c:a",
            "ac3",
            "-c:s",
            "copy",
            "-metadata:s:s:0",
            "language=per",
        ],
    )
    .await;

    let json = tools::probe_json(&tools, &mkv).await.unwrap();
    let info = Arc::new(probe::parse(&json).unwrap());
    assert_eq!(info.video[0].codec, "mpeg4");
    assert_eq!(info.audio[0].codec, "ac3");
    assert_eq!(info.subtitles[0].language, Some("fa"));

    // The embedded subtitle: raw bytes, decoded, Persian, letters fixed.
    let st = &info.subtitles[0];
    let state = super::MediaState::new(None);
    let raw = state.extract(&mkv, &info, st, &|_| {}).await.unwrap();
    let track = load_track("embedded:2".into(), &raw, st.extract_as, None, st.language).unwrap();
    assert_eq!(track.encoding.encoding, "windows-1256");
    assert_eq!(track.parsed.format, Format::Srt);
    assert_eq!(track.language.best.as_ref().unwrap().code, "fa");
    assert!(track.persian_letters_fixed);
    assert_eq!(track.parsed.cues.len(), 2);

    if caps.h264.is_none() {
        eprintln!("no H.264 encoder works here: the transcode part is skipped");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    // A frame for the seek bar, and progress reported while a run converts.
    let jpeg = state
        .thumbs
        .frame(&tools, "f", &mkv, &info, 5)
        .await
        .unwrap();
    assert_eq!(&jpeg[..2], &[0xff, 0xd8], "a JPEG");
    let seen = Arc::new(std::sync::Mutex::new(Vec::<f64>::new()));
    let session = Session::new(
        "s".into(),
        "f".into(),
        mkv.clone(),
        info.clone(),
        tools.clone(),
        caps,
        Options::default(),
    )
    .unwrap()
    .with_progress(Arc::new({
        let seen = seen.clone();
        move |_, ready, _| seen.lock().unwrap().push(ready)
    }));
    assert_eq!(session.plan().mode, Mode::VideoTranscode);
    let run = session.start_run(0.0).unwrap();
    let bytes = pull_some(&run, 64 * 1024).await;
    let (init, t) = first_fragment(&bytes);
    assert!(init, "a run begins with an init segment");
    let t = t.expect("a fragment");
    assert!(
        (t - 1.0).abs() < 0.2,
        "first fragment at the timeline offset, got {t}"
    );

    // A seek: a new run, on the same timeline.
    let run2 = session.start_run(8.0).unwrap();
    assert!(session.run(run.id).is_none(), "the old run is replaced");
    let bytes = pull_some(&run2, 32 * 1024).await;
    let (_, t2) = first_fragment(&bytes);
    let t2 = t2.expect("a fragment");
    assert!((t2 - 9.0).abs() < 0.3, "seek run at 8 s + offset, got {t2}");
    // Progress comes every half second while the run converts.
    for _ in 0..40 {
        if seen.lock().unwrap().iter().any(|r| *r >= 8.0) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let ready = seen.lock().unwrap().clone();
    assert!(
        ready.iter().any(|r| *r >= 8.0),
        "progress from the seek run: {ready:?}"
    );
    session.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The engine over real films (`LEXPAD_REAL_MEDIA`: a folder with `subs/`,
/// `bench/` and `series/`, as the branch's report describes): every
/// subtitle's encoding and language, every video's plan, the embedded
/// tracks' extraction, the series logic, and how fast a run starts.
///
///   LEXPAD_REAL_MEDIA=<folder> cargo test --lib real_media -- --ignored --nocapture
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs LEXPAD_REAL_MEDIA, a folder of real films"]
async fn real_media_report() {
    use std::time::Instant;
    let Ok(root) = std::env::var("LEXPAD_REAL_MEDIA") else {
        return;
    };
    let root = PathBuf::from(root);
    let tools = Tools::locate().expect("ffmpeg");
    let caps = tools::caps(&tools).await;
    let entries = |d: &str| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(root.join(d))
            .map(|r| r.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        v.sort();
        v
    };
    println!("\n== subtitle files");
    let mut subs = entries("subs");
    subs.extend(entries("series"));
    for p in subs
        .iter()
        .filter(|p| super::files::kind_of(p) == super::files::Kind::Subtitle)
    {
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(p).unwrap();
        let hint = super::subs::lang::from_file_name(&name);
        let t0 = Instant::now();
        match load_track(
            name.clone(),
            &bytes,
            Format::from_ext("srt"),
            hint.lang,
            None,
        ) {
            Ok(t) => {
                let best = t.language.best.as_ref();
                let text = t.language.text.as_ref();
                println!(
                    "{name:40} {:13} {:9?} cues {:4} best {} {:.2} ({:?}) text {} {:.2} name {:?}{} {:?}",
                    t.encoding.encoding,
                    t.encoding.how,
                    t.parsed.cues.len(),
                    best.map(|g| g.code).unwrap_or("-"),
                    best.map(|g| g.confidence).unwrap_or(0.0),
                    best.map(|g| g.source),
                    text.map(|g| g.code).unwrap_or("-"),
                    text.map(|g| g.confidence).unwrap_or(0.0),
                    hint.lang,
                    if t.persian_letters_fixed { " yeh/kaf fixed" } else { "" },
                    t0.elapsed()
                );
                println!(
                    "{:40} first line: {}",
                    "",
                    t.parsed
                        .cues
                        .iter()
                        .find(|c| c.text.chars().count() > 8)
                        .map(|c| c.text.replace('\n', " / "))
                        .unwrap_or_default()
                );
            }
            Err(e) => println!("{name:40} ERROR {e}"),
        }
    }
    println!("\n== videos");
    for p in entries("bench") {
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        let t0 = Instant::now();
        let json = tools::probe_json(&tools, &p).await.unwrap();
        let probe_ms = t0.elapsed().as_millis();
        let info = Arc::new(probe::parse(&json).unwrap());
        let ext = super::files::ext_of(&p);
        match super::plan::decide(&info, &ext, Options::default(), &caps) {
            Ok(plan) => {
                println!(
                    "{name:42} probe {probe_ms:4} ms  {:?}  {}  {:?}",
                    plan.mode, plan.mime, plan.reasons
                );
                for a in info.audio.iter().skip(1) {
                    let alt = super::plan::decide(
                        &info,
                        &ext,
                        Options {
                            audio: Some(a.index),
                            audio_delay_ms: 0,
                            ..Options::default()
                        },
                        &caps,
                    )
                    .unwrap();
                    println!(
                        "{:42} audio #{} ({}): {:?} {:?}",
                        "", a.index, a.codec, alt.mode, alt.reasons
                    );
                }
                if plan.mode != Mode::Direct {
                    let s = Session::new(
                        "r".into(),
                        "f".into(),
                        p.clone(),
                        info.clone(),
                        tools.clone(),
                        caps,
                        Options::default(),
                    )
                    .unwrap();
                    let d = info.duration.unwrap_or(30.0);
                    let mut times = Vec::new();
                    for at in [0.0, d * 0.5, d * 0.9] {
                        let t = Instant::now();
                        let run = s.start_run(at).unwrap();
                        let _ = pull_some(&run, 1).await;
                        times.push(t.elapsed().as_millis());
                    }
                    println!(
                        "{:42} run to first bytes at 0 / 50% / 90%: {:?} ms",
                        "", times
                    );
                    s.stop();
                }
            }
            Err(e) => println!("{name:42} UNPLAYABLE {e:?}"),
        }
        let state = super::MediaState::new(None);
        for st in info.subtitles.iter() {
            let t = Instant::now();
            match state.extract(&p, &info, st, &|_| {}).await {
                Ok(raw) => {
                    let tr = load_track(
                        format!("embedded:{}", st.index),
                        &raw,
                        st.extract_as,
                        None,
                        st.language,
                    );
                    match tr {
                        Ok(tr) => {
                            println!(
                            "{:42} embedded #{} tag {:?} -> {} {:.2} ({:?}), {} cues, {} in {} ms",
                            "",
                            st.index,
                            st.language,
                            tr.language.best.as_ref().map(|g| g.code).unwrap_or("-"),
                            tr.language.best.as_ref().map(|g| g.confidence).unwrap_or(0.0),
                            tr.language.best.as_ref().map(|g| g.source),
                            tr.parsed.cues.len(),
                            tr.encoding.encoding,
                            t.elapsed().as_millis()
                        )
                        }
                        Err(e) => println!("{:42} embedded #{} ERROR {e}", "", st.index),
                    }
                }
                Err(e) => println!("{:42} embedded #{} ERROR {e}", "", st.index),
            }
        }
    }
    println!("\n== series");
    let series = entries("series");
    let videos: Vec<PathBuf> = series
        .iter()
        .filter(|p| super::files::kind_of(p) == super::files::Kind::Video)
        .cloned()
        .collect();
    let order = super::series::episodes_of(&videos[1], &videos);
    println!(
        "order: {:?}",
        order
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    );
    for v in &order {
        let matched: Vec<String> = series
            .iter()
            .filter(|p| super::files::kind_of(p) == super::files::Kind::Subtitle)
            .filter_map(|s| {
                let fit = super::series::subtitle_fit(v, s);
                (fit > 0)
                    .then(|| format!("{} (fit {fit})", s.file_name().unwrap().to_string_lossy()))
            })
            .collect();
        println!(
            "{:36} <- {:?}",
            v.file_name().unwrap().to_string_lossy(),
            matched
        );
    }
}

/// The committed film with three embedded text tracks
/// (tests/fixtures/media/make-soft-subs.sh).
fn soft_subs_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/media/soft-subs.mkv")
}

/// Soft subtitles, as VLC shows them: every text track of an MKV found,
/// named by its tag, extracted in one pass without decoding the picture,
/// each in its own format and encoding, Persian found and right to left.
#[tokio::test(flavor = "multi_thread")]
async fn embedded_srt_and_ass_tracks_are_read_out_of_an_mkv() {
    let Some(tools) = tools_or_skip() else {
        return;
    };
    let mkv = soft_subs_fixture();
    let json = tools::probe_json(&tools, &mkv).await.unwrap();
    let info = probe::parse(&json).unwrap();
    let subs = &info.subtitles;
    assert_eq!(subs.len(), 3);
    assert_eq!(
        subs.iter()
            .map(|s| (s.index, s.codec.as_str(), s.language))
            .collect::<Vec<_>>(),
        vec![
            (2, "subrip", Some("fa")),
            (3, "ass", Some("en")),
            (4, "subrip", Some("fa")),
        ]
    );
    assert_eq!(subs[0].title.as_deref(), Some("ValaMovie.Com"));
    // Two Persian tracks: the forced flag of #2 stands (no statistics, and not alone).
    assert!(subs[0].default && subs[0].forced);
    assert!(subs[1].hearing_impaired);

    let state = super::MediaState::new(None);
    let seen = std::sync::Mutex::new(Vec::<f64>::new());
    let progress = |f: f64| seen.lock().unwrap().push(f);
    let fa = state
        .extract(&mkv, &info, &subs[0], &progress)
        .await
        .unwrap();
    // One pass extracted all three: the others come from memory now.
    assert_eq!(state.extracted.lock().unwrap().len(), 3);
    assert!(seen.lock().unwrap().iter().all(|f| (0.0..=1.0).contains(f)));

    let fa = load_track(
        "embedded:2".into(),
        &fa,
        subs[0].extract_as,
        None,
        subs[0].language,
    )
    .unwrap();
    assert_eq!(fa.parsed.format, Format::Srt);
    assert_eq!(fa.encoding.encoding, "UTF-8");
    assert_eq!(fa.language.best.as_ref().unwrap().code, "fa");
    assert_eq!(fa.parsed.cues.len(), 5);
    assert_eq!(fa.parsed.cues[0].text, "سلام، حالت چطوره؟");
    assert_eq!((fa.parsed.cues[0].start, fa.parsed.cues[0].end), (100, 900));

    let en = state.extract(&mkv, &info, &subs[1], &|_| {}).await.unwrap();
    let en = load_track(
        "embedded:3".into(),
        &en,
        subs[1].extract_as,
        None,
        subs[1].language,
    )
    .unwrap();
    assert_eq!(en.parsed.format, Format::Ass);
    assert_eq!(en.language.best.as_ref().unwrap().code, "en");
    assert_eq!(en.parsed.cues.len(), 5);
    assert_eq!(en.parsed.cues[1].text, "I'm outside, buddy.");

    let old = state.extract(&mkv, &info, &subs[2], &|_| {}).await.unwrap();
    let old = load_track(
        "embedded:4".into(),
        &old,
        subs[2].extract_as,
        None,
        subs[2].language,
    )
    .unwrap();
    assert_eq!(old.encoding.encoding, "windows-1256");
    assert!(old.persian_letters_fixed);
    assert_eq!(old.parsed.cues[0].text, "کلیدهایم را روی میز گذاشتم.");

    // A file that went away between the probe and the extraction.
    let gone = std::env::temp_dir().join("lexpad-no-such-film.mkv");
    assert_eq!(
        state
            .extract(&gone, &info, &subs[0], &|_| {})
            .await
            .unwrap_err(),
        "file_gone"
    );
}

/// A real film with soft subtitles (`LEXPAD_SOFTSUB_FILE`): its tracks,
/// how long reading them took, and the first five cues of each.
///
///   LEXPAD_SOFTSUB_FILE=<film.mkv> cargo test --lib soft_subs_of_a_real_film -- --ignored --nocapture
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs LEXPAD_SOFTSUB_FILE, a real film"]
async fn soft_subs_of_a_real_film() {
    use std::time::Instant;
    let Ok(file) = std::env::var("LEXPAD_SOFTSUB_FILE") else {
        return;
    };
    let file = PathBuf::from(file);
    let tools = Tools::locate().expect("ffmpeg");
    let json = tools::probe_json(&tools, &file).await.unwrap();
    let info = probe::parse(&json).unwrap();
    let state = super::MediaState::new(None);
    for st in &info.subtitles {
        println!(
            "#{} {} {:?} tag {:?} title {:?} default {} forced {} events {:?}",
            st.index,
            st.codec,
            st.kind,
            st.language.or(st.language_tag.as_deref().map(|_| "?")),
            st.title,
            st.default,
            st.forced,
            st.events
        );
        let t = Instant::now();
        let last = std::sync::Mutex::new(0.0);
        let raw = match state
            .extract(&file, &info, st, &|f| *last.lock().unwrap() = f)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                println!("   not read: {e}");
                continue;
            }
        };
        let tr = load_track(
            format!("embedded:{}", st.index),
            &raw,
            st.extract_as,
            None,
            st.language,
        )
        .unwrap();
        println!(
            "   {} bytes in {} ms (last progress {:.2}), {} cues, {}, language {:?}",
            raw.len(),
            t.elapsed().as_millis(),
            *last.lock().unwrap(),
            tr.parsed.cues.len(),
            tr.encoding.encoding,
            tr.language.best.as_ref().map(|g| (g.code, g.confidence))
        );
        for c in tr.parsed.cues.iter().take(5) {
            println!(
                "   {:>8} → {:>8}  {}",
                c.start,
                c.end,
                c.text.replace('\n', " / ")
            );
        }
    }
}

/// `mean_volume` and `max_volume` (dBFS) of a file's audio, by FFmpeg's volumedetect.
async fn loudness(tools: &Tools, file: &Path) -> (f64, f64) {
    let out = tokio::process::Command::new(&tools.ffmpeg)
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(file)
        .args(["-map", "0:a", "-af", "volumedetect", "-f", "null", "-"])
        .output()
        .await
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    let read = |key: &str| -> f64 {
        err.lines()
            .find_map(|l| l.split(key).nth(1))
            .and_then(|v| v.trim().trim_end_matches(" dB").parse().ok())
            .unwrap_or_else(|| panic!("no {key} in {err}"))
    };
    (read("mean_volume:"), read("max_volume:"))
}

/// The run's whole output, pulled to its end.
async fn pull_all(run: &super::stream::Run) -> Vec<u8> {
    pull_some(run, usize::MAX).await
}

/// Volume above 100% on macOS (WKWebView): a boosted run's sound is twice
/// as loud (+6 dB) where there is room, and never above the limiter's
/// -1 dBFS where the doubled peaks would clip; a Direct MP4 becomes an
/// audio transcode while boost is on, and plain again when it is off.
#[tokio::test(flavor = "multi_thread")]
async fn a_boosted_run_is_twice_as_loud_and_never_clips() {
    let Some(tools) = tools_or_skip() else {
        return;
    };
    let caps = tools::caps(&tools).await;
    let dir = std::env::temp_dir().join(format!("lexpad-boost-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    // 4 s quiet (0.1, -20 dBFS peak) then 4 s loud (0.7, -3 dBFS peak), H.264-free
    // so the clip is made by every build: MPEG-4 video copied is not needed here,
    // only the audio is measured, so the video is FFmpeg's own mpeg4 in MP4.
    for (name, amp) in [("quiet.mp4", 0.1), ("loud.mp4", 0.7)] {
        let expr = format!("aevalsrc={amp}*sin(2*PI*440*t):s=48000:d=6");
        make(
            &tools,
            &dir,
            name,
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x120:rate=25:duration=6",
                "-f",
                "lavfi",
                "-i",
                &expr,
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
            ],
        )
        .await;
    }
    if caps.h264.is_none() {
        eprintln!("no H.264 encoder works here: the boost run is skipped");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    for (name, boost) in [
        ("quiet.mp4", false),
        ("quiet.mp4", true),
        ("loud.mp4", false),
        ("loud.mp4", true),
    ] {
        let file = dir.join(name);
        let json = tools::probe_json(&tools, &file).await.unwrap();
        let info = Arc::new(probe::parse(&json).unwrap());
        let session = Session::new(
            "s".into(),
            "f".into(),
            file.clone(),
            info,
            tools.clone(),
            caps,
            Options {
                boost,
                ..Options::default()
            },
        )
        .unwrap();
        let gain = session.plan().audio.as_ref().unwrap().gain;
        assert_eq!(gain, if boost { 2.0 } else { 1.0 });
        let run = session.start_run(0.0).unwrap();
        let bytes = pull_all(&run).await;
        let out = dir.join(format!("{name}-{boost}.mp4"));
        std::fs::write(&out, &bytes).unwrap();
        let (mean, max) = loudness(&tools, &out).await;
        eprintln!("{name} boost={boost}: mean {mean} dB, max {max} dB");
        let (src_mean, src_max) = loudness(&tools, &file).await;
        if !boost {
            assert!((mean - src_mean).abs() < 0.5, "unboosted keeps the level");
        } else if name == "quiet.mp4" {
            assert!(
                (mean - src_mean - 6.0).abs() < 0.6,
                "quiet sound doubled: {src_mean} -> {mean}"
            );
            assert!((max - src_max - 6.0).abs() < 0.6);
        } else {
            assert!(max <= -0.5, "loud peaks limited, not clipped: max {max} dB");
            assert!(mean > src_mean + 1.0, "still louder: {src_mean} -> {mean}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
