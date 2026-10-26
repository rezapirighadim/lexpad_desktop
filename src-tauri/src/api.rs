//! The API client. The only part of the app that holds tokens or talks to
//! the API: the windows ask through commands (commands.rs) and get cards,
//! notebooks and typed failures back, never a token.
//!
//! It mirrors the browser extension's client (`src/lib/api.ts` in
//! `lexpad_extension`): a bearer token, refreshed before it expires and once
//! more on a 401, one refresh at a time; the API's problem+json turned into
//! failures the popup can phrase.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::store::{Saved, SessionStore, User};

/// What can go wrong, in words the popup can say something about. The same
/// vocabulary as the extension's `Failure`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    SignedOut,
    Offline,
    AiUnavailable,
    Duplicate,
    NoNotebook,
    Error,
}

impl Failure {
    pub fn as_str(self) -> &'static str {
        match self {
            Failure::SignedOut => "signed_out",
            Failure::Offline => "offline",
            Failure::AiUnavailable => "ai_unavailable",
            Failure::Duplicate => "duplicate",
            Failure::NoNotebook => "no_notebook",
            Failure::Error => "error",
        }
    }
}

/// A refresh this close to expiry happens before the request rather than
/// after a 401, so a lookup never starts with a token about to lapse.
const EXPIRY_MARGIN: Duration = Duration::from_secs(30);
/// The longest a response to the main window may go quiet before it is
/// treated as cut off: longer than the API's own keep-alive on a stream.
const FORWARD_SILENCE: Duration = Duration::from_secs(90);
/// How often a queued AI job is asked about, and how many times.
const POLL_EVERY: Duration = Duration::from_millis(1200);
const POLL_TIMES: usize = 40;
/// How long the server's answer on the activity headers is kept
/// (`activity` in GET /config, reports/specs/2026-10-26-activity-tracking.md
/// §2.1), and how soon a failed read is asked again. Off until it has said.
const ACTIVITY_KEEP: Duration = Duration::from_secs(6 * 60 * 60);
const ACTIVITY_RETRY: Duration = Duration::from_secs(5 * 60);

/// Where in the app a call comes from (§4 of the activity spec), sent as
/// `X-Lexpad-Surface` while the server says it takes it. A fixed name:
/// nothing the person typed or selected ever goes in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// The global shortcut's popup (the selection, or a word typed in it).
    Shortcut,
    /// The macOS Services menu, "Add to Lexpad".
    Services,
    /// The menu-bar / tray panel's "Add a word" box.
    Tray,
}

impl Surface {
    pub fn as_str(self) -> &'static str {
        match self {
            Surface::Shortcut => "desktop.shortcut",
            Surface::Services => "desktop.services",
            Surface::Tray => "desktop.tray",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthSession {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
    user: Option<ApiUser>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiUser {
    id: String,
    email: String,
    display_name: String,
}

impl From<ApiUser> for User {
    fn from(u: ApiUser) -> Self {
        User {
            id: u.id,
            email: u.email,
            display_name: u.display_name,
        }
    }
}

/// The device this app reports when it signs in.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub platform: &'static str,
    pub device_id: String,
    pub device_name: String,
    pub app_version: String,
}

struct Inner {
    /// None until the store has been read once.
    saved: Option<Option<Saved>>,
    access: Option<(String, Instant)>,
}

/// The longest a /config read for the activity answer may take; it runs
/// in the background, so this only bounds a hung one.
const ACTIVITY_TIMEOUT: Duration = Duration::from_secs(15);

/// The server's answer on the activity headers (`activity` in GET /config,
/// reports/specs/2026-10-26-activity-tracking.md §2.1). Per process and per
/// API origin: it says what the server takes, nothing about who is signed
/// in, so it outlives a sign-out.
struct Gate {
    http: reqwest::Client,
    url: String,
    /// The last answer and until when it holds; None until one arrives.
    cache: std::sync::Mutex<Option<(bool, Instant)>>,
    /// One read at a time; a caller that waited on it finds the answer fresh.
    flight: Mutex<()>,
    /// A background refresh has been started and not finished.
    kicked: std::sync::atomic::AtomicBool,
}

impl Gate {
    fn mark_stale(&self) {
        if let Some((_, until)) = self.cache.lock().expect("activity lock").as_mut() {
            *until = Instant::now();
        }
    }

    fn fresh(&self) -> Option<bool> {
        match *self.cache.lock().expect("activity lock") {
            Some((on, until)) if Instant::now() < until => Some(on),
            _ => None,
        }
    }

    async fn refresh(&self) -> bool {
        let _one = self.flight.lock().await;
        if let Some(on) = self.fresh() {
            return on;
        }
        let answer = async {
            let res = self.http.get(&self.url).send().await.ok()?;
            if !res.status().is_success() {
                return None;
            }
            let body: Value = res.json().await.ok()?;
            Some(body.get("activity").and_then(Value::as_bool) == Some(true))
        }
        .await;
        let kept = match answer {
            Some(on) => (on, Instant::now() + ACTIVITY_KEEP),
            None => (false, Instant::now() + ACTIVITY_RETRY),
        };
        *self.cache.lock().expect("activity lock") = Some(kept);
        kept.0
    }
}

pub struct Api {
    http: reqwest::Client,
    /// For the main window's calls (`forward`): no limit on the whole
    /// exchange, because Lex's answers stream for as long as they take, but
    /// a limit on connecting and on each silence.
    stream_http: reqwest::Client,
    base: String,
    store: Arc<dyn SessionStore>,
    inner: Mutex<Inner>,
    /// The server's last answer on the activity headers (see `Gate`).
    activity: Arc<Gate>,
}

impl Api {
    pub fn new(api_origin: &str, store: Arc<dyn SessionStore>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(crate::config::user_agent())
            .build()
            .expect("HTTP client");
        let stream_http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(FORWARD_SILENCE)
            .user_agent(crate::config::user_agent())
            .build()
            .expect("HTTP client");
        Self {
            http,
            stream_http,
            base: format!("{}/api/v1", api_origin.trim_end_matches('/')),
            store,
            inner: Mutex::new(Inner {
                saved: None,
                access: None,
            }),
            activity: Arc::new(Gate {
                http: reqwest::Client::builder()
                    .timeout(ACTIVITY_TIMEOUT)
                    .user_agent(crate::config::user_agent())
                    .build()
                    .expect("HTTP client"),
                url: format!("{}/api/v1/config", api_origin.trim_end_matches('/')),
                cache: std::sync::Mutex::new(None),
                flight: Mutex::new(()),
                kicked: std::sync::atomic::AtomicBool::new(false),
            }),
        }
    }

    fn saved<'a>(&self, inner: &'a mut Inner) -> Option<&'a Saved> {
        if inner.saved.is_none() {
            inner.saved = Some(self.store.load());
        }
        inner.saved.as_ref().and_then(|s| s.as_ref())
    }

    /// Who is signed in, from the saved session; no network.
    pub async fn user(&self) -> Option<User> {
        let mut inner = self.inner.lock().await;
        self.saved(&mut inner).map(|s| s.user.clone())
    }

    /// Trades the one-time code from the browser, with the PKCE verifier, for
    /// this app's own session, and keeps it.
    pub async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
        device: &Device,
    ) -> Result<User, Failure> {
        let body = json!({ "code": code, "codeVerifier": verifier, "redirectUri": redirect_uri, "device": device });
        let res = self
            .http
            .post(format!("{}/auth/desktop/token", self.base))
            .json(&body)
            .send()
            .await
            .map_err(|_| Failure::Offline)?;
        if !res.status().is_success() {
            log::warn!("desktop code refused: {}", res.status());
            return Err(Failure::Error);
        }
        let session: AuthSession = res.json().await.map_err(|_| Failure::Error)?;
        let (Some(refresh), Some(user)) = (session.refresh_token, session.user) else {
            return Err(Failure::Error);
        };
        let saved = Saved {
            refresh_token: refresh,
            user: user.into(),
        };
        if let Err(e) = self.store.save(&saved) {
            // Kept for this run; the next start will ask to connect again.
            log::warn!("could not save the session: {e}");
        }
        let mut inner = self.inner.lock().await;
        inner.access = Some((
            session.access_token,
            Instant::now() + Duration::from_secs(session.expires_in),
        ));
        inner.saved = Some(Some(saved.clone()));
        drop(inner);
        // A new sign-in: ask the server again, in the background; the last
        // answer stands until then.
        self.activity.mark_stale();
        self.kick_activity();
        Ok(saved.user)
    }

    /// Signs out: revokes the session on the server, then forgets it here, in
    /// that order. Offline or already gone, forgetting locally is what matters.
    pub async fn sign_out(&self) {
        if let Ok(token) = self.token().await {
            let _ = self
                .http
                .post(format!("{}/auth/logout", self.base))
                .bearer_auth(token)
                .send()
                .await;
        }
        self.forget().await;
    }

    async fn forget(&self) {
        self.store.clear();
        let mut inner = self.inner.lock().await;
        inner.saved = Some(None);
        inner.access = None;
    }

    /// A usable access token, refreshed first when it is missing or about to
    /// lapse. The lock makes it one refresh at a time: whoever waits behind a
    /// refresh finds the new token and uses it.
    async fn token(&self) -> Result<String, Failure> {
        let mut inner = self.inner.lock().await;
        if self.saved(&mut inner).is_none() {
            return Err(Failure::SignedOut);
        }
        if let Some((token, expires)) = &inner.access {
            if *expires > Instant::now() + EXPIRY_MARGIN {
                return Ok(token.clone());
            }
        }
        self.refresh_locked(&mut inner).await
    }

    /// Refreshes after a 401 on `used`, unless somebody else already has.
    async fn token_after_401(&self, used: &str) -> Result<String, Failure> {
        let mut inner = self.inner.lock().await;
        if let Some((token, _)) = &inner.access {
            if token != used {
                return Ok(token.clone());
            }
        }
        self.refresh_locked(&mut inner).await
    }

    async fn refresh_locked(&self, inner: &mut Inner) -> Result<String, Failure> {
        let Some(saved) = self.saved(inner).cloned() else {
            return Err(Failure::SignedOut);
        };
        let res = self
            .http
            .post(format!("{}/auth/refresh", self.base))
            .json(&json!({ "refreshToken": saved.refresh_token }))
            .send()
            .await
            // Offline keeps the session: the next call tries again.
            .map_err(|_| Failure::Offline)?;
        let status = res.status().as_u16();
        if status == 401 || status == 403 {
            // A refused refresh ends the session for good.
            self.store.clear();
            inner.saved = Some(None);
            inner.access = None;
            return Err(Failure::SignedOut);
        }
        if !res.status().is_success() {
            return Err(Failure::Error);
        }
        let next: AuthSession = res.json().await.map_err(|_| Failure::Error)?;
        let renewed = Saved {
            refresh_token: next.refresh_token.unwrap_or(saved.refresh_token),
            user: next.user.map(User::from).unwrap_or(saved.user),
        };
        if let Err(e) = self.store.save(&renewed) {
            log::warn!("could not save the refreshed session: {e}");
        }
        inner.saved = Some(Some(renewed));
        inner.access = Some((
            next.access_token.clone(),
            Instant::now() + Duration::from_secs(next.expires_in),
        ));
        Ok(next.access_token)
    }

    /// Whether the server takes the activity headers, from the last answer
    /// only: never waits for the network, so a lookup or an add is never
    /// slowed by /config. Unknown is "no". A missing or stale answer starts a
    /// refresh in the background (`kick_activity`) for the calls after.
    pub fn activity_now(&self) -> bool {
        let cached = *self.activity.cache.lock().expect("activity lock");
        match cached {
            Some((on, until)) if Instant::now() < until => on,
            Some((on, _)) => {
                self.kick_activity();
                on
            }
            None => {
                self.kick_activity();
                false
            }
        }
    }

    /// Starts a refresh of the activity answer without waiting for it: at
    /// start, after signing in, and when the answer is stale. At most one is
    /// started at a time.
    pub fn kick_activity(&self) {
        use std::sync::atomic::Ordering;
        if self.activity.kicked.swap(true, Ordering::SeqCst) {
            return;
        }
        let gate = self.activity.clone();
        tauri::async_runtime::spawn(async move {
            gate.refresh().await;
            gate.kicked.store(false, Ordering::SeqCst);
        });
    }

    /// Asks GET /config now and keeps the answer; single-flight, so callers
    /// that arrive together share one request. The app only kicks it
    /// (`kick_activity`); tests await it to know the answer has landed.
    #[cfg(test)]
    pub async fn refresh_activity(&self) -> bool {
        self.activity.refresh().await
    }

    /// One request with the bearer token, refreshed once on a 401.
    pub(crate) async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
        idempotent: bool,
    ) -> Result<Value, Failure> {
        self.request_from(method, path, body, idempotent, None)
            .await
    }

    /// The same, saying where in the app the person asked for it, while the
    /// server takes that (`activity_now`, never awaited).
    pub(crate) async fn request_from(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
        idempotent: bool,
        surface: Option<Surface>,
    ) -> Result<Value, Failure> {
        let surface = match surface {
            Some(s) if self.activity_now() => Some(s),
            _ => None,
        };
        let mut token = self.token().await?;
        let key = idempotent.then(|| uuid::Uuid::new_v4().to_string());
        for attempt in 0..2 {
            let mut req = self
                .http
                .request(method.clone(), format!("{}{}", self.base, path))
                .bearer_auth(&token);
            if let Some(b) = body {
                req = req.json(b);
            }
            if let Some(k) = &key {
                req = req.header("Idempotency-Key", k);
            }
            if let Some(s) = surface {
                req = req.header("X-Lexpad-Surface", s.as_str());
            }
            let res = req.send().await.map_err(|_| Failure::Offline)?;
            let status = res.status().as_u16();
            if status == 401 && attempt == 0 {
                token = self.token_after_401(&token).await?;
                continue;
            }
            if status == 204 {
                return Ok(Value::Null);
            }
            if res.status().is_success() {
                return res.json().await.map_err(|_| Failure::Error);
            }
            let problem: Value = res.json().await.unwrap_or(Value::Null);
            return Err(failure(
                status,
                problem.get("code").and_then(Value::as_str).unwrap_or(""),
            ));
        }
        Err(Failure::SignedOut)
    }

    /// Sends a request from the main window (already checked by `proxy`)
    /// with the session, refreshed once on a 401, and hands back the
    /// response for its body to be streamed. Without a session it goes
    /// without one, as an anonymous call from a browser would. A 401 that
    /// survives a refusal to refresh comes back as it is: the session has
    /// ended, and the caller tells the windows.
    pub async fn forward(&self, req: &crate::proxy::Checked) -> Result<reqwest::Response, Failure> {
        let mut token = match self.token().await {
            Ok(t) => Some(t),
            Err(Failure::SignedOut) => None,
            Err(e) => return Err(e),
        };
        for attempt in 0..2 {
            let mut out = self
                .stream_http
                .request(req.method.clone(), req.url.clone());
            for (name, value) in &req.headers {
                out = out.header(name.as_str(), value.as_str());
            }
            if let Some(t) = &token {
                out = out.bearer_auth(t);
            }
            if let Some(body) = &req.body {
                out = out.body(body.clone());
            }
            let res = out.send().await.map_err(|_| Failure::Offline)?;
            if res.status().as_u16() == 401 && attempt == 0 {
                if let Some(used) = token.clone() {
                    match self.token_after_401(&used).await {
                        Ok(next) => {
                            token = Some(next);
                            continue;
                        }
                        Err(Failure::SignedOut) => return Ok(res),
                        Err(e) => return Err(e),
                    }
                }
            }
            return Ok(res);
        }
        Err(Failure::SignedOut)
    }

    /// The account's notebooks, as the API sends them.
    pub async fn notebooks(&self) -> Result<Vec<Value>, Failure> {
        let body = self
            .request(reqwest::Method::GET, "/notebooks", None, false)
            .await?;
        Ok(match body {
            Value::Array(items) => items,
            Value::Object(mut o) => match o.remove("items") {
                Some(Value::Array(items)) => items,
                _ => vec![],
            },
            _ => vec![],
        })
    }

    /// Asks the model for one card and waits for it. The sentence the word was
    /// met in goes along as the hint, so the meaning fits the context.
    pub async fn lookup(
        &self,
        notebook_id: &str,
        headword: &str,
        hint: Option<&str>,
        surface: Option<Surface>,
    ) -> Result<Value, Failure> {
        let mut body = json!({ "notebookId": notebook_id, "words": [headword] });
        if let Some(h) = hint {
            body["hint"] = json!(h);
        }
        let mut job = self
            .request_from(
                reqwest::Method::POST,
                "/ai/cards",
                Some(&body),
                true,
                surface,
            )
            .await?;
        for _ in 0..POLL_TIMES {
            let status = job.get("status").and_then(Value::as_str).unwrap_or("");
            if status != "queued" && status != "running" {
                break;
            }
            tokio::time::sleep(POLL_EVERY).await;
            let id = job
                .get("id")
                .and_then(Value::as_str)
                .ok_or(Failure::AiUnavailable)?
                .to_owned();
            job = self
                .request(reqwest::Method::GET, &format!("/ai/jobs/{id}"), None, false)
                .await?;
        }
        if job.get("status").and_then(Value::as_str) != Some("done") {
            return Err(Failure::AiUnavailable);
        }
        job.get("cards")
            .and_then(Value::as_array)
            .and_then(|cards| cards.first().cloned())
            .ok_or(Failure::AiUnavailable)
    }

    /// Adds one word to a notebook and returns its id.
    pub async fn add_word(
        &self,
        notebook_id: &str,
        word: &Value,
        surface: Option<Surface>,
    ) -> Result<String, Failure> {
        let body = json!({ "items": [word] });
        let created = self
            .request_from(
                reqwest::Method::POST,
                &format!("/notebooks/{notebook_id}/words"),
                Some(&body),
                true,
                surface,
            )
            .await?;
        created
            .as_array()
            .and_then(|a| a.first())
            .and_then(|w| w.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(Failure::Error)
    }
}

/// The API's refusal, as a failure the popup can phrase. Limits are never
/// announced: a refused AI call is only "not available right now".
fn failure(status: u16, code: &str) -> Failure {
    match (status, code) {
        (409, _) | (_, "duplicate_headword") => Failure::Duplicate,
        (429 | 503, _) | (_, "email_unverified" | "ai_disabled" | "ai_quota_exceeded") => {
            Failure::AiUnavailable
        }
        (401, _) => Failure::SignedOut,
        (404, "notebook_not_found") => Failure::NoNotebook,
        _ => Failure::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Memory;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A tiny HTTP server: `handle(method_and_path, body)` answers (status, json).
    async fn serve<F>(handle: F) -> String
    where
        F: Fn(&str, &str, &str) -> (u16, String) + Send + Sync + 'static,
    {
        serve_head(move |line, head, body| {
            let auth = head
                .lines()
                .find_map(|l| l.strip_prefix("authorization: Bearer "))
                .unwrap_or("");
            handle(line, auth, body)
        })
        .await
    }

    /// The same, handing over the whole request head (lower-case header names).
    async fn serve_head<F>(handle: F) -> String
    where
        F: Fn(&str, &str, &str) -> (u16, String) + Send + Sync + 'static,
    {
        serve_slow(move |line, head, body| {
            let (status, out) = handle(line, head, body);
            (status, out, 0)
        })
        .await
    }

    /// The same, answering after the given milliseconds (a slow or hung server).
    async fn serve_slow<F>(handle: F) -> String
    where
        F: Fn(&str, &str, &str) -> (u16, String, u64) + Send + Sync + 'static,
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = Arc::new(handle);
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else {
                    return;
                };
                let handle = handle.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    loop {
                        let n = s.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        let text = String::from_utf8_lossy(&buf).to_string();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let len = text[..end]
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length: ")
                                        .map(|v| v.trim().parse::<usize>().unwrap())
                                })
                                .unwrap_or(0);
                            if buf.len() >= end + 4 + len {
                                let head = &text[..end];
                                let line = head.lines().next().unwrap_or_default();
                                let body = &text[end + 4..end + 4 + len];
                                let (status, out, delay) = handle(line, head, body);
                                if delay > 0 {
                                    tokio::time::sleep(Duration::from_millis(delay)).await;
                                }
                                let resp = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len());
                                let _ = s.write_all(resp.as_bytes()).await;
                                return;
                            }
                        }
                    }
                });
            }
        });
        format!("http://{addr}")
    }

    fn signed_in() -> Arc<Memory> {
        let store = Arc::new(Memory::default());
        store
            .save(&Saved {
                refresh_token: "rt-1".into(),
                user: User {
                    id: "u".into(),
                    email: "e@example.com".into(),
                    display_name: "E".into(),
                },
            })
            .unwrap();
        store
    }

    const SESSION: &str = r#"{"accessToken":"at-2","expiresIn":900,"refreshToken":"rt-2","user":{"id":"u","email":"e@example.com","displayName":"E"}}"#;

    #[tokio::test]
    async fn refreshes_once_for_many_callers_and_rotates_the_saved_token() {
        let refreshes = Arc::new(AtomicUsize::new(0));
        let r = refreshes.clone();
        let base = serve(move |line, auth, _| {
            if line.starts_with("POST /api/v1/auth/refresh") {
                r.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(50));
                return (200, SESSION.into());
            }
            if auth == "at-2" {
                (200, r#"{"items":[{"id":"n1"}]}"#.into())
            } else {
                (401, "{}".into())
            }
        })
        .await;
        let store = signed_in();
        let api = Arc::new(Api::new(&base, store.clone()));
        let calls: Vec<_> = (0..5)
            .map(|_| {
                let a = api.clone();
                tokio::spawn(async move { a.notebooks().await })
            })
            .collect();
        for c in calls {
            assert_eq!(c.await.unwrap().unwrap().len(), 1);
        }
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(store.load().unwrap().refresh_token, "rt-2");
    }

    #[tokio::test]
    async fn a_refused_refresh_signs_out() {
        let base = serve(|_, _, _| (401, "{}".into())).await;
        let store = signed_in();
        let api = Api::new(&base, store.clone());
        assert_eq!(api.notebooks().await.unwrap_err(), Failure::SignedOut);
        assert!(store.load().is_none());
        assert!(api.user().await.is_none());
    }

    #[tokio::test]
    async fn a_401_refreshes_and_retries_once() {
        let seen = Arc::new(AtomicUsize::new(0));
        let s = seen.clone();
        let base = serve(move |line, auth, _| {
            if line.starts_with("POST /api/v1/auth/refresh") {
                return (200, SESSION.into());
            }
            s.fetch_add(1, Ordering::SeqCst);
            if auth == "at-2" {
                (200, "[]".into())
            } else {
                (401, "{}".into())
            }
        })
        .await;
        let api = Api::new(&base, signed_in());
        // An access token the server no longer takes.
        api.inner.lock().await.access =
            Some(("stale".into(), Instant::now() + Duration::from_secs(600)));
        assert!(api.notebooks().await.unwrap().is_empty());
        assert_eq!(seen.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn failures_are_phrased_like_the_extension() {
        assert_eq!(failure(409, ""), Failure::Duplicate);
        assert_eq!(failure(429, "ai_quota_exceeded"), Failure::AiUnavailable);
        assert_eq!(failure(403, "email_unverified"), Failure::AiUnavailable);
        assert_eq!(failure(401, ""), Failure::SignedOut);
        assert_eq!(failure(500, ""), Failure::Error);
        let api = Api::new("http://127.0.0.1:9", Arc::new(Memory::default()));
        assert_eq!(api.notebooks().await.unwrap_err(), Failure::SignedOut);
    }

    fn checked(base: &str, path: &str) -> crate::proxy::Checked {
        crate::proxy::check(
            base,
            &crate::proxy::Request {
                method: "GET".into(),
                url: format!("{base}/api/v1{path}"),
                headers: vec![("Authorization".into(), "Bearer forged".into())],
                body: None,
            },
        )
        .unwrap()
    }

    #[tokio::test]
    async fn forwarding_adds_the_session_and_never_a_token_from_the_window() {
        let base = serve(|line, auth, _| {
            if line.starts_with("POST /api/v1/auth/refresh") {
                return (200, SESSION.into());
            }
            assert!(line.starts_with("GET /api/v1/me "));
            // The window's own Authorization header was dropped.
            assert_eq!(auth, "at-2");
            (200, r#"{"id":"u"}"#.into())
        })
        .await;
        let api = Api::new(&base, signed_in());
        let res = api.forward(&checked(&base, "/me")).await.unwrap();
        assert_eq!(res.status().as_u16(), 200);
        assert_eq!(res.text().await.unwrap(), r#"{"id":"u"}"#);
    }

    #[tokio::test]
    async fn forwarding_without_a_session_goes_anonymous_and_a_refused_refresh_ends_it() {
        let base = serve(|_, auth, _| {
            assert_eq!(auth, "");
            (401, "{}".into())
        })
        .await;
        let api = Api::new(&base, Arc::new(Memory::default()));
        let res = api.forward(&checked(&base, "/me")).await.unwrap();
        assert_eq!(res.status().as_u16(), 401);

        let base = serve(|_, _, _| (401, "{}".into())).await;
        let store = signed_in();
        let api = Api::new(&base, store.clone());
        let res = api.forward(&checked(&base, "/words")).await.unwrap();
        assert_eq!(res.status().as_u16(), 401);
        assert!(store.load().is_none());
    }

    #[tokio::test]
    async fn the_code_exchange_keeps_the_session() {
        let base = serve(|line, _, body| {
            assert!(line.starts_with("POST /api/v1/auth/desktop/token"));
            let v: Value = serde_json::from_str(body).unwrap();
            assert_eq!(v["device"]["platform"], "desktop");
            assert_eq!(v["codeVerifier"], "verifier");
            (200, SESSION.into())
        })
        .await;
        let store = Arc::new(Memory::default());
        let api = Api::new(&base, store.clone());
        let device = Device {
            platform: "desktop",
            device_id: "d".into(),
            device_name: "macOS".into(),
            app_version: "0.1.0".into(),
        };
        let user = api
            .exchange_code("code", "verifier", "http://127.0.0.1:1/callback", &device)
            .await
            .unwrap();
        assert_eq!(user.email, "e@example.com");
        assert_eq!(store.load().unwrap().refresh_token, "rt-2");
    }

    #[tokio::test]
    async fn the_surface_goes_only_while_the_server_takes_it() {
        for (config, expect) in [
            (r#"{"activity":true}"#, Some("desktop.shortcut")),
            (r#"{"text":{"read":1,"paste":1}}"#, None),
        ] {
            let seen: Arc<std::sync::Mutex<Vec<Option<String>>>> = Arc::default();
            let s = seen.clone();
            let configs = Arc::new(AtomicUsize::new(0));
            let c = configs.clone();
            let config = config.to_string();
            let base = serve_head(move |line, head, _| {
                if line.starts_with("POST /api/v1/auth/refresh") {
                    return (200, SESSION.into());
                }
                if line.starts_with("GET /api/v1/config") {
                    c.fetch_add(1, Ordering::SeqCst);
                    return (200, config.clone());
                }
                let surface = head
                    .lines()
                    .find_map(|l| l.strip_prefix("x-lexpad-surface: "))
                    .map(str::to_owned);
                s.lock().unwrap().push(surface);
                (200, r#"[{"id":"w1"}]"#.into())
            })
            .await;
            let api = Api::new(&base, signed_in());
            // As at start: the answer is read once, before any word.
            api.refresh_activity().await;
            let word = json!({ "headword": "hello" });
            assert_eq!(
                api.add_word("n1", &word, Some(Surface::Shortcut))
                    .await
                    .unwrap(),
                "w1"
            );
            api.add_word("n1", &word, Some(Surface::Shortcut))
                .await
                .unwrap();
            // Untagged calls never carry it, whatever the server says.
            api.add_word("n1", &word, None).await.unwrap();
            let seen = seen.lock().unwrap().clone();
            assert_eq!(
                seen,
                [expect.map(str::to_owned), expect.map(str::to_owned), None]
            );
            // The answer is kept: one read for all three calls.
            assert_eq!(configs.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn a_config_that_cannot_be_read_is_off() {
        let base = serve(|_, _, _| (503, "{}".into())).await;
        let api = Api::new(&base, Arc::new(Memory::default()));
        assert!(!api.refresh_activity().await);
        assert!(!api.activity_now());
    }

    /// A server for the §10 gate tests: answers /config with `config` (None =
    /// a 500), a done AI job, and a created word; records every other
    /// request's activity headers, and how often /config was asked.
    async fn gate_server(
        config: Option<&'static str>,
    ) -> (String, Arc<std::sync::Mutex<Vec<String>>>, Arc<AtomicUsize>) {
        let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
        let configs = Arc::new(AtomicUsize::new(0));
        let (s, c) = (seen.clone(), configs.clone());
        let base = serve_head(move |line, head, _| {
            if line.starts_with("POST /api/v1/auth/refresh") {
                return (200, SESSION.into());
            }
            if line.starts_with("GET /api/v1/config") {
                c.fetch_add(1, Ordering::SeqCst);
                return match config {
                    Some(body) => (200, body.into()),
                    None => (500, "{}".into()),
                };
            }
            let tags: Vec<&str> = head
                .lines()
                .filter(|l| l.starts_with("x-lexpad-"))
                .collect();
            s.lock().unwrap().push(format!(
                "{} {}",
                line.split(' ').take(2).collect::<Vec<_>>().join(" "),
                tags.join(",")
            ));
            if line.starts_with("POST /api/v1/ai/cards") {
                return (
                    200,
                    r#"{"id":"j1","status":"done","cards":[{"headword":"hello"}]}"#.into(),
                );
            }
            (200, r#"[{"id":"w1"}]"#.into())
        })
        .await;
        (base, seen, configs)
    }

    #[tokio::test]
    async fn an_old_server_gets_no_activity_headers_on_lookup_or_add() {
        // No flag at all, and a /config that fails: both are "off".
        for config in [Some(r#"{"text":{"read":1,"paste":1}}"#), None] {
            let (base, seen, _) = gate_server(config).await;
            let api = Api::new(&base, signed_in());
            api.refresh_activity().await;
            api.lookup("n1", "hello", None, Some(Surface::Services))
                .await
                .unwrap();
            api.add_word("n1", &json!({ "headword": "hello" }), Some(Surface::Tray))
                .await
                .unwrap();
            assert_eq!(
                *seen.lock().unwrap(),
                ["POST /api/v1/ai/cards ", "POST /api/v1/notebooks/n1/words "],
                "{config:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_new_server_gets_the_surface_on_lookup_and_add() {
        let (base, seen, configs) = gate_server(Some(r#"{"activity":true}"#)).await;
        let api = Api::new(&base, signed_in());
        api.refresh_activity().await;
        api.lookup("n1", "hello", None, Some(Surface::Services))
            .await
            .unwrap();
        api.add_word("n1", &json!({ "headword": "hello" }), Some(Surface::Tray))
            .await
            .unwrap();
        assert_eq!(
            *seen.lock().unwrap(),
            [
                "POST /api/v1/ai/cards x-lexpad-surface: desktop.services",
                "POST /api/v1/notebooks/n1/words x-lexpad-surface: desktop.tray"
            ]
        );
        assert_eq!(configs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_gate_belongs_to_the_server_not_the_account() {
        // Per process, per API origin: signing out and in as someone else
        // keeps the answer, because it says what the server takes, nothing
        // about who is signed in. No account data is kept with it.
        let (base, _seen, configs) = gate_server(Some(r#"{"activity":true}"#)).await;
        let api = Api::new(&base, signed_in());
        assert!(api.refresh_activity().await);
        api.sign_out().await;
        assert!(api.user().await.is_none());
        assert!(api.activity_now());
        assert_eq!(configs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_windows_calls_are_forwarded_as_they_are_whatever_the_gate() {
        // The proxy never asks /config: an old server just gets headers it
        // ignores, and the window's call (here the events batch) arrives
        // whole, body and all.
        let configs = Arc::new(AtomicUsize::new(0));
        let c = configs.clone();
        let base = serve_head(move |line, head, body| {
            if line.starts_with("POST /api/v1/auth/refresh") {
                return (200, SESSION.into());
            }
            if line.starts_with("GET /api/v1/config") {
                c.fetch_add(1, Ordering::SeqCst);
            }
            assert!(line.starts_with("POST /api/v1/events "));
            assert!(head.contains("x-lexpad-surface: desktop.player"));
            assert!(head.contains("x-lexpad-context: mkv"));
            assert_eq!(body, r#"{"events":[{"name":"player.open"}]}"#);
            (202, r#"{"accepted":1,"dropped":0}"#.into())
        })
        .await;
        use base64::Engine as _;
        let checked = crate::proxy::check(
            &base,
            &crate::proxy::Request {
                method: "POST".into(),
                url: format!("{base}/api/v1/events"),
                headers: vec![
                    ("Content-Type".into(), "application/json".into()),
                    ("X-Lexpad-Surface".into(), "desktop.player".into()),
                    ("X-Lexpad-Context".into(), "mkv".into()),
                ],
                body: Some(
                    base64::engine::general_purpose::STANDARD
                        .encode(br#"{"events":[{"name":"player.open"}]}"#),
                ),
            },
        )
        .unwrap();
        let api = Api::new(&base, signed_in());
        let res = api.forward(&checked).await.unwrap();
        assert_eq!(res.status().as_u16(), 202);
        assert_eq!(configs.load(Ordering::SeqCst), 0);
    }

    /// A server whose /config answers `{"activity":true}` after `delay_ms`;
    /// records the other requests' surface headers and the /config count.
    async fn slow_config(
        delay_ms: u64,
    ) -> (
        String,
        Arc<std::sync::Mutex<Vec<Option<String>>>>,
        Arc<AtomicUsize>,
    ) {
        let seen: Arc<std::sync::Mutex<Vec<Option<String>>>> = Arc::default();
        let configs = Arc::new(AtomicUsize::new(0));
        let (s, c) = (seen.clone(), configs.clone());
        let base = serve_slow(move |line, head, _| {
            if line.starts_with("POST /api/v1/auth/refresh") {
                return (200, SESSION.into(), 0);
            }
            if line.starts_with("GET /api/v1/config") {
                c.fetch_add(1, Ordering::SeqCst);
                return (200, r#"{"activity":true}"#.into(), delay_ms);
            }
            let surface = head
                .lines()
                .find_map(|l| l.strip_prefix("x-lexpad-surface: "))
                .map(str::to_owned);
            s.lock().unwrap().push(surface);
            if line.starts_with("POST /api/v1/ai/cards") {
                return (
                    200,
                    r#"{"id":"j1","status":"done","cards":[{"headword":"hello"}]}"#.into(),
                    0,
                );
            }
            (200, r#"[{"id":"w1"}]"#.into(), 0)
        })
        .await;
        (base, seen, configs)
    }

    #[tokio::test]
    async fn a_hung_config_never_delays_a_lookup_or_an_add() {
        let (base, seen, configs) = slow_config(60_000).await;
        let api = Api::new(&base, signed_in());
        let started = Instant::now();
        api.lookup("n1", "hello", None, Some(Surface::Shortcut))
            .await
            .unwrap();
        api.add_word(
            "n1",
            &json!({ "headword": "hello" }),
            Some(Surface::Shortcut),
        )
        .await
        .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        // Unknown is "no": both went without the header.
        assert_eq!(*seen.lock().unwrap(), [None, None]);
        // The read was started in the background, once.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(configs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_slow_config_tags_the_calls_after_its_answer() {
        let (base, seen, configs) = slow_config(300).await;
        let api = Api::new(&base, signed_in());
        let word = json!({ "headword": "hello" });
        api.add_word("n1", &word, Some(Surface::Tray))
            .await
            .unwrap();
        // Joins the read the first call started rather than asking again.
        assert!(api.refresh_activity().await);
        api.add_word("n1", &word, Some(Surface::Tray))
            .await
            .unwrap();
        assert_eq!(
            *seen.lock().unwrap(),
            [None, Some("desktop.tray".to_string())]
        );
        assert_eq!(configs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn many_callers_share_one_config_read() {
        let (base, _seen, configs) = slow_config(100).await;
        let api = Arc::new(Api::new(&base, signed_in()));
        let calls: Vec<_> = (0..8)
            .map(|_| {
                let a = api.clone();
                tokio::spawn(async move {
                    let _ = a.activity_now();
                    a.refresh_activity().await
                })
            })
            .collect();
        for c in calls {
            assert!(c.await.unwrap());
        }
        assert_eq!(configs.load(Ordering::SeqCst), 1);
    }
}
