//! A playing file: its plan, and the FFmpeg run that makes its stream.
//!
//! A run is one FFmpeg process from a start time, writing fragmented MP4 to
//! a pipe; the core keeps what it wrote in a buffer, and the page pulls it
//! by byte offset (`lexmedia://localhost/s/<session>/<run>?from=<offset>`,
//! `protocol`), appending each answer to its `SourceBuffer`. A pull for
//! offset N drops everything before N (the page has it). When the page is
//! far enough ahead it stops pulling, the buffer fills to `MAX_AHEAD`, the
//! reader stops reading, the pipe fills, and FFmpeg waits: nothing runs
//! ahead without limit, in memory or in CPU.
//!
//! A seek outside what the page has, a new audio track or a new audio delay
//! ends the run (FFmpeg is killed) and starts another at the new time; its
//! number goes up, so a late pull for the old run is answered "gone".

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::sync::Notify;

use super::plan::{self, Options, Plan};
use super::probe::MediaInfo;
use super::tools::{self, Caps, Tools};

/// The most bytes a run holds that the page has not pulled: about 10-60
/// seconds of video depending on its bit rate. The page asks for more
/// long before it runs out.
const MAX_AHEAD: usize = 48 * 1024 * 1024;
/// The most one pull returns.
pub const MAX_PULL: usize = 4 * 1024 * 1024;
/// How long a pull waits for FFmpeg before answering "nothing yet" (the
/// page asks again at once).
const PULL_WAIT: Duration = Duration::from_secs(20);
/// FFmpeg's last error lines kept for the page and the log.
const STDERR_LINES: usize = 8;

#[derive(Default)]
struct Buf {
    /// The offset of `data[0]` in the run's output.
    base: u64,
    data: VecDeque<u8>,
    done: bool,
    error: Option<String>,
}

pub struct Run {
    pub id: u64,
    /// Where it started, in the file's seconds.
    pub start: f64,
    started: Instant,
    /// Milliseconds from starting FFmpeg to its first bytes (the latency
    /// of a seek), once known.
    pub first_bytes_ms: AtomicU64,
    buf: Mutex<Buf>,
    changed: Notify,
    cancelled: AtomicBool,
}

/// What a pull gets.
pub enum Pulled {
    Bytes {
        data: Vec<u8>,
        next: u64,
        end: bool,
    },
    /// Nothing yet; ask again.
    Wait {
        next: u64,
    },
    /// The run is over and everything was pulled.
    End {
        next: u64,
    },
    Failed(String),
    /// Asked for bytes already dropped (an offset before an earlier pull).
    Gone,
}

impl Run {
    fn new(id: u64, start: f64) -> Run {
        Run {
            id,
            start,
            started: Instant::now(),
            first_bytes_ms: AtomicU64::new(0),
            buf: Mutex::new(Buf::default()),
            changed: Notify::new(),
            cancelled: AtomicBool::new(false),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Resolves once the run is cancelled.
    async fn cancelled(&self) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }

    /// The bytes from `from`, waiting for FFmpeg when there are none yet.
    pub async fn pull(&self, from: u64, max: usize) -> Pulled {
        let deadline = Instant::now() + PULL_WAIT;
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let Ok(mut b) = self.buf.lock() else {
                    return Pulled::Failed("lock".into());
                };
                if from < b.base {
                    return Pulled::Gone;
                }
                // The page has everything before `from`: let it go.
                let drop = ((from - b.base) as usize).min(b.data.len());
                b.data.drain(..drop);
                b.base += drop as u64;
                if from > b.base {
                    // Asked for bytes not written yet: wait for them.
                } else if !b.data.is_empty() {
                    let n = b.data.len().min(max);
                    let data: Vec<u8> = b.data.iter().take(n).copied().collect();
                    let next = from + n as u64;
                    let end = b.done && n == b.data.len() && b.error.is_none();
                    drop_lock_and_wake(b, &self.changed);
                    return Pulled::Bytes { data, next, end };
                } else if b.done {
                    return match &b.error {
                        Some(e) => Pulled::Failed(e.clone()),
                        None => Pulled::End { next: from },
                    };
                }
            }
            if self.is_cancelled() {
                return Pulled::Gone;
            }
            if tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), notified)
                .await
                .is_err()
            {
                return Pulled::Wait { next: from };
            }
        }
    }

    fn ahead(&self) -> usize {
        self.buf.lock().map(|b| b.data.len()).unwrap_or(0)
    }

    fn push(&self, bytes: &[u8]) {
        if let Ok(mut b) = self.buf.lock() {
            b.data.extend(bytes);
        }
        if self.first_bytes_ms.load(Ordering::Relaxed) == 0 {
            let ms = self.started.elapsed().as_millis().max(1) as u64;
            self.first_bytes_ms.store(ms, Ordering::Relaxed);
        }
        self.changed.notify_waiters();
    }

    fn finish(&self, error: Option<String>) {
        if let Ok(mut b) = self.buf.lock() {
            b.done = true;
            b.error = error;
        }
        self.changed.notify_waiters();
    }
}

/// A pull made room: the reader may read on.
fn drop_lock_and_wake(guard: std::sync::MutexGuard<'_, Buf>, changed: &Notify) {
    drop(guard);
    changed.notify_waiters();
}

/// Starts FFmpeg for `run` and feeds its output into the run's buffer until
/// it ends or the run is cancelled.
fn spawn(run: Arc<Run>, ffmpeg: PathBuf, args: Vec<String>) {
    tauri::async_runtime::spawn(async move {
        let mut cmd = tools::command(&ffmpeg);
        cmd.args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                run.finish(Some(format!("could not start ffmpeg: {e}")));
                return;
            }
        };
        let (Some(mut stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            run.finish(Some("no pipes".into()));
            return;
        };
        let errors = Arc::new(Mutex::new(VecDeque::<String>::new()));
        let err_lines = errors.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(mut l) = err_lines.lock() {
                    if l.len() == STDERR_LINES {
                        l.pop_front();
                    }
                    l.push_back(line);
                }
            }
        });
        let mut chunk = vec![0u8; 256 * 1024];
        let mut error = None;
        loop {
            if run.is_cancelled() {
                break;
            }
            // Back-pressure: wait while the page has not pulled enough.
            while run.ahead() >= MAX_AHEAD && !run.is_cancelled() {
                let notified = run.changed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if run.ahead() < MAX_AHEAD || run.is_cancelled() {
                    break;
                }
                let _ = tokio::time::timeout(Duration::from_secs(1), notified).await;
            }
            if run.is_cancelled() {
                break;
            }
            let read = tokio::select! {
                r = stdout.read(&mut chunk) => r,
                _ = run.cancelled() => break,
            };
            match read {
                Ok(0) => break,
                Ok(n) => run.push(&chunk[..n]),
                Err(e) => {
                    error = Some(format!("read: {e}"));
                    break;
                }
            }
        }
        if run.is_cancelled() {
            let _ = child.kill().await;
            run.finish(None);
            return;
        }
        match child.wait().await {
            Ok(status) if status.success() => {}
            Ok(status) => {
                let detail = errors
                    .lock()
                    .map(|l| l.iter().cloned().collect::<Vec<_>>().join(" | "))
                    .unwrap_or_default();
                log::warn!("media: ffmpeg exited with {status}: {detail}");
                error = error.or(Some(if detail.is_empty() {
                    format!("ffmpeg {status}")
                } else {
                    detail
                }));
            }
            Err(e) => error = error.or(Some(e.to_string())),
        }
        run.finish(error);
    });
}

pub struct Session {
    pub id: String,
    pub file_id: String,
    pub path: PathBuf,
    pub ext: String,
    pub info: Arc<MediaInfo>,
    pub caps: Caps,
    tools: Tools,
    pub state: Mutex<(Options, Plan)>,
    run: Mutex<Option<Arc<Run>>>,
    next_run: AtomicU64,
}

impl Session {
    pub fn new(
        id: String,
        file_id: String,
        path: PathBuf,
        info: Arc<MediaInfo>,
        tools: Tools,
        caps: Caps,
        opts: Options,
    ) -> Result<Session, plan::Unplayable> {
        let ext = super::files::ext_of(&path);
        let plan = plan::decide(&info, &ext, opts, &caps)?;
        Ok(Session {
            id,
            file_id,
            path,
            ext,
            info,
            caps,
            tools,
            state: Mutex::new((opts, plan)),
            run: Mutex::new(None),
            next_run: AtomicU64::new(1),
        })
    }

    pub fn plan(&self) -> Plan {
        self.state
            .lock()
            .map(|s| s.1.clone())
            .expect("session state")
    }

    /// New options (audio track, delay): a new plan, which may change the mode.
    pub fn replan(&self, opts: Options) -> Result<Plan, plan::Unplayable> {
        let plan = plan::decide(&self.info, &self.ext, opts, &self.caps)?;
        if let Ok(mut s) = self.state.lock() {
            *s = (opts, plan.clone());
        }
        Ok(plan)
    }

    /// Ends the current run and starts one at `start` (the file's seconds).
    pub fn start_run(&self, start: f64) -> Option<Arc<Run>> {
        let plan = self.plan();
        if let Ok(mut cur) = self.run.lock() {
            if let Some(old) = cur.take() {
                old.cancel();
            }
            if plan.mode == plan::Mode::Direct {
                return None;
            }
            let duration = self.info.duration.unwrap_or(f64::MAX);
            let start = start.clamp(0.0, (duration - 0.5).max(0.0));
            let id = self.next_run.fetch_add(1, Ordering::SeqCst);
            let run = Arc::new(Run::new(id, start));
            let path = self.path.to_string_lossy().into_owned();
            let args = plan::args(&plan, &self.info, &path, start, &self.caps);
            log::info!(
                "media: run {id} of {} from {start:.2}s ({:?})",
                self.id,
                plan.mode
            );
            spawn(run.clone(), self.tools.ffmpeg.clone(), args);
            *cur = Some(run.clone());
            return Some(run);
        }
        None
    }

    /// The run a pull names, if it is still the current one.
    pub fn run(&self, id: u64) -> Option<Arc<Run>> {
        let cur = self.run.lock().ok()?;
        cur.as_ref().filter(|r| r.id == id).cloned()
    }

    pub fn stop(&self) {
        if let Ok(mut cur) = self.run.lock() {
            if let Some(old) = cur.take() {
                old.cancel();
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pulls_drop_what_the_page_has_and_wait_for_more() {
        let run = Arc::new(Run::new(1, 0.0));
        run.push(b"hello ");
        let Pulled::Bytes { data, next, end } = run.pull(0, 4).await else {
            panic!()
        };
        assert_eq!((data.as_slice(), next, end), (&b"hell"[..], 4, false));
        // Asking from 4 drops the first four bytes.
        let Pulled::Bytes { data, next, .. } = run.pull(4, 100).await else {
            panic!()
        };
        assert_eq!((data.as_slice(), next), (&b"o "[..], 6));
        assert!(matches!(run.pull(0, 10).await, Pulled::Gone));
        // A pull waits for FFmpeg's next bytes.
        let r = run.clone();
        let waiter = tokio::spawn(async move { r.pull(6, 100).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        run.push(b"world");
        run.finish(None);
        let Pulled::Bytes { data, next, end } = waiter.await.unwrap() else {
            panic!()
        };
        assert_eq!((data.as_slice(), next, end), (&b"world"[..], 11, true));
        assert!(matches!(run.pull(11, 10).await, Pulled::End { next: 11 }));
    }

    #[tokio::test]
    async fn a_failed_run_says_why_and_a_cancelled_one_is_gone() {
        let run = Run::new(2, 0.0);
        run.finish(Some("bad file".into()));
        assert!(matches!(run.pull(0, 10).await, Pulled::Failed(e) if e == "bad file"));
        let run = Arc::new(Run::new(3, 0.0));
        let r = run.clone();
        let waiter = tokio::spawn(async move { r.pull(0, 10).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        run.cancel();
        assert!(matches!(waiter.await.unwrap(), Pulled::Gone));
    }
}
