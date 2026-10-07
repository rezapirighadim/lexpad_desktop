//! The end-to-end test of Lexpad's window against a local stack (never
//! production). Ignored by default; `scripts/e2e-main.sh` runs it with a
//! browser driver beside it (`scripts/e2e-main.mjs`).
//!
//! The window's page (the built `dist/index.html`, the web app from `web/`)
//! runs in Chromium with `window.__LEXPAD_CONFIG` as the app injects it and
//! Tauri's `invoke` pointed at the bridge below, which answers with the
//! app's own code: `connect` is the real RFC 8252 sign-in (the driver signs
//! in on the local web app's /connect-desktop and presses Allow), and every
//! API call goes through `proxy::check` and `Api::forward` with the session
//! this test holds. So the page signs in, syncs, practises (also offline,
//! from its IndexedDB) and signs out exactly as in the app, with no token
//! ever in it.
//!
//! What it cannot exercise from an automated shell: the native window itself
//! (WKWebView, its title bar, the Dock); that is the owner's checklist.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

use crate::api::{Api, Device};
use crate::auth;
use crate::proxy;
use crate::store::{Keyring, SessionStore};

fn local(var: &str) -> String {
    let v = std::env::var(var).unwrap_or_else(|_| panic!("{var} is not set"));
    assert!(
        v.starts_with("http://localhost:") || v.starts_with("http://127.0.0.1:"),
        "{var} must be a local address, never production"
    );
    v
}

fn write(path: &str, text: &str) {
    std::fs::write(path, text).unwrap_or_else(|e| panic!("write {path}: {e}"));
}

struct Ctx {
    api: Arc<Api>,
    api_origin: String,
    app_origin: String,
    dir: String,
    done: Notify,
    /// Every request the page sent, as the core received it.
    seen: std::sync::Mutex<Vec<Value>>,
}

async fn read_request(s: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 65536];
    loop {
        let n = s.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&buf[..end]).to_string();
        let len = head
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
            })
            .unwrap_or(0);
        if buf.len() >= end + 4 + len {
            return Some((head, buf[end + 4..end + 4 + len].to_vec()));
        }
    }
}

async fn answer(s: &mut TcpStream, body: &str) {
    let cors = "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: content-type\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\n";
    let _ = s
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\n{cors}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await;
}

async fn handle(ctx: Arc<Ctx>, mut s: TcpStream) {
    let Some((head, body)) = read_request(&mut s).await else {
        return;
    };
    if head.starts_with("OPTIONS") {
        let _ = s.write_all(b"HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: content-type\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        return;
    }
    let req: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let cmd = req["cmd"].as_str().unwrap_or("").to_owned();
    let args = req["args"].clone();
    let result: Result<Value, String> = match cmd.as_str() {
        "main_signed_in" => Ok(json!(ctx.api.user().await.is_some())),
        "main_take_pending" | "cancel_connect" => Ok(Value::Null),
        "plugin:event|listen" => Ok(json!(1)),
        "plugin:event|unlisten" => Ok(Value::Null),
        "connect" => connect(&ctx).await,
        "disconnect" => {
            ctx.api.sign_out().await;
            write(&format!("{}/signed-out.txt", ctx.dir), "ok");
            Ok(Value::Null)
        }
        "api_fetch" => fetch(&ctx, &args["request"]).await,
        // The desktop section of Settings, from a fresh install's settings
        // and this computer's real permission states.
        "desktop_settings" => {
            let s = crate::settings::Settings::default();
            Ok(json!({
                "os": if cfg!(target_os = "macos") { "macos" } else { "windows" },
                "version": crate::config::VERSION,
                "developmentBuild": true,
                "shortcut": s.shortcut,
                "startOnLogin": s.start_on_login,
                "openOnLaunch": s.open_on_launch,
                "openInBrowser": s.open_in_browser,
                "notebookId": s.notebook_id,
                "accessibility": crate::capture::permission(),
                "accessibilityStale": false,
                "notifications": s.notifications,
                "notificationAccess": crate::notify::access(),
            }))
        }
        "e2e_done" => {
            ctx.done.notify_one();
            Ok(Value::Null)
        }
        other => Err(format!("unknown command {other}")),
    };
    let out = match result {
        Ok(v) => json!({ "ok": true, "value": v }),
        Err(e) => json!({ "ok": false, "error": e }),
    };
    answer(&mut s, &out.to_string()).await;
}

/// The app's `connect` command, step for step, with the address written
/// down for the driver instead of opened in a browser.
async fn connect(ctx: &Ctx) -> Result<Value, String> {
    let pkce = auth::new_pkce();
    let state = auth::new_state();
    let (listener, port) = auth::listen().await.map_err(|_| "error".to_owned())?;
    let redirect = auth::redirect_uri(port);
    write(
        &format!("{}/connect-url.txt", ctx.dir),
        &auth::connect_url(&ctx.app_origin, &redirect, &state, &pkce.challenge),
    );
    let cancel = Notify::new();
    let answer = tokio::time::timeout(
        Duration::from_secs(180),
        auth::wait_for_answer(listener, &state, &cancel),
    )
    .await
    .map_err(|_| "timeout".to_owned())?;
    let Some(auth::Answer::Code(code)) = answer else {
        return Err("cancelled".into());
    };
    let device = Device {
        platform: "desktop",
        device_id: format!("e2e-main-{}", uuid::Uuid::new_v4()),
        device_name: "macOS".into(),
        app_version: crate::config::VERSION.into(),
    };
    let user = ctx
        .api
        .exchange_code(&code, &pkce.verifier, &redirect, &device)
        .await
        .map_err(|f| f.as_str().to_owned())?;
    Ok(serde_json::to_value(user).unwrap())
}

/// The app's `api_fetch` command, with the body sent back whole (the page's
/// channel is fed by the driver).
async fn fetch(ctx: &Ctx, request: &Value) -> Result<Value, String> {
    use base64::Engine as _;
    let req: proxy::Request = serde_json::from_value(request.clone()).map_err(|e| e.to_string())?;
    if let Ok(mut seen) = ctx.seen.lock() {
        seen.push(json!({ "method": req.method, "url": req.url, "headers": req.headers }));
    }
    let checked = proxy::check(&ctx.api_origin, &req).map_err(|_| "error".to_owned())?;
    let res = ctx
        .api
        .forward(&checked)
        .await
        .map_err(|f| f.as_str().to_owned())?;
    let status = res.status().as_u16();
    let content_type = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let bytes = res.bytes().await.map_err(|_| "offline".to_owned())?;
    Ok(json!({
        "status": status,
        "headers": [["content-type", content_type]],
        "body": base64::engine::general_purpose::STANDARD.encode(&bytes),
    }))
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the local stack; run scripts/e2e-main.sh"]
async fn local_main_window() {
    let api_origin = local("LEXPAD_E2E_API");
    let app_origin = local("LEXPAD_E2E_APP");
    let dir = std::env::var("LEXPAD_E2E_DIR").expect("LEXPAD_E2E_DIR");

    // A Keychain entry of its own, so the test never touches an app's session.
    let store = Arc::new(Keyring::new(&format!("{api_origin} main window test")));
    store.clear();
    let ctx = Arc::new(Ctx {
        api: Arc::new(Api::new(&api_origin, store.clone())),
        api_origin,
        app_origin,
        dir: dir.clone(),
        done: Notify::new(),
        seen: std::sync::Mutex::new(Vec::new()),
    });

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    write(
        &format!("{dir}/bridge-port.txt"),
        &listener.local_addr().unwrap().port().to_string(),
    );
    let server = {
        let ctx = ctx.clone();
        tokio::spawn(async move {
            loop {
                let Ok((s, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(handle(ctx.clone(), s));
            }
        })
    };
    tokio::time::timeout(Duration::from_secs(600), ctx.done.notified())
        .await
        .expect("the driver finished in time");
    server.abort();
    if let Ok(why) = std::fs::read_to_string(format!("{dir}/driver-failed.txt")) {
        panic!("the browser half failed: {why}");
    }

    // The page asked for everything through the core, and never carried a token.
    let seen = ctx.seen.lock().unwrap().clone();
    write(
        &format!("{dir}/requests.json"),
        &serde_json::to_string_pretty(&seen).unwrap(),
    );
    assert!(seen.len() > 3, "the page made its calls through the core");
    for r in &seen {
        let headers = r["headers"].as_array().unwrap();
        assert!(
            headers.iter().all(|h| !h[0]
                .as_str()
                .unwrap_or("")
                .eq_ignore_ascii_case("authorization")),
            "the page never sends a token: {r}"
        );
    }
    assert!(
        seen.iter()
            .any(|r| r["url"].as_str().unwrap_or("").contains("/sync/")),
        "the page synced"
    );
    // The page signed out through the core, which revoked and forgot the session.
    assert!(
        store.load().is_none(),
        "signing out in the window empties the Keychain entry"
    );
}
