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
            // Not -hw_encoding 1: where the GPU has no H.264 MFT, Windows'
            // own software MFT (always there) does the work.
            Encoder::MediaFoundation => &["-scenario", "display_remoting"],
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

/// The seconds in one line of FFmpeg's `-progress` output (`out_time_us=`;
/// "N/A" before the first packet).
pub fn progress_seconds(line: &str) -> Option<f64> {
    let v = line.trim().strip_prefix("out_time_us=")?;
    let us: i64 = v.parse().ok()?;
    (us >= 0).then(|| us as f64 / 1_000_000.0)
}

/// Runs FFmpeg with `-progress pipe:1` among `args` to its end, calling
/// `on_time` with each position it reports. It is stopped when it says
/// nothing for `stall` (a drive that stopped answering) or runs past
/// `limit`; a slow drive that still moves is waited for. The error is the
/// last line FFmpeg wrote, or `timeout`.
pub async fn run_with_progress(
    program: &Path,
    args: &[&str],
    stall: Duration,
    limit: Duration,
    on_time: &(dyn Fn(f64) + Send + Sync),
) -> Result<(), String> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
    let mut c = command(program);
    c.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c
        .spawn()
        .map_err(|e| format!("could not run {}: {e}", program.display()))?;
    let stdout = child.stdout.take().ok_or("stdout")?;
    let mut stderr = child.stderr.take().ok_or("stderr")?;
    // Read alongside, so a chatty error log can never fill its pipe and stop FFmpeg.
    let errors = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        String::from_utf8_lossy(&buf).into_owned()
    });
    let deadline = tokio::time::Instant::now() + limit;
    let mut lines = BufReader::new(stdout).lines();
    loop {
        let wait = stall.min(deadline.saturating_duration_since(tokio::time::Instant::now()));
        match tokio::time::timeout(wait, lines.next_line()).await {
            Ok(Ok(Some(line))) => {
                if let Some(t) = progress_seconds(&line) {
                    on_time(t);
                }
            }
            Ok(Ok(None)) | Ok(Err(_)) => break,
            Err(_) => {
                let _ = child.kill().await;
                errors.abort();
                return Err("timeout".into());
            }
        }
    }
    let status = tokio::time::timeout(stall, child.wait()).await;
    let log = errors.await.unwrap_or_default();
    match status {
        Ok(Ok(st)) if st.success() => Ok(()),
        Ok(Ok(_)) => Err(log.lines().last().unwrap_or("failed").trim().to_owned()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => {
            let _ = child.kill().await;
            Err("timeout".into())
        }
    }
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
        // The trial runs the encoder exactly as a transcode will (its
        // options, a bit rate, key frames): a listed encoder that opens
        // with defaults may still refuse the real settings (Media
        // Foundation with hardware forced, on a machine without a GPU).
        let mut args: Vec<String> = [
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=640x360:r=30:d=0.5",
            "-vf",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        args.push(format!("format={}", enc.pix_fmt()));
        args.extend(
            [
                "-c:v",
                enc.ffmpeg_name(),
                "-b:v",
                "2000000",
                "-maxrate",
                "3000000",
                "-bufsize",
                "4000000",
                "-g",
                "60",
            ]
            .map(String::from),
        );
        args.extend(enc.options());
        args.extend(["-frames:v", "10", "-f", "null", "-"].map(String::from));
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let ok = output(&tools.ffmpeg, &refs, TRIAL_TIMEOUT).await.is_ok();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_ffmpeg_progress_lines() {
        assert_eq!(progress_seconds("out_time_us=2991557000"), Some(2991.557));
        assert_eq!(progress_seconds("out_time_us=N/A"), None);
        assert_eq!(progress_seconds("out_time_us=-5"), None);
        assert_eq!(progress_seconds("out_time_ms=2991557000"), None);
        assert_eq!(progress_seconds("progress=end"), None);
    }
}
