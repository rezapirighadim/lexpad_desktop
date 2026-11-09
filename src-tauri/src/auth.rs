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
//!
//! The Mac App Store copy may not listen on a port (its sandbox has no
//! `network.server`; App Review 2.4.5(i)), so it is sent back the other way
//! RFC 8252 offers (§7.1): the private-use scheme `lexpad`, registered in
//! Info.mas.plist, at [`APP_REDIRECT`]. macOS hands the URL to the running
//! app (the deep-link plugin, lib.rs), and [`SchemeWait`] gives it to the
//! sign-in that waits with the same state. Any app could register `lexpad`
//! too; the code it caught would be worthless without the verifier.

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

/// The Mac App Store copy's address (RFC 8252 §7.1). The API takes it only
/// because it is listed exactly in its DESKTOP_APP_REDIRECT_URIS, and the
/// web app's /connect-desktop only this exact string.
pub const APP_REDIRECT: &str = "lexpad://auth/callback";

/// The address the browser is sent back to, for a port we listen on.
pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
}

/// The connect page's address, with every part encoded.
pub fn connect_url(
    app_origin: &str,
    redirect: &str,
    state: &str,
    challenge: &str,
    provider: Option<Provider>,
) -> String {
    let mut url = url::Url::parse(app_origin).expect("APP_ORIGIN is a valid URL");
    url.set_path(crate::config::CONNECT_PATH);
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("redirect_uri", redirect)
            .append_pair("state", state)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256");
        // One tap from the welcome or the player's card: the connect page
        // goes straight to that provider (spec §24.2).
        if let Some(p) = provider {
            q.append_pair("provider", p.as_str());
        }
    }
    url.into()
}

/// A sign-in provider the connect page may go straight to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Google,
    Apple,
}

impl Provider {
    /// Only these two words; anything else is no hint at all.
    pub fn parse(word: Option<&str>) -> Option<Provider> {
        match word {
            Some("google") => Some(Provider::Google),
            Some("apple") => Some(Provider::Apple),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Google => "google",
            Provider::Apple => "apple",
        }
    }
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
    answer_in(&url, state)
}

/// The answer in a callback's query, when it carries our state.
fn answer_in(url: &url::Url, state: &str) -> Request {
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

/// Reads a URL macOS handed over: an answer only when it is exactly our
/// scheme's callback (`lexpad://auth/callback`, no port, no user, no
/// fragment) with our state; any other URL is not ours to answer.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn classify_app_url(raw: &str, state: &str) -> Request {
    let Ok(url) = url::Url::parse(raw) else {
        return Request::Other;
    };
    let mut bare = url.clone();
    bare.set_query(None);
    if bare.as_str() != APP_REDIRECT || url.fragment().is_some() {
        return Request::Other;
    }
    answer_in(&url, state)
}

/// What became of a URL handed to [`SchemeWait::deliver`].
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum Delivered {
    /// The sign-in waiting in this run has its answer.
    Answered,
    /// A sign-in begun before Lexpad was quit (or before macOS started it
    /// again for this link): its code and the verifier kept for it, to trade.
    Resume { code: String, verifier: String },
    /// Our callback, but no sign-in waits with that state (an old tab, a
    /// second click, a cancelled or timed-out sign-in, another copy's link).
    NotAwaited,
    /// Not our callback at all (a film opened with Lexpad, another link).
    NotOurs,
}

/// A sign-in waiting for the browser, kept in the credential store so that
/// a Lexpad quit (or ended) meanwhile can still finish it when macOS starts
/// it again with the callback. The verifier is a secret: only the credential
/// store holds it, and only until `expires` (unix seconds).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Pending {
    pub state: String,
    pub verifier: String,
    pub expires: i64,
}

/// Where [`Pending`] is kept (store.rs: the Keychain; tests: memory).
pub trait PendingStore: Send + Sync {
    fn load(&self) -> Option<Pending>;
    fn save(&self, pending: &Pending);
    fn clear(&self);
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The sign-in a Mac copy waits for through the `lexpad` scheme: its state,
/// where its answer goes in this run, and the same kept for a later run.
/// One at a time; a new one replaces the old, whose wait ends.
pub struct SchemeWait {
    waiting: std::sync::Mutex<Option<(String, tokio::sync::oneshot::Sender<Answer>)>>,
    store: Box<dyn PendingStore>,
}

impl SchemeWait {
    pub fn new(store: Box<dyn PendingStore>) -> Self {
        Self {
            waiting: std::sync::Mutex::new(None),
            store,
        }
    }

    /// Starts waiting for the answer carrying `state`, and keeps `verifier`
    /// for a later run until CONNECT_TIMEOUT has passed.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn begin(&self, state: &str, verifier: &str) -> tokio::sync::oneshot::Receiver<Answer> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Ok(mut w) = self.waiting.lock() {
            *w = Some((state.to_owned(), tx));
        }
        self.store.save(&Pending {
            state: state.to_owned(),
            verifier: verifier.to_owned(),
            expires: now_unix() + CONNECT_TIMEOUT.as_secs() as i64,
        });
        rx
    }

    /// Stops waiting for `state` (answered, timed out or cancelled), here
    /// and in the store, so a late or repeated answer is not taken. A newer
    /// sign-in's wait is left alone.
    pub fn end(&self, state: &str) {
        if let Ok(mut w) = self.waiting.lock() {
            if w.as_ref().is_some_and(|(s, _)| s == state) {
                *w = None;
            }
        }
        if self.store.load().is_some_and(|p| p.state == state) {
            self.store.clear();
        }
    }

    /// Hands a URL macOS opened with Lexpad to the sign-in it answers: the
    /// one waiting in this run, else the one kept from an earlier run (not
    /// expired). Taken once, either way.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn deliver(&self, raw: &str) -> Delivered {
        if classify_app_url(raw, "") == Request::Other {
            return Delivered::NotOurs;
        }
        // Taken out under the lock, answered after it (end() locks again).
        let taken = self.waiting.lock().ok().and_then(|mut w| {
            let state = w.as_ref().map(|(s, _)| s.clone())?;
            match classify_app_url(raw, &state) {
                Request::Answer(answer) => w.take().map(|(s, tx)| (s, tx, answer)),
                _ => None,
            }
        });
        if let Some((state, tx, answer)) = taken {
            self.end(&state);
            return if tx.send(answer).is_ok() {
                Delivered::Answered
            } else {
                Delivered::NotAwaited
            };
        }
        let Some(pending) = self.store.load() else {
            return Delivered::NotAwaited;
        };
        if pending.expires <= now_unix() {
            self.store.clear();
            return Delivered::NotAwaited;
        }
        match classify_app_url(raw, &pending.state) {
            Request::Answer(Answer::Code(code)) => {
                self.store.clear();
                Delivered::Resume {
                    code,
                    verifier: pending.verifier,
                }
            }
            Request::Answer(Answer::Denied) => {
                self.store.clear();
                Delivered::NotAwaited
            }
            _ => Delivered::NotAwaited,
        }
    }
}

/// A [`PendingStore`] in memory, for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryPending(pub std::sync::Arc<std::sync::Mutex<Option<Pending>>>);

#[cfg(test)]
impl PendingStore for MemoryPending {
    fn load(&self) -> Option<Pending> {
        self.0.lock().unwrap().clone()
    }
    fn save(&self, pending: &Pending) {
        *self.0.lock().unwrap() = Some(pending.clone());
    }
    fn clear(&self) {
        *self.0.lock().unwrap() = None;
    }
}

/// The forwarded mark: a callback one Mac copy of Lexpad passed on to the
/// other (both register `lexpad`; macOS gives a link to one of them) is
/// never passed on again.
pub const FORWARDED: &str = "fwd";

/// The callback with the forwarded mark, or `None` when it already has it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn forwarded(raw: &str) -> Option<String> {
    let mut url = url::Url::parse(raw).ok()?;
    if url.query_pairs().any(|(k, _)| k == FORWARDED) {
        return None;
    }
    url.query_pairs_mut().append_pair(FORWARDED, "1");
    Some(url.into())
}

/// Where the browser's answer comes back: a loopback port (the dmg and
/// Windows) or the `lexpad` scheme (the Mac App Store copy).
pub enum Wait {
    Loopback(TcpListener),
    Scheme(tokio::sync::oneshot::Receiver<Answer>),
}

/// Waits for the answer either way. `None` when `cancel` fires first (or
/// a newer sign-in replaced this one); the caller bounds the wait.
pub async fn wait(wait: Wait, state: &str, cancel: &tokio::sync::Notify) -> Option<Answer> {
    match wait {
        Wait::Loopback(listener) => wait_for_answer(listener, state, cancel).await,
        Wait::Scheme(rx) => tokio::select! {
            answer = rx => answer.ok(),
            () = cancel.notified() => None,
        },
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
        let url = connect_url(
            "https://app.lexpad.app",
            &redirect_uri(53682),
            "st",
            "ch",
            None,
        );
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
        assert!(!q.contains_key("provider"));
    }

    #[test]
    fn a_provider_hint_goes_only_as_one_of_two_words() {
        let url = connect_url(
            "https://app.lexpad.app",
            &redirect_uri(1),
            "st",
            "ch",
            Provider::parse(Some("google")),
        );
        let q: std::collections::HashMap<_, _> = url::Url::parse(&url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        assert_eq!(q["provider"], "google");
        assert_eq!(Provider::parse(Some("apple")), Some(Provider::Apple));
        assert_eq!(Provider::parse(Some("github")), None);
        assert_eq!(Provider::parse(Some("google&x=1")), None);
        assert_eq!(Provider::parse(None), None);
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

    #[test]
    fn only_our_scheme_callback_with_our_state_is_an_answer() {
        let s = "the-state";
        assert_eq!(
            classify_app_url("lexpad://auth/callback?code=abc&state=the-state", s),
            Request::Answer(Answer::Code("abc".into()))
        );
        assert_eq!(
            classify_app_url(
                "lexpad://auth/callback?error=access_denied&state=the-state",
                s
            ),
            Request::Answer(Answer::Denied)
        );
        assert_eq!(
            classify_app_url("lexpad://auth/callback?code=abc&state=other", s),
            Request::WrongState
        );
        assert_eq!(
            classify_app_url("lexpad://auth/callback?code=abc", s),
            Request::WrongState
        );
        for other in [
            "lexpad://auth/callback/x?code=abc&state=the-state",
            "lexpad://auth/other?code=abc&state=the-state",
            "lexpad://evil/callback?code=abc&state=the-state",
            "lexpad://auth:8080/callback?code=abc&state=the-state",
            "lexpad://me@auth/callback?code=abc&state=the-state",
            "lexpad://auth/callback?code=abc&state=the-state#x",
            "lexpadx://auth/callback?code=abc&state=the-state",
            "http://127.0.0.1:1/callback?code=abc&state=the-state",
            "file:///Users/me/Film.mkv",
            "not a url",
        ] {
            assert_eq!(classify_app_url(other, s), Request::Other, "{other}");
        }
    }

    fn scheme_wait() -> (
        SchemeWait,
        std::sync::Arc<std::sync::Mutex<Option<Pending>>>,
    ) {
        let kept = std::sync::Arc::new(std::sync::Mutex::new(None));
        (SchemeWait::new(Box::new(MemoryPending(kept.clone()))), kept)
    }

    #[test]
    fn the_scheme_answer_reaches_only_the_waiting_sign_in_once() {
        let (wait, kept) = scheme_wait();
        // Nothing waits: our callback is ignored, a film is not ours.
        assert_eq!(
            wait.deliver("lexpad://auth/callback?code=a&state=s1"),
            Delivered::NotAwaited
        );
        assert_eq!(wait.deliver("file:///tmp/a.mkv"), Delivered::NotOurs);

        let mut rx = wait.begin("s1", "v1");
        assert_eq!(kept.lock().unwrap().as_ref().unwrap().verifier, "v1");
        assert_eq!(
            wait.deliver("lexpad://auth/callback?code=a&state=s2"),
            Delivered::NotAwaited
        );
        assert!(rx.try_recv().is_err());
        assert_eq!(
            wait.deliver("lexpad://auth/callback?code=the-code&state=s1"),
            Delivered::Answered
        );
        assert_eq!(rx.try_recv().unwrap(), Answer::Code("the-code".into()));
        // Answered: nothing is kept for a later run, and a second click on
        // the same link finds nothing waiting.
        assert!(kept.lock().unwrap().is_none());
        assert_eq!(
            wait.deliver("lexpad://auth/callback?code=the-code&state=s1"),
            Delivered::NotAwaited
        );

        // A newer sign-in replaces the old one, whose wait ends.
        let old = wait.begin("old", "vo");
        let mut new = wait.begin("new", "vn");
        assert!(old.blocking_recv().is_err());
        // Ending the old one leaves the new one waiting, here and kept.
        wait.end("old");
        assert_eq!(kept.lock().unwrap().as_ref().unwrap().state, "new");
        // Cancel in the browser: the answer is Denied.
        assert_eq!(
            wait.deliver("lexpad://auth/callback?error=access_denied&state=new"),
            Delivered::Answered
        );
        assert_eq!(new.try_recv().unwrap(), Answer::Denied);

        // Timed out: a late answer is not taken, here or from the store.
        let _late = wait.begin("late", "vl");
        wait.end("late");
        assert!(kept.lock().unwrap().is_none());
        assert_eq!(
            wait.deliver("lexpad://auth/callback?code=a&state=late"),
            Delivered::NotAwaited
        );
    }

    #[test]
    fn a_sign_in_survives_lexpad_being_quit_until_it_expires() {
        let (first_run, kept) = scheme_wait();
        let _rx = first_run.begin("s", "the-verifier");
        drop(first_run);
        // macOS starts Lexpad again for the link: the new run resumes it.
        let second_run = SchemeWait::new(Box::new(MemoryPending(kept.clone())));
        assert_eq!(
            second_run.deliver("lexpad://auth/callback?code=c&state=other"),
            Delivered::NotAwaited
        );
        assert_eq!(
            second_run.deliver("lexpad://auth/callback?code=c&state=s"),
            Delivered::Resume {
                code: "c".into(),
                verifier: "the-verifier".into()
            }
        );
        // Once only.
        assert_eq!(
            second_run.deliver("lexpad://auth/callback?code=c&state=s"),
            Delivered::NotAwaited
        );
        // A cancel answered after a restart ends it too.
        *kept.lock().unwrap() = Some(Pending {
            state: "d".into(),
            verifier: "v".into(),
            expires: now_unix() + 60,
        });
        assert_eq!(
            second_run.deliver("lexpad://auth/callback?error=access_denied&state=d"),
            Delivered::NotAwaited
        );
        assert!(kept.lock().unwrap().is_none());
        // Expired: never resumed, and forgotten.
        *kept.lock().unwrap() = Some(Pending {
            state: "e".into(),
            verifier: "v".into(),
            expires: now_unix() - 1,
        });
        assert_eq!(
            second_run.deliver("lexpad://auth/callback?code=c&state=e"),
            Delivered::NotAwaited
        );
        assert!(kept.lock().unwrap().is_none());
    }

    #[test]
    fn a_callback_is_passed_to_the_other_copy_once() {
        let once = forwarded("lexpad://auth/callback?code=c&state=s").unwrap();
        assert_eq!(once, "lexpad://auth/callback?code=c&state=s&fwd=1");
        assert_eq!(forwarded(&once), None);
        // The mark does not stop the other copy from taking it.
        let (wait, _) = scheme_wait();
        let mut rx = wait.begin("s", "v");
        assert_eq!(wait.deliver(&once), Delivered::Answered);
        assert_eq!(rx.try_recv().unwrap(), Answer::Code("c".into()));
    }

    #[tokio::test]
    async fn the_scheme_wait_ends_with_the_answer_or_a_cancel() {
        let (sw, _) = scheme_wait();
        let cancel = tokio::sync::Notify::new();
        let rx = sw.begin("s", "v");
        sw.deliver("lexpad://auth/callback?code=c&state=s");
        assert_eq!(
            wait(Wait::Scheme(rx), "s", &cancel).await,
            Some(Answer::Code("c".into()))
        );
        let rx = sw.begin("t", "v");
        cancel.notify_one();
        assert_eq!(wait(Wait::Scheme(rx), "t", &cancel).await, None);
    }

    #[test]
    fn the_app_redirect_is_what_the_api_and_the_web_app_take() {
        // lexpad_back DESKTOP_APP_REDIRECT_URIS and lexpad_front
        // desktopRequest.ts APP_CALLBACK hold this exact string.
        assert_eq!(APP_REDIRECT, "lexpad://auth/callback");
        let url = connect_url("https://app.lexpad.app", APP_REDIRECT, "st", "ch", None);
        let q: std::collections::HashMap<_, _> = url::Url::parse(&url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        assert_eq!(q["redirect_uri"], "lexpad://auth/callback");
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
