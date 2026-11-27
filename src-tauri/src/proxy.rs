//! What Lexpad's window may send to the API through the core.
//!
//! The window runs the whole web app, and every call it makes comes here
//! (`api_fetch` in `commands.rs`): the core adds the session, which the
//! window never sees. So this is where the window is held to the API and to
//! what the session is for:
//!
//! - only the API's own origin, under `/api/v1/`, with no user name,
//!   password or fragment in the address;
//! - only the methods the client uses;
//! - only the headers the client sends: anything else, an `Authorization`
//!   or a `Cookie` above all, is dropped, so the window cannot speak for
//!   another session; the activity headers (`X-Lexpad-Surface`,
//!   `X-Lexpad-Context`) only when well formed, and a surface only when it
//!   is one of the desktop app's own (`desktop.`…), since this is that app;
//! - none of the calls that sign in, refresh, sign out, change a password,
//!   mint another session or approve an assistant: the core does its own
//!   signing in and out, and the rest belongs in the browser, where the
//!   learner is signed in in full (the server refuses them to this
//!   delegated session as well; this is the second lock);
//! - a body no bigger than `MAX_BODY`.
//!
//! One other shape crosses (owner, 27 Nov 2026: Podcasts in the desktop
//! app): a GET of the API's podcast audio proxy,
//! `/podcast-audio/<episodeId>/a.<ext>?t=<token>`, with at most a `Range`,
//! which the live transcript reads its windows from (op 88). The token in the
//! address is the whole grant, so it goes without the session, and the core
//! never follows its redirect to a publisher (`Kind::PodcastAudio`,
//! `Api::forward_audio`).

use serde::Deserialize;
use url::Url;

/// The biggest body the window may send: an import file is the largest
/// thing it sends, and the API refuses bigger ones itself.
pub const MAX_BODY: usize = 25 * 1024 * 1024;

/// Headers the web app's client sets; nothing else crosses.
const HEADERS: &[&str] = &[
    "accept",
    "content-type",
    "idempotency-key",
    "last-event-id",
    "if-none-match",
];

/// Where in the app a call comes from (reports/specs/2026-10-26-activity-tracking.md §2).
const SURFACE: &str = "x-lexpad-surface";
/// The short slug that goes with it (a file kind; never a name or an address).
const CONTEXT: &str = "x-lexpad-context";
/// The app session the call belongs to (contract §9), so what the server
/// records for it joins the visit: 8–64 of `[A-Za-z0-9_-]`, a random id.
const APP_SESSION: &str = "x-lexpad-app-session";

/// An app session as the API takes it.
pub fn valid_app_session(value: &str) -> bool {
    (8..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A surface as the API takes it: `^[a-z0-9]+(\.[a-z0-9_]+){0,3}$`, at most
/// 48 characters, and, from this window, always one of the desktop app's own.
pub fn valid_surface(value: &str) -> bool {
    if value.len() > 48 || !value.starts_with("desktop.") {
        return false;
    }
    let mut parts = value.split('.');
    let first = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    !first.is_empty()
        && first
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && rest.len() <= 3
        && rest.iter().all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

/// A context as the API takes it: `^[a-z0-9.-]+$`, at most 64 characters.
pub fn valid_context(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

/// Whether a header from the window may cross, by name and value.
fn header_allowed(name: &str, value: &str) -> bool {
    if value.chars().any(|c| c.is_control()) {
        return false;
    }
    match name {
        SURFACE => valid_surface(value),
        CONTEXT => valid_context(value),
        APP_SESSION => valid_app_session(value),
        _ => HEADERS.contains(&name),
    }
}

/// Calls under `/api/v1/auth/` the window may make: confirming an e-mail
/// address and asking for the link again. Everything else there is signing
/// in or out, refreshing, or a password.
const AUTH_ALLOWED: &[&str] = &["auth/resend-verification", "auth/verify-email"];

/// A request as the window describes it.
#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// The body, base64, if there is one.
    pub body: Option<String>,
}

/// Which way a checked request goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A call under `/api/v1/`, sent with the desktop app's session.
    Api,
    /// A read of the podcast audio proxy: no session, no redirect followed.
    PodcastAudio,
}

/// A request the core will send.
#[derive(Clone, Debug)]
pub struct Checked {
    pub method: reqwest::Method,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub kind: Kind,
}

/// The audio proxy's path on the API (lexpad_back `audioproxy.PathPrefix`).
const AUDIO_PREFIX: &str = "/podcast-audio/";

/// Whether `path` is `/podcast-audio/<episodeId>/a.<ext>`: an id of the
/// API's plain characters, a short lower-case extension, nothing else.
fn audio_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix(AUDIO_PREFIX) else {
        return false;
    };
    let Some((id, file)) = rest.split_once('/') else {
        return false;
    };
    let Some(ext) = file.strip_prefix("a.") else {
        return false;
    };
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && (1..=5).contains(&ext.len())
        && ext
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Whether the query is exactly one `t` of the token's characters.
fn audio_query(url: &Url) -> bool {
    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    matches!(pairs.as_slice(), [(name, token)]
        if name == "t"
            && (1..=512).contains(&token.len())
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.~".contains(&b)))
}

/// A byte range as the live transcript asks for it: `bytes=<from>-[<to>]`.
fn valid_range(value: &str) -> bool {
    let Some(spec) = value.strip_prefix("bytes=") else {
        return false;
    };
    let Some((from, to)) = spec.split_once('-') else {
        return false;
    };
    let digits = |s: &str| s.len() <= 15 && s.bytes().all(|b| b.is_ascii_digit());
    !from.is_empty() && digits(from) && digits(to)
}

/// Why a request was refused. Never shown to anyone; the window gets a
/// failed fetch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    Method,
    Address,
    Call,
    Body,
}

/// Checks a request from the window against the API at `api_origin`.
pub fn check(api_origin: &str, req: &Request) -> Result<Checked, Refused> {
    let method = match req.method.to_ascii_uppercase().as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "PATCH" => reqwest::Method::PATCH,
        "DELETE" => reqwest::Method::DELETE,
        _ => return Err(Refused::Method),
    };

    let api = Url::parse(api_origin).map_err(|_| Refused::Address)?;
    let url = Url::parse(&req.url).map_err(|_| Refused::Address)?;
    if url.origin() != api.origin()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Refused::Address);
    }
    // The parser has already resolved any dot segments; a raw one left in
    // the original text is refused anyway, as is an encoded slash.
    let raw = req.url.to_ascii_lowercase();
    if raw.contains("/../") || raw.contains("/./") || raw.contains("%2f") || raw.contains("%5c") {
        return Err(Refused::Address);
    }
    if url.path().starts_with(AUDIO_PREFIX) {
        return check_audio(method, url, req);
    }
    let Some(call) = url.path().strip_prefix("/api/v1/") else {
        return Err(Refused::Address);
    };
    if !allowed_call(&method, call) {
        return Err(Refused::Call);
    }

    let headers = req
        .headers
        .iter()
        .filter(|(name, value)| header_allowed(&name.to_ascii_lowercase(), value))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
        .collect();

    let body = match &req.body {
        None => None,
        Some(encoded) => {
            use base64::Engine as _;
            // Base64 is four characters for every three bytes.
            if encoded.len() > MAX_BODY / 3 * 4 + 4 {
                return Err(Refused::Body);
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| Refused::Body)?;
            if bytes.len() > MAX_BODY {
                return Err(Refused::Body);
            }
            Some(bytes)
        }
    };
    Ok(Checked {
        method,
        url,
        headers,
        body,
        kind: Kind::Api,
    })
}

/// A read of the podcast audio proxy: GET only, its exact shape, no body, and
/// of the headers only `Range` (well formed) and `Accept`.
fn check_audio(method: reqwest::Method, url: Url, req: &Request) -> Result<Checked, Refused> {
    if method != reqwest::Method::GET {
        return Err(Refused::Method);
    }
    if !audio_path(url.path()) || !audio_query(&url) {
        return Err(Refused::Address);
    }
    if req.body.is_some() {
        return Err(Refused::Body);
    }
    let headers = req
        .headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.clone()))
        .filter(|(name, value)| match name.as_str() {
            "range" => valid_range(value),
            "accept" => !value.chars().any(|c| c.is_control()),
            _ => false,
        })
        .collect();
    Ok(Checked {
        method,
        url,
        headers,
        body: None,
        kind: Kind::PodcastAudio,
    })
}

/// Whether the window may make this call (the path after `/api/v1/`).
fn allowed_call(method: &reqwest::Method, call: &str) -> bool {
    // Compared without case, so a spelling the server might route the same
    // way cannot slip past.
    let lower = call.trim_end_matches('/').to_ascii_lowercase();
    let call = lower.as_str();
    if call.starts_with("auth/") || call == "auth" {
        return AUTH_ALLOWED.contains(&call);
    }
    // Another session for another program, or a code for another desktop.
    if call == "me/desktop-codes" || (call == "me/sessions" && method == reqwest::Method::POST) {
        return false;
    }
    // An assistant's approval belongs to the browser that asked for it.
    if call.starts_with("oauth/") {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const API: &str = "https://api.lexpad.app";

    fn req(method: &str, url: &str) -> Request {
        Request {
            method: method.into(),
            url: url.into(),
            headers: vec![],
            body: None,
        }
    }

    #[test]
    fn the_app_calls_go_through() {
        for (m, u) in [
            ("GET", "https://api.lexpad.app/api/v1/me"),
            ("GET", "https://api.lexpad.app/api/v1/sync/snapshot"),
            ("POST", "https://api.lexpad.app/api/v1/sync/push"),
            (
                "GET",
                "https://api.lexpad.app/api/v1/sync/changes?cursor=12",
            ),
            ("PATCH", "https://api.lexpad.app/api/v1/me"),
            ("DELETE", "https://api.lexpad.app/api/v1/me"),
            ("GET", "https://api.lexpad.app/api/v1/me/sessions"),
            (
                "DELETE",
                "https://api.lexpad.app/api/v1/me/sessions/01J0000000000000000000000A",
            ),
            (
                "POST",
                "https://api.lexpad.app/api/v1/auth/resend-verification",
            ),
            ("POST", "https://api.lexpad.app/api/v1/devices"),
            ("PUT", "https://api.lexpad.app/api/v1/me/nudges/n1"),
        ] {
            let checked = check(API, &req(m, u)).unwrap_or_else(|e| panic!("{m} {u}: {e:?}"));
            assert_eq!(checked.url.as_str(), u);
        }
    }

    #[test]
    fn signing_in_out_and_other_sessions_stay_with_the_core_or_the_browser() {
        for (m, u) in [
            ("POST", "https://api.lexpad.app/api/v1/auth/refresh"),
            ("POST", "https://api.lexpad.app/api/v1/auth/login"),
            ("POST", "https://api.lexpad.app/api/v1/auth/logout"),
            ("POST", "https://api.lexpad.app/api/v1/auth/google"),
            ("POST", "https://api.lexpad.app/api/v1/auth/desktop/token"),
            ("POST", "https://api.lexpad.app/api/v1/auth/change-password"),
            ("POST", "https://api.lexpad.app/api/v1/auth/set-password"),
            ("POST", "https://api.lexpad.app/api/v1/auth/reauth"),
            ("POST", "https://api.lexpad.app/api/v1/AUTH/refresh"),
            ("POST", "https://api.lexpad.app/api/v1/me/desktop-codes"),
            ("POST", "https://api.lexpad.app/api/v1/me/sessions"),
            (
                "POST",
                "https://api.lexpad.app/api/v1/oauth/requests/r1/decision",
            ),
        ] {
            assert_eq!(
                check(API, &req(m, u)).map(|_| ()),
                Err(Refused::Call),
                "{m} {u}"
            );
        }
    }

    const AUDIO: &str =
        "https://api.lexpad.app/podcast-audio/01JBQ0000000000000PLAYER01/a.mp3?t=AbC-12_x.y~z";

    #[test]
    fn the_podcast_audio_proxy_crosses_with_a_range_only_and_no_session() {
        let mut r = req("GET", AUDIO);
        r.headers = vec![
            ("Range".into(), "bytes=0-65535".into()),
            ("Authorization".into(), "Bearer forged".into()),
            ("Cookie".into(), "a=b".into()),
            ("X-Lexpad-Surface".into(), "desktop.app".into()),
            ("Accept".into(), "*/*".into()),
        ];
        let checked = check(API, &r).unwrap();
        assert_eq!(checked.kind, Kind::PodcastAudio);
        assert_eq!(checked.url.as_str(), AUDIO);
        assert_eq!(
            checked.headers,
            vec![
                ("range".to_string(), "bytes=0-65535".to_string()),
                ("accept".to_string(), "*/*".to_string()),
            ]
        );
        let mut open_ended = req("GET", AUDIO);
        open_ended.headers = vec![("range".into(), "bytes=1024-".into())];
        assert_eq!(check(API, &open_ended).unwrap().headers.len(), 1);
        for bad in [
            "bytes=-500",
            "bytes=1-2,4-5",
            "items=0-1",
            "bytes=a-b",
            "bytes=0-1\n",
        ] {
            let mut r = req("GET", AUDIO);
            r.headers = vec![("range".into(), bad.into())];
            assert!(check(API, &r).unwrap().headers.is_empty(), "{bad}");
        }
        assert_eq!(
            check(API, &req("GET", "https://api.lexpad.app/api/v1/me"))
                .unwrap()
                .kind,
            Kind::Api
        );
    }

    #[test]
    fn only_the_audio_proxys_own_shape_crosses() {
        for m in ["POST", "PUT", "DELETE"] {
            assert_eq!(
                check(API, &req(m, AUDIO)).map(|_| ()),
                Err(Refused::Method),
                "{m}"
            );
        }
        for u in [
            "https://api.lexpad.app/podcast-audio/ep1/a.mp3",
            "https://api.lexpad.app/podcast-audio/ep1/a.mp3?t=x&u=y",
            "https://api.lexpad.app/podcast-audio/ep1/a.mp3?t=x&t=y",
            "https://api.lexpad.app/podcast-audio/ep1/a.mp3?t=",
            "https://api.lexpad.app/podcast-audio/ep1/a.mp3?t=a%2Fb",
            "https://api.lexpad.app/podcast-audio/ep1/b.mp3?t=x",
            "https://api.lexpad.app/podcast-audio/ep1/a.MP3?t=x",
            "https://api.lexpad.app/podcast-audio/ep1/a.toolong?t=x",
            "https://api.lexpad.app/podcast-audio/ep1/sub/a.mp3?t=x",
            "https://api.lexpad.app/podcast-audio//a.mp3?t=x",
            "https://api.lexpad.app/podcast-audio/e.p/a.mp3?t=x",
            "https://api.lexpad.app/podcast-audio/ep1/a.mp3/x?t=x",
            "https://api.lexpad.app/podcast-audio/ep1/a.mp3?t=x#f",
            "https://evil.example/podcast-audio/ep1/a.mp3?t=x",
            "http://api.lexpad.app/podcast-audio/ep1/a.mp3?t=x",
            "https://api.lexpad.app/podcast-audio/../api/v1/oauth/token?t=x",
            "https://api.lexpad.app/podcast-audio/ep1%2f..%2f/a.mp3?t=x",
        ] {
            assert!(check(API, &req("GET", u)).is_err(), "{u}");
        }
        let mut with_body = req("GET", AUDIO);
        with_body.body = Some("AAAA".into());
        assert_eq!(check(API, &with_body).map(|_| ()), Err(Refused::Body));
    }

    #[test]
    fn only_the_api_and_only_its_v1() {
        for u in [
            "https://evil.example/api/v1/me",
            "http://api.lexpad.app/api/v1/me",
            "https://api.lexpad.app:8443/api/v1/me",
            "https://user:pw@api.lexpad.app/api/v1/me",
            "https://api.lexpad.app/mcp",
            "https://api.lexpad.app/api/v2/me",
            "https://api.lexpad.app/api/v1/../oauth/token",
            "https://api.lexpad.app/api/v1/words/..%2f..%2foauth",
            "https://api.lexpad.app/api/v1/me#x",
            "https://api.lexpad.app.evil.example/api/v1/me",
            "not a url",
        ] {
            assert_eq!(
                check(API, &req("GET", u)).map(|_| ()),
                Err(Refused::Address),
                "{u}"
            );
        }
        assert_eq!(
            check(API, &req("TRACE", "https://api.lexpad.app/api/v1/me")).map(|_| ()),
            Err(Refused::Method)
        );
    }

    #[test]
    fn only_the_clients_own_headers_cross() {
        let mut r = req("POST", "https://api.lexpad.app/api/v1/sync/push");
        r.headers = vec![
            ("Authorization".into(), "Bearer someone-else".into()),
            ("Cookie".into(), "lexpad_refresh=x".into()),
            ("Content-Type".into(), "application/json".into()),
            ("Idempotency-Key".into(), "k1".into()),
            ("Accept".into(), "text/event-stream".into()),
            ("Last-Event-ID".into(), "7".into()),
            ("X-Forwarded-For".into(), "1.2.3.4".into()),
            ("Accept".into(), "bad\r\nInjected: yes".into()),
        ];
        let checked = check(API, &r).unwrap();
        let names: Vec<_> = checked.headers.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["content-type", "idempotency-key", "accept", "last-event-id"]
        );
    }

    #[test]
    fn the_activity_headers_cross_only_when_well_formed_and_the_desktops_own() {
        let mut r = req("POST", "https://api.lexpad.app/api/v1/events");
        r.headers = vec![
            ("X-Lexpad-Surface".into(), "desktop.app.notebook".into()),
            ("X-Lexpad-Context".into(), "mkv".into()),
        ];
        let checked = check(API, &r).unwrap();
        assert_eq!(
            checked.headers,
            [
                (
                    "x-lexpad-surface".to_string(),
                    "desktop.app.notebook".to_string()
                ),
                ("x-lexpad-context".to_string(), "mkv".to_string()),
            ]
        );
        for bad in [
            "app.notebook",
            "ext.selection",
            "desktop",
            "desktop.",
            "desktop.App",
            "desktop.a.b.c.d",
            "desktop.player-transcript",
            "desktop.app.notebook\r\nX: y",
            "desktop.app\tnotebook",
            "DESKTOP.APP.NOTEBOOK",
            &format!("desktop.{}", "a".repeat(41)),
        ] {
            assert!(!valid_surface(bad), "{bad}");
        }
        assert!(valid_surface("desktop.player.transcript"));
        assert!(valid_surface("desktop.app.lex_chat"));
        for bad in [
            "",
            "My Movie.mkv",
            "a/b",
            "/users/me/movies/film.mkv",
            "c:\\films\\a.mkv",
            "mkv\n",
            "MKV",
            &"a".repeat(65),
        ] {
            assert!(!valid_context(bad), "{bad}");
        }
        r.headers = vec![
            ("X-Lexpad-Surface".into(), "app.notebook".into()),
            (
                "X-Lexpad-Context".into(),
                "/Users/me/Movies/film.mkv".into(),
            ),
        ];
        assert!(check(API, &r).unwrap().headers.is_empty());
    }

    #[test]
    fn the_app_session_crosses_only_as_a_short_random_id() {
        let mut r = req("POST", "https://api.lexpad.app/api/v1/notebooks/x/words");
        r.headers = vec![("X-Lexpad-App-Session".into(), "visit-0001_AbC".into())];
        assert_eq!(
            check(API, &r).unwrap().headers,
            [(
                "x-lexpad-app-session".to_string(),
                "visit-0001_AbC".to_string()
            )]
        );
        for bad in [
            "short",
            "has space here",
            "a/b/c/d/e/f",
            "x\r\nX: y",
            &"a".repeat(65),
        ] {
            assert!(!valid_app_session(bad), "{bad}");
        }
        r.headers = vec![("X-Lexpad-App-Session".into(), "has space here".into())];
        assert!(check(API, &r).unwrap().headers.is_empty());
    }

    #[test]
    fn the_events_call_passes_with_its_body() {
        use base64::Engine as _;
        let body = br#"{"events":[{"name":"app.open"},{"name":"player.watch","n":3}]}"#;
        let mut r = req("POST", "https://api.lexpad.app/api/v1/events");
        r.body = Some(base64::engine::general_purpose::STANDARD.encode(body));
        let checked = check(API, &r).unwrap();
        assert_eq!(checked.method, reqwest::Method::POST);
        assert_eq!(checked.body.unwrap(), body);
    }

    #[test]
    fn a_body_is_decoded_and_kept_within_bounds() {
        use base64::Engine as _;
        let mut r = req("POST", "https://api.lexpad.app/api/v1/sync/push");
        r.body = Some(base64::engine::general_purpose::STANDARD.encode(b"{\"a\":1}"));
        assert_eq!(check(API, &r).unwrap().body.unwrap(), b"{\"a\":1}");
        r.body = Some("not base64!".into());
        assert_eq!(check(API, &r).map(|_| ()), Err(Refused::Body));
        r.body = Some("A".repeat(MAX_BODY / 3 * 4 + 8));
        assert_eq!(check(API, &r).map(|_| ()), Err(Refused::Body));
    }

    #[test]
    fn a_local_api_is_held_to_its_own_origin_too() {
        let local = "http://localhost:8091";
        assert!(check(local, &req("GET", "http://localhost:8091/api/v1/me")).is_ok());
        assert_eq!(
            check(local, &req("GET", "http://localhost:8092/api/v1/me")).map(|_| ()),
            Err(Refused::Address)
        );
    }
}
