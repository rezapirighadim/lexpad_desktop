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
    let raw = state.extract("f", &mkv, st).await.unwrap();
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
            match state.extract(&name, &p, st).await {
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
