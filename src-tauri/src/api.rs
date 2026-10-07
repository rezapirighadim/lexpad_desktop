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

pub struct Api {
    http: reqwest::Client,
    /// For the main window's calls (`forward`): no limit on the whole
    /// exchange, because Lex's answers stream for as long as they take, but
    /// a limit on connecting and on each silence.
    stream_http: reqwest::Client,
    base: String,
    store: Arc<dyn SessionStore>,
    inner: Mutex<Inner>,
}

impl Api {
    pub fn new(api_origin: &str, store: Arc<dyn SessionStore>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(format!("Lexpad-Desktop/{}", crate::config::VERSION))
            .build()
            .expect("HTTP client");
        let stream_http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(FORWARD_SILENCE)
            .user_agent(format!("Lexpad-Desktop/{}", crate::config::VERSION))
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

    /// One request with the bearer token, refreshed once on a 401.
    pub(crate) async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
        idempotent: bool,
    ) -> Result<Value, Failure> {
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
    ) -> Result<Value, Failure> {
        let mut body = json!({ "notebookId": notebook_id, "words": [headword] });
        if let Some(h) = hint {
            body["hint"] = json!(h);
        }
        let mut job = self
            .request(reqwest::Method::POST, "/ai/cards", Some(&body), true)
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
    pub async fn add_word(&self, notebook_id: &str, word: &Value) -> Result<String, Failure> {
        let body = json!({ "items": [word] });
        let created = self
            .request(
                reqwest::Method::POST,
                &format!("/notebooks/{notebook_id}/words"),
                Some(&body),
                true,
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
                                let auth = head
                                    .lines()
                                    .find_map(|l| l.strip_prefix("authorization: Bearer "))
                                    .unwrap_or("");
                                let body = &text[end + 4..end + 4 + len];
                                let (status, out) = handle(line, auth, body);
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
}
