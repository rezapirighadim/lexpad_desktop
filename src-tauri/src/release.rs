//! Which desktop version is out: the desktop section of the site's version
//! file (`lexpad.app/version.json`, written by the landing's
//! `scripts/features.py` from the release facts every release updates).
//! Nobody updates this app for the learner: the updater is off and a copy
//! from lexpad.app has no store behind it. So Lexpad's window asks the core
//! about once a day, and its Today page says when a newer version is ready
//! (front `lib/updates.ts`, which decides what to offer and checks every
//! address before it draws a button).
//!
//! The window cannot read the site itself: its CSP allows IPC only. The core
//! reads one fixed address, sends nothing but the request (no session, no
//! token, no account: the API's client is not used), and hands back only
//! version numbers and download pages, with which system this is and, on
//! Windows, whether this copy came from the Microsoft Store, since the Store
//! updates its copies itself and may serve a version later than the site.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The public site, which serves the version file.
pub const SITE_ORIGIN: &str = match option_env!("LEXPAD_SITE_ORIGIN") {
    Some(v) => v,
    None => "https://lexpad.app",
};

/// The version file on the site; Android's sideloaded builds read it too.
pub const VERSION_PATH: &str = "/version.json";

/// The longest the file may take; it is a few hundred bytes.
const TIMEOUT: Duration = Duration::from_secs(15);

/// The largest version file accepted, so a broken deploy cannot hand the
/// window something large.
const MAX_BYTES: usize = 64 * 1024;

/// One way to get the app, as the site names it.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Channel {
    pub version: String,
    pub url: String,
}

/// What the Microsoft Store serves. Its address is written in the web app,
/// never taken from the file.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct StoreChannel {
    pub version: String,
}

/// The desktop section of the version file.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Releases {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<Channel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<Channel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms_store: Option<StoreChannel>,
}

#[derive(Deserialize)]
struct VersionFile {
    #[serde(default)]
    desktop: Releases,
}

/// What the window is told.
#[derive(Debug, Serialize)]
pub struct Release {
    pub os: &'static str,
    pub install: Option<&'static str>,
    pub releases: Releases,
}

/// The desktop section of a version file. A file without one is no news,
/// not an error; a file that is not the version file is an error.
pub fn parse(body: &[u8]) -> Result<Releases, String> {
    if body.len() > MAX_BYTES {
        return Err("too large".into());
    }
    serde_json::from_slice::<VersionFile>(body)
        .map(|f| f.desktop)
        .map_err(|_| "not a version file".to_string())
}

/// Reads the version file at `origin`. Any failure (offline, a timeout, a
/// status other than 200, a body that is not the file) is an error the
/// window ignores: it keeps its last answer and asks again later.
pub async fn fetch(origin: &str) -> Result<Releases, String> {
    let http = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(crate::config::user_agent())
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}{VERSION_PATH}", origin.trim_end_matches('/'));
    let response = http
        .get(url)
        .header("Cache-Control", "no-cache")
        .send()
        .await
        .map_err(|_| "offline".to_string())?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(format!("status {}", response.status().as_u16()));
    }
    let body = response.bytes().await.map_err(|_| "offline".to_string())?;
    parse(&body)
}

/// This computer's system, as the window names it.
pub fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

/// The newest versions on the site, for this computer and this copy.
pub async fn latest() -> Result<Release, String> {
    let releases = fetch(SITE_ORIGIN).await?;
    Ok(Release {
        os: os(),
        install: crate::config::install_source(),
        releases,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const FILE: &str = r#"{
      "android": {"versionName": "1.0.74", "versionCode": 75, "url": "https://lexpad.app/android/"},
      "desktop": {
        "mac": {"version": "0.3.16", "url": "https://lexpad.app/download/mac"},
        "windows": {"version": "0.3.16", "url": "https://lexpad.app/download/windows"},
        "msStore": {"version": "0.3.15"}
      }
    }"#;

    #[test]
    fn reads_the_desktop_section_of_the_version_file() {
        let r = parse(FILE.as_bytes()).unwrap();
        assert_eq!(r.mac.unwrap().version, "0.3.16");
        assert_eq!(
            r.windows.unwrap().url,
            "https://lexpad.app/download/windows"
        );
        assert_eq!(r.ms_store.unwrap().version, "0.3.15");
    }

    #[test]
    fn a_file_without_a_desktop_section_is_no_news() {
        let r = parse(br#"{"android": {"versionName": "1.0.74"}}"#).unwrap();
        assert_eq!(r, Releases::default());
    }

    #[test]
    fn anything_but_the_version_file_is_an_error() {
        assert!(parse(b"<html>not found</html>").is_err());
        assert!(parse(br#"{"desktop": {"mac": {"version": 3}}}"#).is_err());
        assert!(parse(&vec![b' '; MAX_BYTES + 1]).is_err());
    }

    #[test]
    fn the_window_is_told_camel_case_and_nothing_empty() {
        let release = Release {
            os: "windows",
            install: Some("store"),
            releases: parse(FILE.as_bytes()).unwrap(),
        };
        let json = serde_json::to_value(&release).unwrap();
        assert_eq!(json["install"], "store");
        assert_eq!(json["releases"]["msStore"]["version"], "0.3.15");
        let empty = serde_json::to_value(Releases::default()).unwrap();
        assert_eq!(empty, serde_json::json!({}));
    }

    /// Answers every request with `status` and `body`, once per connection.
    async fn serve(status: u16, body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let mut seen = Vec::new();
                    while !String::from_utf8_lossy(&seen).contains("\r\n\r\n") {
                        let n = s.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        seen.extend_from_slice(&buf[..n]);
                    }
                    let head = String::from_utf8_lossy(&seen).to_string();
                    // Only the file is asked for, with no session of any kind.
                    assert!(head.starts_with("GET /version.json HTTP/1.1"));
                    assert!(!head.to_ascii_lowercase().contains("authorization"));
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = s.write_all(resp.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn fetches_the_file_from_the_site() {
        let origin = serve(200, FILE).await;
        let r = fetch(&origin).await.unwrap();
        assert_eq!(r.mac.unwrap().version, "0.3.16");
    }

    #[tokio::test]
    async fn a_failing_site_is_an_error_the_window_ignores() {
        let origin = serve(503, "{}").await;
        assert_eq!(fetch(&origin).await, Err("status 503".to_string()));
        // Nothing listening: offline.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert_eq!(fetch(&closed).await, Err("offline".to_string()));
    }
}
