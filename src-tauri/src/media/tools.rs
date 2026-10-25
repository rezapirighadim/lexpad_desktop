//! The bundled FFmpeg and ffprobe (Tauri sidecars, `bundle.externalBin`):
//! where they are, what this machine's copy can do (which H.264 encoder
//! works, hardware decoding, zscale for tone-mapping), and running them.
//!
//! The sidecars sit beside the app's executable (`Contents/MacOS/` on
//! macOS, the install folder on Windows). `cargo test` and `tauri dev` use
//! `src-tauri/binaries/<tool>-<target triple>` (scripts/ffmpeg/), and
//! `LEXPAD_FFMPEG_DIR` may point a development build anywhere else.
//!
//! On Windows every process is started with CREATE_NO_WINDOW, or each run
//! would flash a console window over the player.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use serde::Serialize;
use tokio::process::Command;

/// The target triple this build is for (build.rs), for the development
/// copies in src-tauri/binaries/.
const TARGET: &str = env!("LEXPAD_TARGET_TRIPLE");
/// How long a probe may take: a file on a sleeping network drive or a
/// broken file must not hang the player.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
/// How long an encoder's trial may take.
const TRIAL_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Encoder {
    /// macOS: Apple's hardware encoder (software fallback inside VideoToolbox).
    VideoToolbox,
    /// Windows: NVIDIA.
    Nvenc,
    /// Windows: Intel Quick Sync.
    Qsv,
    /// Windows: AMD.
    Amf,
    /// Windows: Media Foundation (whatever hardware MFT the GPU driver has).
    MediaFoundation,
    /// Cisco's OpenH264 (BSD), software: the last resort where no hardware
    /// encoder works. Never x264 (GPL).
    OpenH264,
}

impl Encoder {
    pub fn ffmpeg_name(self) -> &'static str {
        match self {
            Encoder::VideoToolbox => "h264_videotoolbox",
            Encoder::Nvenc => "h264_nvenc",
            Encoder::Qsv => "h264_qsv",
            Encoder::Amf => "h264_amf",
            Encoder::MediaFoundation => "h264_mf",
            Encoder::OpenH264 => "libopenh264",
        }
    }

    /// The pixel format it takes from the filter chain.
    pub fn pix_fmt(self) -> &'static str {
        match self {
            Encoder::OpenH264 => "yuv420p",
            _ => "nv12",
        }
    }

    /// Speed over size: the player encodes in real time, ahead of playback.
    pub fn options(self) -> Vec<String> {
        let o: &[&str] = match self {
            Encoder::VideoToolbox => &["-profile:v", "high", "-realtime", "1", "-allow_sw", "1"],
            Encoder::Nvenc => &[
                "-profile:v",
                "high",
                "-preset",
                "p4",
                "-tune",
                "ll",
                "-rc",
                "vbr",
            ],
            Encoder::Qsv => &["-profile:v", "high", "-preset", "veryfast"],
            Encoder::Amf => &[
                "-profile:v",
                "high",
                "-quality",
                "speed",
                "-usage",
                "lowlatency",
            ],
            Encoder::MediaFoundation => &["-hw_encoding", "1", "-scenario", "display_remoting"],
            Encoder::OpenH264 => &["-profile:v", "high", "-allow_skip_frames", "1"],
        };
        o.iter().map(|s| s.to_string()).collect()
    }

    /// The order they are tried in on this platform.
    fn candidates() -> &'static [Encoder] {
        if cfg!(target_os = "macos") {
            &[Encoder::VideoToolbox, Encoder::OpenH264]
        } else if cfg!(windows) {
            &[
                Encoder::Nvenc,
                Encoder::Qsv,
                Encoder::Amf,
                Encoder::MediaFoundation,
                Encoder::OpenH264,
            ]
        } else {
            &[Encoder::OpenH264]
        }
    }
}

/// What this machine's FFmpeg can do.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Caps {
    pub h264: Option<Encoder>,
    /// `-hwaccel` for decoding before a transcode.
    pub hwaccel: Option<&'static str>,
    /// The zscale filter (zimg), for tone-mapping HDR.
    pub zscale: bool,
}

#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

impl Tools {
    /// Finds the sidecars, or says where they were looked for.
    pub fn locate() -> Result<Tools, String> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Ok(dir) = std::env::var("LEXPAD_FFMPEG_DIR") {
            if cfg!(debug_assertions) || cfg!(test) {
                dirs.push(PathBuf::from(dir));
            }
        }
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
        {
            dirs.push(dir);
        }
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
        for dir in &dirs {
            let (f, p) = (dir.join(exe("ffmpeg")), dir.join(exe("ffprobe")));
            if f.is_file() && p.is_file() {
                return Ok(Tools {
                    ffmpeg: f,
                    ffprobe: p,
                });
            }
        }
        if cfg!(debug_assertions) || cfg!(test) {
            let (f, p) = (
                dev.join(exe(&format!("ffmpeg-{TARGET}"))),
                dev.join(exe(&format!("ffprobe-{TARGET}"))),
            );
            if f.is_file() && p.is_file() {
                return Ok(Tools {
                    ffmpeg: f,
                    ffprobe: p,
                });
            }
        }
        Err("ffmpeg_missing".into())
    }
}

/// A command for a tool, with no window on Windows and killed when dropped.
pub fn command(program: &Path) -> Command {
    let mut c = Command::new(program);
    c.stdin(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    {
        /// CREATE_NO_WINDOW: no console flashes up for a child of a GUI app.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// Runs a tool to its end and returns its standard output.
pub async fn output(program: &Path, args: &[&str], timeout: Duration) -> Result<Vec<u8>, String> {
    let mut c = command(program);
    c.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = tokio::time::timeout(timeout, c.output())
        .await
        .map_err(|_| "timeout".to_string())?
        .map_err(|e| format!("could not run {}: {e}", program.display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().last().unwrap_or("failed").trim().to_owned());
    }
    Ok(out.stdout)
}

/// ffprobe's JSON for a file.
pub async fn probe_json(tools: &Tools, path: &Path) -> Result<serde_json::Value, String> {
    let path = path.to_str().ok_or("path")?;
    let out = output(
        &tools.ffprobe,
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-show_format",
            "-show_streams",
            "-show_chapters",
            "-of",
            "json",
            path,
        ],
        PROBE_TIMEOUT,
    )
    .await?;
    serde_json::from_slice(&out).map_err(|e| format!("ffprobe json: {e}"))
}

/// The first encoder on this platform's list that encodes a few frames
/// here (a listed encoder may still fail: no GPU of that brand, an old
/// driver, a VM).
async fn find_encoder(tools: &Tools) -> Option<Encoder> {
    let listed = output(&tools.ffmpeg, &["-hide_banner", "-encoders"], TRIAL_TIMEOUT)
        .await
        .map(|o| String::from_utf8_lossy(&o).into_owned())
        .unwrap_or_default();
    for enc in Encoder::candidates() {
        if !listed.contains(enc.ffmpeg_name()) {
            continue;
        }
        let ok = output(
            &tools.ffmpeg,
            &[
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=640x360:r=30:d=0.2",
                "-vf",
                &format!("format={}", enc.pix_fmt()),
                "-c:v",
                enc.ffmpeg_name(),
                "-frames:v",
                "5",
                "-f",
                "null",
                "-",
            ],
            TRIAL_TIMEOUT,
        )
        .await
        .is_ok();
        if ok {
            return Some(*enc);
        }
        log::info!(
            "media: {} is listed but does not work here",
            enc.ffmpeg_name()
        );
    }
    None
}

/// Reads the capabilities once per run of the app (a second or so).
pub async fn caps(tools: &Tools) -> Caps {
    static CAPS: OnceLock<Caps> = OnceLock::new();
    if let Some(c) = CAPS.get() {
        return *c;
    }
    let filters = output(&tools.ffmpeg, &["-hide_banner", "-filters"], TRIAL_TIMEOUT)
        .await
        .map(|o| String::from_utf8_lossy(&o).into_owned())
        .unwrap_or_default();
    let hwaccels = output(&tools.ffmpeg, &["-hide_banner", "-hwaccels"], TRIAL_TIMEOUT)
        .await
        .map(|o| String::from_utf8_lossy(&o).into_owned())
        .unwrap_or_default();
    let hwaccel = if cfg!(target_os = "macos") {
        hwaccels.contains("videotoolbox").then_some("videotoolbox")
    } else if cfg!(windows) {
        // d3d11va, else dxva2, else cuda: FFmpeg picks; it falls back to
        // software decoding by itself when none works for a codec.
        Some("auto")
    } else {
        None
    };
    let c = Caps {
        h264: find_encoder(tools).await,
        hwaccel,
        zscale: filters.contains(" zscale "),
    };
    log::info!("media: {c:?}");
    *CAPS.get_or_init(|| c)
}

/// FFmpeg's version line, for the about box and reports.
pub async fn version(tools: &Tools) -> Option<String> {
    let out = output(&tools.ffmpeg, &["-hide_banner", "-version"], TRIAL_TIMEOUT)
        .await
        .ok()?;
    String::from_utf8_lossy(&out)
        .lines()
        .next()
        .map(|l| l.trim().to_owned())
}
