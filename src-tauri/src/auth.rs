//! Signing in the way RFC 8252 asks of a native app: in the system browser,
//! never in a window of ours, with a loopback redirect and PKCE (RFC 7636).
//!
//! 1. Make a code verifier and its S256 challenge, a random state, and listen
//!    on `127.0.0.1:<random port>`.
//! 2. Open `APP_ORIGIN/connect-desktop?redirect_uri=http://127.0.0.1:<port>/callback
//!    &state=…&code_challenge=…&code_challenge_method=S256` in the browser.
//!    The learner, signed in there, allows it; the web app asks the API for a
//!    one-time code bound to the challenge and the address and sends the
//!    browser to the address.
//! 3. Take the code from the one request that carries our state, and trade it
//!    with the verifier for this app's own session (`api::exchange_code`).
//!
//! The password never reaches the app, the code is worthless without the
//! verifier (which never leaves this process), and the session is a
//! delegated one the learner can sign out alone under Signed-in devices.

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The one path the browser is sent back to. The API accepts no other.
pub const CALLBACK_PATH: &str = "/callback";

/// How long the app waits for the learner to allow it in the browser: long
/// enough to sign in there first, short enough that a forgotten tab does not
/// keep a port open all day.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The most of a request the listener reads. A browser's GET with a code and
/// a state is well under this; anything larger is not that request.
const MAX_REQUEST: usize = 16 * 1024;

/// A PKCE pair: the verifier stays here, the challenge goes to the browser.
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// 32 random bytes as base64url: a 43-character verifier, the shortest RFC
/// 7636 allows and 256 bits of entropy.
pub fn new_pkce() -> Pkce {
    let verifier = random_token();
    let challenge = s256(&verifier);
    Pkce {
        verifier,
        challenge,
    }
}

/// The S256 challenge of a verifier.
pub fn s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The state that ties the browser's answer to this request.
pub fn new_state() -> String {
    random_token()
}

/// The address the browser is sent back to, for a port we listen on.
pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
}

/// The connect page's address, with every part encoded.
pub fn connect_url(app_origin: &str, redirect: &str, state: &str, challenge: &str) -> String {
    let mut url = url::Url::parse(app_origin).expect("APP_ORIGIN is a valid URL");
    url.set_path(crate::config::CONNECT_PATH);
    url.query_pairs_mut()
        .append_pair("redirect_uri", redirect)
        .append_pair("state", state)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256");
    url.into()
}

/// What the browser brought back.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    /// Allowed: the one-time code to trade.
    Code(String),
    /// The learner pressed Cancel.
    Denied,
}

/// What one request to the listener was.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    /// Our callback, with our state.
    Answer(Answer),
    /// Our callback with another state, or none: not the request we wait for.
    WrongState,
    /// Anything else (a favicon, a stray probe).
    Other,
}

/// Reads the request line of an HTTP request: `GET /callback?… HTTP/1.1`.
fn classify(head: &str, state: &str) -> Request {
    let line = head.lines().next().unwrap_or_default();
    let mut parts = line.split(' ');
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return Request::Other;
    };
    let Ok(url) = url::Url::parse(&format!("http://127.0.0.1{target}")) else {
        return Request::Other;
    };
    if url.path() != CALLBACK_PATH {
        return Request::Other;
    }
    let mut code = None;
    let mut error = None;
    let mut got_state = None;
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "error" => error = Some(v.into_owned()),
            "state" => got_state = Some(v.into_owned()),
            _ => {}
        }
    }
    let state_ok = got_state
        .map(|s| s.len() == state.len() && bool::from(s.as_bytes().ct_eq(state.as_bytes())))
        .unwrap_or(false);
    if !state_ok {
        return Request::WrongState;
    }
    match (code, error) {
        (Some(code), None) if !code.is_empty() && code.len() <= 256 => {
            Request::Answer(Answer::Code(code))
        }
        (_, Some(_)) => Request::Answer(Answer::Denied),
        _ => Request::WrongState,
    }
}

const PAGE_DONE: &str = "<!doctype html><meta charset=utf-8><title>Lexpad</title><body style=\"font:15px -apple-system,Segoe UI,sans-serif;margin:3em;color:#1f2320\"><h1 style=\"font-size:20px\">Lexpad is connected</h1><p>You can close this tab and go back to what you were reading.</p>";
const PAGE_DENIED: &str = "<!doctype html><meta charset=utf-8><title>Lexpad</title><body style=\"font:15px -apple-system,Segoe UI,sans-serif;margin:3em;color:#1f2320\"><h1 style=\"font-size:20px\">Nothing was connected</h1><p>You can close this tab.</p>";
const PAGE_WRONG: &str = "<!doctype html><meta charset=utf-8><title>Lexpad</title><body style=\"font:15px -apple-system,Segoe UI,sans-serif;margin:3em;color:#1f2320\"><h1 style=\"font-size:20px\">This link is not the one Lexpad is waiting for</h1><p>Choose Sign in in the Lexpad app again.</p>";

async fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    // No referrer, no caching, no scripts: the address bar holds a one-time
    // code until the tab is closed, and nothing on this page may carry it on.
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body.as_bytes()).await;
    let _ = stream.shutdown().await;
}

async fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() >= MAX_REQUEST {
            break;
        }
    }
    String::from_utf8(buf).ok()
}

/// Waits for the browser to come back with our state, answering every other
/// request without giving up. Ends with the answer, or `None` when `cancel`
/// fires first. The caller bounds the wait with a timeout.
pub async fn wait_for_answer(
    listener: TcpListener,
    state: &str,
    cancel: &tokio::sync::Notify,
) -> Option<Answer> {
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            () = cancel.notified() => return None,
        };
        let Ok((mut stream, peer)) = accepted else {
            continue;
        };
        if !peer.ip().is_loopback() {
            continue;
        }
        let Some(head) = read_head(&mut stream).await else {
            continue;
        };
        match classify(&head, state) {
            Request::Answer(answer) => {
                let page = if answer == Answer::Denied {
                    PAGE_DENIED
                } else {
                    PAGE_DONE
                };
                respond(&mut stream, "200 OK", page).await;
                return Some(answer);
            }
            Request::WrongState => respond(&mut stream, "400 Bad Request", PAGE_WRONG).await,
            Request::Other => respond(&mut stream, "404 Not Found", "").await,
        }
    }
}

/// Listens on a random loopback port.
pub async fn listen() -> std::io::Result<(TcpListener, u16)> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_is_rfc7636_s256() {
        // RFC 7636 appendix B.
        assert_eq!(
            s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let p = new_pkce();
        assert_eq!(p.verifier.len(), 43);
        assert_eq!(p.challenge.len(), 43);
        assert_eq!(s256(&p.verifier), p.challenge);
        assert_ne!(new_pkce().verifier, p.verifier);
        assert_eq!(new_state().len(), 43);
    }

    #[test]
    fn the_connect_url_carries_everything_encoded() {
        let url = connect_url("https://app.lexpad.app", &redirect_uri(53682), "st", "ch");
        let parsed = url::Url::parse(&url).unwrap();
        assert_eq!(
            parsed.origin().ascii_serialization(),
            "https://app.lexpad.app"
        );
        assert_eq!(parsed.path(), "/connect-desktop");
        let q: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:53682/callback");
        assert_eq!(q["state"], "st");
        assert_eq!(q["code_challenge"], "ch");
        assert_eq!(q["code_challenge_method"], "S256");
    }

    #[test]
    fn only_our_callback_with_our_state_is_an_answer() {
        let s = "the-state";
        assert_eq!(
            classify("GET /callback?code=abc&state=the-state HTTP/1.1\r\n", s),
            Request::Answer(Answer::Code("abc".into()))
        );
        assert_eq!(
            classify(
                "GET /callback?error=access_denied&state=the-state HTTP/1.1\r\n",
                s
            ),
            Request::Answer(Answer::Denied)
        );
        assert_eq!(
            classify("GET /callback?code=abc&state=other HTTP/1.1\r\n", s),
            Request::WrongState
        );
        assert_eq!(
            classify("GET /callback?code=abc HTTP/1.1\r\n", s),
            Request::WrongState
        );
        assert_eq!(
            classify("GET /callback?state=the-state HTTP/1.1\r\n", s),
            Request::WrongState
        );
        assert_eq!(classify("GET /favicon.ico HTTP/1.1\r\n", s), Request::Other);
        assert_eq!(
            classify("GET /callback/x?code=abc&state=the-state HTTP/1.1\r\n", s),
            Request::Other
        );
        assert_eq!(
            classify("POST /callback?code=abc&state=the-state HTTP/1.1\r\n", s),
            Request::Other
        );
        assert_eq!(classify("", s), Request::Other);
    }

    #[tokio::test]
    async fn the_listener_waits_past_strangers_for_the_answer() {
        let (listener, port) = listen().await.unwrap();
        let cancel = tokio::sync::Notify::new();
        let client = tokio::spawn(async move {
            for target in [
                "/favicon.ico",
                "/callback?code=x&state=nope",
                "/callback?code=the-code&state=s1",
            ] {
                let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                s.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                let mut out = String::new();
                s.read_to_string(&mut out).await.unwrap();
                if target.contains("the-code") {
                    assert!(out.starts_with("HTTP/1.1 200"));
                    assert!(out.contains("Referrer-Policy: no-referrer"));
                }
            }
        });
        let answer = wait_for_answer(listener, "s1", &cancel).await;
        client.await.unwrap();
        assert_eq!(answer, Some(Answer::Code("the-code".into())));
    }

    #[tokio::test]
    async fn cancelling_stops_the_wait() {
        let (listener, _) = listen().await.unwrap();
        let cancel = std::sync::Arc::new(tokio::sync::Notify::new());
        let c = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            c.notify_one();
        });
        assert_eq!(wait_for_answer(listener, "s", &cancel).await, None);
    }
}
