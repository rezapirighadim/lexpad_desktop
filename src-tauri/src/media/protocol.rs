//! The `lexmedia` scheme: how media reaches the page, without a network
//! socket. `lexmedia://localhost/...` on macOS, `http://lexmedia.localhost/...`
//! on Windows (WebView2 serves custom schemes under that name); `base_url`
//! says which.
//!
//! - `GET /f/<session>` with a `Range`: the file itself, for `Direct`
//!   sessions (`<video src>`), at most `MAX_RANGE` per answer.
//! - `GET /s/<session>/<run>?from=<offset>`: the next bytes of a streamed
//!   run (see `stream`); the answer's `X-Lexmedia-Next` is the offset to ask
//!   for next, `X-Lexmedia-End: 1` says the run is over, 204 means "nothing
//!   yet, ask again", 410 that the run was replaced.
//!
//! Only the windows that may play (Lexpad's window and the development
//! page) are answered; a session id is 128 random bits and is only ever
//! handed to the window that opened it.

use std::io::SeekFrom;
use std::sync::Arc;

use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, Runtime};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use super::stream::{Pulled, MAX_PULL};
use super::MediaState;

pub const SCHEME: &str = "lexmedia";
/// The most bytes one range answer holds; the player asks again for more.
const MAX_RANGE: u64 = 4 * 1024 * 1024;
/// The windows that may load media.
pub const PLAYER_WINDOWS: &[&str] = &["main", "player-dev"];

/// The scheme's origin as the page must write it.
pub fn base_url() -> &'static str {
    if cfg!(windows) {
        "http://lexmedia.localhost"
    } else {
        "lexmedia://localhost"
    }
}

fn reply(status: StatusCode, body: Vec<u8>, headers: &[(&str, String)]) -> Response<Vec<u8>> {
    let mut b = Response::builder()
        .status(status)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            "X-Lexmedia-Next, X-Lexmedia-End, X-Lexmedia-Startup-Ms, Content-Range, Content-Length",
        )
        .header(header::CACHE_CONTROL, "no-store");
    for (k, v) in headers {
        b = b.header(*k, v);
    }
    b.body(body).unwrap_or_else(|_| Response::new(Vec::new()))
}

fn error(status: StatusCode, msg: &str) -> Response<Vec<u8>> {
    reply(
        status,
        msg.as_bytes().to_vec(),
        &[("Content-Type", "text/plain; charset=utf-8".into())],
    )
}

/// `bytes=a-b`, `bytes=a-`, `bytes=-n` against a file of `len` bytes.
pub fn parse_range(h: &str, len: u64) -> Option<(u64, u64)> {
    let spec = h.trim().strip_prefix("bytes=")?;
    let first = spec.split(',').next()?.trim();
    let (a, b) = first.split_once('-')?;
    if len == 0 {
        return None;
    }
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        (len.saturating_sub(n), len - 1)
    } else {
        let s: u64 = a.parse().ok()?;
        let e = if b.is_empty() {
            len - 1
        } else {
            b.parse::<u64>().ok()?.min(len - 1)
        };
        (s, e)
    };
    (start <= end && start < len).then_some((start, end))
}

pub async fn handle<R: Runtime>(
    app: &AppHandle<R>,
    webview: &str,
    req: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    if !PLAYER_WINDOWS.contains(&webview) {
        return error(StatusCode::FORBIDDEN, "not a player window");
    }
    if req.method() == tauri::http::Method::OPTIONS {
        return reply(
            StatusCode::NO_CONTENT,
            Vec::new(),
            &[
                ("Access-Control-Allow-Methods", "GET, OPTIONS".into()),
                ("Access-Control-Allow-Headers", "Range".into()),
            ],
        );
    }
    let Some(state) = app.try_state::<Arc<MediaState>>() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "media engine not ready");
    };
    let path = req.uri().path().to_owned();
    let query = req.uri().query().unwrap_or("").to_owned();
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match parts.as_slice() {
        ["f", sid] => file(&state, sid, &req).await,
        ["s", sid, run] => {
            let from = query
                .split('&')
                .find_map(|kv| kv.strip_prefix("from="))
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            let Ok(run) = run.parse::<u64>() else {
                return error(StatusCode::BAD_REQUEST, "run");
            };
            pull(&state, sid, run, from).await
        }
        _ => error(StatusCode::NOT_FOUND, "no such media"),
    }
}

async fn file(state: &MediaState, sid: &str, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let Some(session) = state.session(sid) else {
        return error(StatusCode::NOT_FOUND, "no such session");
    };
    let Ok(mut f) = tokio::fs::File::open(&session.path).await else {
        return error(StatusCode::NOT_FOUND, "file gone");
    };
    let len = match f.metadata().await {
        Ok(m) => m.len(),
        Err(_) => return error(StatusCode::NOT_FOUND, "file gone"),
    };
    let range = req
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(|h| parse_range(h, len));
    let (start, end) = match range {
        Some(Some(r)) => r,
        Some(None) => {
            return reply(
                StatusCode::RANGE_NOT_SATISFIABLE,
                Vec::new(),
                &[("Content-Range", format!("bytes */{len}"))],
            )
        }
        None => (0, len.saturating_sub(1)),
    };
    let end = end.min(start + MAX_RANGE - 1);
    let n = (end + 1 - start) as usize;
    let mut body = vec![0u8; n];
    if f.seek(SeekFrom::Start(start)).await.is_err() || f.read_exact(&mut body).await.is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "read");
    }
    reply(
        StatusCode::PARTIAL_CONTENT,
        body,
        &[
            ("Content-Type", "video/mp4".into()),
            ("Accept-Ranges", "bytes".into()),
            ("Content-Range", format!("bytes {start}-{end}/{len}")),
            ("Content-Length", n.to_string()),
        ],
    )
}

async fn pull(state: &MediaState, sid: &str, run_id: u64, from: u64) -> Response<Vec<u8>> {
    let Some(session) = state.session(sid) else {
        return error(StatusCode::NOT_FOUND, "no such session");
    };
    let Some(run) = session.run(run_id) else {
        return error(StatusCode::GONE, "replaced");
    };
    let startup = run
        .first_bytes_ms
        .load(std::sync::atomic::Ordering::Relaxed)
        .to_string();
    match run.pull(from, MAX_PULL).await {
        Pulled::Bytes { data, next, end } => {
            let mut h = vec![
                ("Content-Type", "video/mp4".to_string()),
                ("X-Lexmedia-Next", next.to_string()),
                (
                    "X-Lexmedia-Startup-Ms",
                    run.first_bytes_ms
                        .load(std::sync::atomic::Ordering::Relaxed)
                        .to_string(),
                ),
            ];
            if end {
                h.push(("X-Lexmedia-End", "1".into()));
            }
            reply(StatusCode::OK, data, &h)
        }
        Pulled::Wait { next } => reply(
            StatusCode::NO_CONTENT,
            Vec::new(),
            &[
                ("X-Lexmedia-Next", next.to_string()),
                ("X-Lexmedia-Startup-Ms", startup),
            ],
        ),
        Pulled::End { next } => reply(
            StatusCode::OK,
            Vec::new(),
            &[
                ("X-Lexmedia-Next", next.to_string()),
                ("X-Lexmedia-End", "1".into()),
            ],
        ),
        Pulled::Gone => error(StatusCode::GONE, "replaced"),
        Pulled::Failed(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-1", 100), Some((0, 1)));
        assert_eq!(parse_range("bytes=10-", 100), Some((10, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=90-200", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=5-1", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
    }
}
