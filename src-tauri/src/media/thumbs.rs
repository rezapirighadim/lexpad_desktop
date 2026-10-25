//! Frames for the seek bar's preview and "Continue watching":
//! `lexmedia://localhost/t/<file id>/<ms>.jpg`, one JPEG per `STEP_S`
//! seconds of the file, made on demand by FFmpeg (a key-frame seek and one
//! decoded frame, tone-mapped when the file is HDR), at most `PARALLEL` at
//! a time, kept in memory up to `CACHE_BYTES`.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::Semaphore;

use super::probe::{Hdr, MediaInfo};
use super::protocol;
use super::tools::{self, Tools};

/// One frame per this many seconds: the preview's grain.
pub const STEP_S: f64 = 10.0;
/// The frames' width; the height follows the picture.
const WIDTH: u32 = 320;
/// Memory kept for frames (about 15 KB each: some 4,000 frames).
const CACHE_BYTES: usize = 64 * 1024 * 1024;
/// FFmpegs making frames at once, so a fast scrub never floods the machine.
const PARALLEL: usize = 2;
const TIMEOUT: Duration = Duration::from_secs(15);

/// The frame's second for `seconds`: the step it falls in.
pub fn snap(seconds: f64) -> u64 {
    ((seconds.max(0.0) / STEP_S).floor() * STEP_S) as u64
}

/// The address of the frame at `seconds` of a file.
pub fn url(file_id: &str, seconds: f64) -> String {
    format!(
        "{}/t/{file_id}/{}.jpg",
        protocol::base_url(),
        snap(seconds) * 1000
    )
}

/// The frames kept: by (file id, second), oldest first, and their bytes.
#[derive(Default)]
struct Cache {
    frames: HashMap<(String, u64), Vec<u8>>,
    order: VecDeque<(String, u64)>,
    bytes: usize,
}

pub struct Thumbs {
    cache: Mutex<Cache>,
    gate: Semaphore,
}

impl Default for Thumbs {
    fn default() -> Self {
        Thumbs {
            cache: Mutex::new(Cache::default()),
            gate: Semaphore::new(PARALLEL),
        }
    }
}

fn filters(info: &MediaInfo) -> String {
    let mut f = format!("scale={WIDTH}:-2:flags=bilinear");
    if let Some(h @ (Hdr::Pq | Hdr::Hlg)) = info.video.first().and_then(|v| v.hdr) {
        let tin = if h == Hdr::Hlg {
            "arib-std-b67"
        } else {
            "smpte2084"
        };
        f.push_str(&format!(
            ",zscale=tin={tin}:t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=pc"
        ));
    }
    f.push_str(",format=yuvj420p");
    f
}

impl Thumbs {
    /// The JPEG of `file_id` at `second` (already snapped).
    pub async fn frame(
        &self,
        tools: &Tools,
        file_id: &str,
        path: &std::path::Path,
        info: &MediaInfo,
        second: u64,
    ) -> Result<Vec<u8>, String> {
        let key = (file_id.to_owned(), second);
        if let Some(b) = self.cache.lock().map_err(|_| "lock")?.frames.get(&key) {
            return Ok(b.clone());
        }
        let _permit = self.gate.acquire().await.map_err(|_| "closed")?;
        let at = format!("{second}");
        let path_s = path.to_str().ok_or("path")?;
        let vf = filters(info);
        let jpeg = tools::output(
            &tools.ffmpeg,
            &[
                "-hide_banner",
                "-nostdin",
                "-loglevel",
                "error",
                "-ss",
                &at,
                "-i",
                path_s,
                "-map",
                "0:v:0",
                "-frames:v",
                "1",
                "-vf",
                &vf,
                "-f",
                "image2pipe",
                "-c:v",
                "mjpeg",
                "-q:v",
                "6",
                "pipe:1",
            ],
            TIMEOUT,
        )
        .await?;
        if jpeg.is_empty() {
            return Err("no_frame".into());
        }
        let mut c = self.cache.lock().map_err(|_| "lock")?;
        c.bytes += jpeg.len();
        c.frames.insert(key.clone(), jpeg.clone());
        c.order.push_back(key);
        while c.bytes > CACHE_BYTES {
            let Some(old) = c.order.pop_front() else {
                break;
            };
            if let Some(b) = c.frames.remove(&old) {
                c.bytes -= b.len();
            }
        }
        Ok(jpeg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_fall_on_steps() {
        assert_eq!(snap(0.0), 0);
        assert_eq!(snap(19.9), 10);
        assert_eq!(snap(-3.0), 0);
        assert!(url("abc", 125.0).ends_with("/t/abc/120000.jpg"));
    }
}
