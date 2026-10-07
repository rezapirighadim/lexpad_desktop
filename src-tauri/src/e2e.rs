//! The end-to-end test against a local stack (never production). Ignored by
//! default; `scripts/e2e-local.sh` runs it with a browser driver beside it.
//!
//! What it exercises, for real: the RFC 8252 sign-in (PKCE, state, loopback
//! listener) with the web app's /connect-desktop in a real browser, the code
//! exchange at the API, the session in the macOS Keychain, the API client's
//! lookup and add, and sign-out revoking the session. The popup's own page
//! (the built `dist/popup.html`) runs in that browser too, its `invoke` calls
//! answered here by the same `Api` the app's commands use, so the word is
//! composed by the popup's real code.
//!
//! What it cannot exercise from an automated shell: reading TextEdit's
//! selection (that needs the Accessibility permission, which only the owner
//! can grant) and pressing keys in a native window. The capture the popup
//! receives is therefore the one the macOS reader produces for "candid"
//! selected in TextEdit, and the README's checklist covers the rest by hand.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::api::{Api, Device};
use crate::auth;
use crate::capture::{Capture, Permission, Via};
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

/// The bridge the popup page's `invoke` is pointed at.
async fn bridge(api: Arc<Api>, user: Value, capture: Capture, listener: TcpListener) -> Value {
    let mut notebook_id: Option<String> = None;
    loop {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        let (head, body) = loop {
            let n = s.read(&mut chunk).await.unwrap();
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
                    break (
                        text[..end].to_owned(),
                        text[end + 4..end + 4 + len].to_owned(),
                    );
                }
            }
            if n == 0 {
                break (text, String::new());
            }
        };
        let cors = "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: content-type\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\n";
        if head.starts_with("OPTIONS") {
            let _ = s.write_all(format!("HTTP/1.1 204 No Content\r\n{cors}Content-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await;
            continue;
        }
        let req: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        let cmd = req["cmd"].as_str().unwrap_or("").to_owned();
        let args = &req["args"];
        let result: Result<Value, String> = match cmd.as_str() {
            "state" => {
                let notebooks = api.notebooks().await.map_err(|f| f.as_str().to_owned());
                notebooks.map(|list| {
                    let default = list
                        .iter()
                        .find(|n| n["isDefault"] == true)
                        .or(list.first())
                        .and_then(|n| n["id"].as_str())
                        .map(str::to_owned);
                    notebook_id = notebook_id.clone().or(default);
                    json!({
                        "connected": true,
                        "user": user,
                        "notebooks": list,
                        "notebookId": notebook_id,
                        "capture": capture,
                        "shortcut": "CommandOrControl+Shift+L",
                        // The shell running this test, as the app would see it.
                        "permission": crate::capture::permission(),
                        "version": crate::config::VERSION,
                    })
                })
            }
            "lookup" => api
                .lookup(
                    args["notebookId"].as_str().unwrap_or(""),
                    args["headword"].as_str().unwrap_or(""),
                    args["hint"].as_str(),
                )
                .await
                .map_err(|f| f.as_str().to_owned()),
            "add_word" => api
                .add_word(args["notebookId"].as_str().unwrap_or(""), &args["word"])
                .await
                .map(Value::String)
                .map_err(|f| f.as_str().to_owned()),
            "set_notebook" => {
                notebook_id = args["notebookId"].as_str().map(str::to_owned);
                Ok(Value::Null)
            }
            "fit_popup" | "plugin:event|listen" | "plugin:event|unlisten" => Ok(Value::Null),
            "get_settings" => Ok(
                json!({ "shortcut": "CommandOrControl+Shift+L", "startOnLogin": true, "developmentBuild": true }),
            ),
            "app_info" => Ok(json!({
                "version": crate::config::VERSION,
                "apiOrigin": crate::config::API_ORIGIN,
                "appOrigin": crate::config::APP_ORIGIN,
                "autostartEnabled": false,
            })),
            "hide_popup" => Ok(Value::Null),
            "e2e_done" => {
                let _ = s.write_all(format!("HTTP/1.1 200 OK\r\n{cors}Content-Length: 2\r\nConnection: close\r\n\r\n{{}}").as_bytes()).await;
                return json!({ "notebookId": notebook_id });
            }
            other => Err(format!("unknown command {other}")),
        };
        let out = match result {
            Ok(v) => json!({ "ok": true, "value": v }),
            Err(e) => json!({ "ok": false, "error": e }),
        }
        .to_string();
        let _ = s
            .write_all(format!("HTTP/1.1 200 OK\r\n{cors}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len()).as_bytes())
            .await;
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the local stack; run scripts/e2e-local.sh"]
async fn local_stack_end_to_end() {
    let api_origin = local("LEXPAD_E2E_API");
    let app_origin = local("LEXPAD_E2E_APP");
    let dir = std::env::var("LEXPAD_E2E_DIR").expect("LEXPAD_E2E_DIR");

    // A Keychain entry of its own, so the test never touches an app's session.
    let store = Arc::new(Keyring::new(&format!("{api_origin} end-to-end test")));
    store.clear();
    let api = Arc::new(Api::new(&api_origin, store.clone()));

    // 1. Sign in through the browser.
    let pkce = auth::new_pkce();
    let state = auth::new_state();
    let (listener, port) = auth::listen().await.unwrap();
    let redirect = auth::redirect_uri(port);
    write(
        &format!("{dir}/connect-url.txt"),
        &auth::connect_url(&app_origin, &redirect, &state, &pkce.challenge),
    );
    let cancel = tokio::sync::Notify::new();
    let answer = tokio::time::timeout(
        Duration::from_secs(180),
        auth::wait_for_answer(listener, &state, &cancel),
    )
    .await
    .expect("the browser came back in time")
    .expect("an answer");
    let auth::Answer::Code(code) = answer else {
        panic!("the connect page was cancelled")
    };
    let device = Device {
        platform: "desktop",
        device_id: format!("e2e-{}", uuid::Uuid::new_v4()),
        device_name: "macOS".into(),
        app_version: crate::config::VERSION.into(),
    };
    let user = api
        .exchange_code(&code, &pkce.verifier, &redirect, &device)
        .await
        .expect("code exchanged");
    let saved = store.load().expect("the session is in the Keychain");
    assert_eq!(saved.user.email, user.email);
    write(&format!("{dir}/signed-in.txt"), &user.email);

    // 2. The popup page, answered by this Api.
    // A word the demo notebook does not have yet, met in a sentence.
    let word = std::env::var("LEXPAD_E2E_WORD").unwrap_or_else(|_| "tentative".into());
    let sentence = std::env::var("LEXPAD_E2E_SENTENCE")
        .unwrap_or_else(|_| "The committee reached a tentative agreement late last night.".into());
    let capture = Capture {
        text: Some(word.clone()),
        context: Some(format!(
            "Nobody expected it. {sentence} The talks resume on Monday."
        )),
        app: Some("TextEdit".into()),
        permission: Permission::Granted,
        via: Via::Accessibility,
        anchor: None,
    };
    let bridge_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    write(
        &format!("{dir}/bridge-port.txt"),
        &bridge_listener.local_addr().unwrap().port().to_string(),
    );
    let done = tokio::time::timeout(
        Duration::from_secs(240),
        bridge(
            api.clone(),
            serde_json::to_value(&user).unwrap(),
            capture,
            bridge_listener,
        ),
    )
    .await
    .expect("the popup finished in time");
    let notebook_id = done["notebookId"].as_str().expect("a notebook").to_owned();

    // 3. The word is in the notebook, with the sentence and where it was seen.
    let words = api
        .request(
            reqwest::Method::GET,
            &format!("/notebooks/{notebook_id}/words?q={word}"),
            None,
            false,
        )
        .await
        .expect("words listed");
    let items = words["items"].as_array().cloned().unwrap_or_default();
    let added = items
        .iter()
        .find(|w| w["headword"] == word.as_str())
        .expect("the word was added");
    write(
        &format!("{dir}/word.json"),
        &serde_json::to_string_pretty(added).unwrap(),
    );
    assert_eq!(
        added["progress"]["memo"], "Seen in TextEdit",
        "the private note names the app"
    );
    let first_example = &added["meanings"][0]["examples"][0]["sentence"];
    assert_eq!(
        first_example.as_str(),
        Some(sentence.as_str()),
        "the sentence it was met in comes first"
    );

    // 4. Listed under Signed-in devices as the desktop app.
    let sessions = api
        .request(reqwest::Method::GET, "/me/sessions", None, false)
        .await
        .expect("sessions");
    let mine = sessions["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["current"] == true)
        .cloned()
        .expect("this session is listed");
    assert_eq!(mine["platform"], "desktop");
    assert_eq!(mine["deviceName"], "macOS");
    write(
        &format!("{dir}/session.json"),
        &serde_json::to_string_pretty(&mine).unwrap(),
    );

    // 5. Sign-out revokes the session and empties the Keychain entry.
    api.sign_out().await;
    assert!(store.load().is_none(), "the Keychain entry is gone");
    let refresh = reqwest::Client::new()
        .post(format!("{api_origin}/api/v1/auth/refresh"))
        .json(&json!({ "refreshToken": saved.refresh_token }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        refresh.status().as_u16(),
        401,
        "the revoked session cannot be refreshed"
    );
    write(&format!("{dir}/signed-out.txt"), "ok");
}
