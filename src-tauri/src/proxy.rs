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

/// A request the core will send.
#[derive(Clone, Debug)]
pub struct Checked {
    pub method: reqwest::Method,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
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
            "desktop.app.notebook
X: y",
            &format!("desktop.{}", "a".repeat(41)),
        ] {
            assert!(!valid_surface(bad), "{bad}");
        }
        assert!(valid_surface("desktop.player.transcript"));
        assert!(valid_surface("desktop.app.lex_chat"));
        for bad in ["", "My Movie.mkv", "a/b", "MKV", &"a".repeat(65)] {
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
