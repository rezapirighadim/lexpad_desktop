//! Signing in without leaving the app, in the Mac App Store copy only
//! (`--features mas`).
//!
//! App Review, 19 Nov 2026 (Guideline 4, on 0.3.33): a Mac App Store app may
//! not send the learner to the default browser to sign in or register; the
//! web may be used for that inside ASWebAuthenticationSession, and Sign in
//! with Apple must finish without leaving the app. So `commands::connect`
//! here:
//!
//! - with Apple, shows Apple's own sheet ([`apple`]) and sends its identity
//!   token to the API (`Api::sign_in_with_apple`); if Apple's sheet or the API
//!   refuses (no Apple ID on this Mac, an account in its deletion grace), it
//!   goes on to the connect page below with Apple asked for;
//! - otherwise shows the same RFC 8252 connect page as the dmg (PKCE, state,
//!   `lexpad://auth/callback`) in ASWebAuthenticationSession's sheet over
//!   Lexpad's window ([`web`]), which catches the callback itself.
//!
//! The sheets are `swift/LexpadStore/Sources/LexpadStore/SignIn.swift`; this
//! module is the bridge: each call waits for the sheet's one answer.

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use tokio::sync::oneshot;

use crate::auth::SheetEnd;

type Reply = extern "C" fn(u64, i32, *const c_char);

extern "C" {
    fn lexpad_web_auth_start(
        url: *const c_char,
        scheme: *const c_char,
        window: *mut c_void,
        ctx: u64,
        cb: Reply,
    );
    fn lexpad_web_auth_cancel();
    fn lexpad_apple_sign_in(nonce_hash: *const c_char, window: *mut c_void, ctx: u64, cb: Reply);
    fn lexpad_apple_cancel();
}

static NEXT: AtomicU64 = AtomicU64::new(1);
static PENDING: OnceLock<Mutex<HashMap<u64, oneshot::Sender<SheetEnd>>>> = OnceLock::new();

fn pending() -> &'static Mutex<HashMap<u64, oneshot::Sender<SheetEnd>>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

extern "C" fn on_reply(ctx: u64, status: i32, text: *const c_char) {
    // SAFETY: the bridge passes a NUL-terminated string valid for the call.
    let text = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    if let Some(tx) = pending().lock().ok().and_then(|mut p| p.remove(&ctx)) {
        let _ = tx.send(SheetEnd::from_status(status, text));
    }
}

/// The window a sheet is shown over, as an address that may cross an await
/// (`NSWindow*`; SignIn.swift uses it on the main thread only, and falls back
/// to Lexpad's window in front when it is gone from the screen).
pub fn anchor_of(window: &tauri::Window) -> usize {
    window.ns_window().map(|p| p as usize).unwrap_or(0)
}

fn begin() -> Option<(u64, oneshot::Receiver<SheetEnd>)> {
    let ctx = NEXT.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    pending().lock().ok()?.insert(ctx, tx);
    Some((ctx, rx))
}

async fn answer(rx: oneshot::Receiver<SheetEnd>) -> SheetEnd {
    rx.await
        .unwrap_or_else(|_| SheetEnd::Failed("the sheet gave no answer".into()))
}

/// The connect page in ASWebAuthenticationSession: ends with the callback
/// URL the sheet caught (for `auth::answer_from_callback`).
pub async fn web(url: &str, scheme: &str, anchor: usize) -> SheetEnd {
    let (Ok(url), Ok(scheme)) = (CString::new(url), CString::new(scheme)) else {
        return SheetEnd::Failed("not a sign-in address".into());
    };
    let Some((ctx, rx)) = begin() else {
        return SheetEnd::Failed("lock".into());
    };
    // SAFETY: both strings outlive the call (the bridge copies them first);
    // the window pointer is only read on the main thread, which owns it.
    unsafe {
        lexpad_web_auth_start(
            url.as_ptr(),
            scheme.as_ptr(),
            anchor as *mut c_void,
            ctx,
            on_reply,
        )
    };
    answer(rx).await
}

/// Ends the connect page's sheet (Cancel in the window, a timeout).
pub fn cancel_web() {
    // SAFETY: no arguments; the bridge hops to the main thread.
    unsafe { lexpad_web_auth_cancel() };
}

/// Apple's own Sign in with Apple sheet: ends with the credential as JSON
/// (`auth::AppleCredential::parse`).
pub async fn apple(nonce_hash: &str, anchor: usize) -> SheetEnd {
    let Ok(nonce) = CString::new(nonce_hash) else {
        return SheetEnd::Failed("nonce".into());
    };
    let Some((ctx, rx)) = begin() else {
        return SheetEnd::Failed("lock".into());
    };
    // SAFETY: as for `web`.
    unsafe { lexpad_apple_sign_in(nonce.as_ptr(), anchor as *mut c_void, ctx, on_reply) };
    answer(rx).await
}

/// Ends Apple's sheet.
pub fn cancel_apple() {
    // SAFETY: as for `cancel_web`.
    unsafe { lexpad_apple_cancel() };
}
