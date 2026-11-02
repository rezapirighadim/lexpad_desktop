//! Lexpad Pro through Apple's in-app purchase, in the Mac App Store build only
//! (`--features mas`; docs/mac-app-store-plan.md).
//!
//! App Store Review Guideline 3.1.1: inside a Mac App Store app, digital goods
//! are sold only through Apple's in-app purchase, and 3.1.3(f) asks a service
//! sold on other platforms to offer it there too. The purchase is StoreKit 2,
//! which is Swift only, so the work is `src-tauri/swift/LexpadStore` (compiled
//! by build.rs for this feature), the iPhone shell's `StoreKitPlugin.swift`
//! for the Mac: the same calls, answers and error codes, so the web app's
//! `features/billing/appstore.ts` drives both. This module is the bridge: one
//! command per plugin method, each taking the plugin's `options` object and
//! answering its JSON, or rejecting with `{code, message, data: {detail}}`.
//!
//! Nothing here decides that anybody has paid: the window sends a purchase to
//! the API (`POST /billing/apple/verify`, through `api_fetch`), and asks for
//! `store_finish` only after the server said 200. Transactions that arrive by
//! themselves (renewals, a purchase approved later, the unfinished ones of the
//! last run) come from `Transaction.updates`: each is kept here
//! (`MAX_WAITING`) until the window takes them (`store_take_transactions`) and
//! is also sent to Lexpad's window as `store:transaction`.
//!
//! In the site's dmg and on Windows every command rejects `unavailable`: those
//! builds sell nothing (the web app never calls them there).

// The bridge's helpers are used only by the Mac App Store build; the rest of
// the module (the commands answering "unavailable") is in every build.
#![cfg_attr(not(all(feature = "mas", target_os = "macos")), allow(dead_code))]

use serde_json::{json, Value};

/// The event the window hears for a transaction that arrived by itself.
pub const EVENT: &str = "store:transaction";

/// How many arrived transactions are kept for the window. StoreKit offers an
/// unfinished one again at every launch, so dropping the oldest loses nothing.
pub const MAX_WAITING: usize = 50;

/// The plugin's error shape, as the web app's `fromPlugin` reads it.
pub fn failure(code: &str, message: &str) -> Value {
    json!({ "code": code, "message": message, "data": {} })
}

/// Reads the bridge's reply: `{"ok": value}` or `{"error": {...}}`.
pub fn parse_reply(text: &str) -> Result<Value, Value> {
    let reply: Value = serde_json::from_str(text)
        .map_err(|_| failure("failed", "The App Store's answer could not be read."))?;
    if let Some(error) = reply.get("error") {
        return Err(error.clone());
    }
    match reply.get("ok") {
        Some(v) => Ok(v.clone()),
        None => Err(failure(
            "failed",
            "The App Store's answer could not be read.",
        )),
    }
}

/// The plugin methods the window may call, by command.
pub const METHODS: &[(&str, &str)] = &[
    ("store_can_make_payments", "canMakePayments"),
    ("store_products", "products"),
    ("store_purchase", "purchase"),
    ("store_finish", "finish"),
    ("store_unfinished", "unfinished"),
    ("store_current_entitlements", "currentEntitlements"),
    ("store_sync", "sync"),
    ("store_manage_subscriptions", "manageSubscriptions"),
    ("store_app_transaction_id", "appTransactionId"),
];

/// What the window passes: an object, or nothing (`{}`).
fn options_of(options: Option<Value>) -> Value {
    match options {
        Some(v @ Value::Object(_)) => v,
        _ => json!({}),
    }
}

/// Keeps one arrived transaction for the window, newest last, at most
/// `MAX_WAITING`, and never the same transaction twice.
pub fn keep(waiting: &mut Vec<Value>, transaction: Value) {
    let id = transaction.get("transactionId").cloned();
    if id.is_some()
        && waiting
            .iter()
            .any(|t| t.get("transactionId") == id.as_ref())
    {
        return;
    }
    waiting.push(transaction);
    if waiting.len() > MAX_WAITING {
        let extra = waiting.len() - MAX_WAITING;
        waiting.drain(..extra);
    }
}

#[cfg(all(feature = "mas", target_os = "macos"))]
mod bridge {
    use std::collections::HashMap;
    use std::ffi::{c_char, CStr, CString};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};

    use serde_json::Value;
    use tauri::{AppHandle, Emitter, Manager};
    use tokio::sync::oneshot;

    type Reply = extern "C" fn(u64, *const c_char);
    type Event = extern "C" fn(*const c_char);

    extern "C" {
        fn lexpad_store_call(method: *const c_char, json: *const c_char, ctx: u64, cb: Reply);
        fn lexpad_store_listen(cb: Event);
    }

    static NEXT: AtomicU64 = AtomicU64::new(1);
    static PENDING: OnceLock<Mutex<HashMap<u64, oneshot::Sender<String>>>> = OnceLock::new();
    static WAITING: Mutex<Vec<Value>> = Mutex::new(Vec::new());
    static APP: OnceLock<AppHandle> = OnceLock::new();

    fn pending() -> &'static Mutex<HashMap<u64, oneshot::Sender<String>>> {
        PENDING.get_or_init(|| Mutex::new(HashMap::new()))
    }

    extern "C" fn on_reply(ctx: u64, text: *const c_char) {
        // SAFETY: the bridge passes a NUL-terminated string valid for the call.
        let text = unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned();
        if let Some(tx) = pending().lock().ok().and_then(|mut p| p.remove(&ctx)) {
            let _ = tx.send(text);
        }
    }

    extern "C" fn on_event(text: *const c_char) {
        // SAFETY: as above.
        let text = unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned();
        let Ok(transaction) = serde_json::from_str::<Value>(&text) else {
            return;
        };
        if let Ok(mut w) = WAITING.lock() {
            super::keep(&mut w, transaction.clone());
        }
        if let Some(app) = APP.get() {
            if app.get_webview_window(crate::main_window::LABEL).is_some() {
                let _ = app.emit_to(crate::main_window::LABEL, super::EVENT, transaction);
            }
        }
    }

    /// Starts `Transaction.updates` for the life of the app, at start-up.
    pub fn start(app: &AppHandle) {
        let _ = APP.set(app.clone());
        // SAFETY: the callback is a plain function that lives for ever.
        unsafe { lexpad_store_listen(on_event) };
    }

    pub fn take() -> Vec<Value> {
        WAITING
            .lock()
            .map(|mut w| std::mem::take(&mut *w))
            .unwrap_or_default()
    }

    pub async fn call(method: &str, options: &Value) -> Result<Value, Value> {
        let ctx = NEXT.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        pending()
            .lock()
            .map_err(|_| super::failure("failed", "lock"))?
            .insert(ctx, tx);
        let method = CString::new(method).map_err(|_| super::failure("failed", "method"))?;
        let json =
            CString::new(options.to_string()).map_err(|_| super::failure("failed", "options"))?;
        // SAFETY: both strings outlive the call; the bridge copies them first.
        unsafe { lexpad_store_call(method.as_ptr(), json.as_ptr(), ctx, on_reply) };
        let text = rx
            .await
            .map_err(|_| super::failure("failed", "The App Store did not answer."))?;
        super::parse_reply(&text)
    }
}

/// Starts listening for transactions (the Mac App Store build; nothing elsewhere).
pub fn start(_app: &tauri::AppHandle) {
    #[cfg(all(feature = "mas", target_os = "macos"))]
    bridge::start(_app);
}

async fn call(command: &str, options: Option<Value>) -> Result<Value, Value> {
    let method = METHODS
        .iter()
        .find(|(c, _)| *c == command)
        .map(|(_, m)| *m)
        .ok_or_else(|| failure("failed", "Unknown store command."))?;
    let options = options_of(options);
    #[cfg(all(feature = "mas", target_os = "macos"))]
    {
        bridge::call(method, &options).await
    }
    #[cfg(not(all(feature = "mas", target_os = "macos")))]
    {
        let _ = (method, options);
        Err(failure(
            "unavailable",
            "This copy of Lexpad does not sell through the App Store.",
        ))
    }
}

macro_rules! store_command {
    ($name:ident) => {
        #[tauri::command]
        pub async fn $name(options: Option<Value>) -> Result<Value, Value> {
            call(stringify!($name), options).await
        }
    };
}

store_command!(store_can_make_payments);
store_command!(store_products);
store_command!(store_purchase);
store_command!(store_finish);
store_command!(store_unfinished);
store_command!(store_current_entitlements);
store_command!(store_sync);
store_command!(store_manage_subscriptions);
store_command!(store_app_transaction_id);

/// The transactions that arrived before the window listened, handed over once.
#[tauri::command]
pub async fn store_take_transactions(options: Option<Value>) -> Result<Value, Value> {
    let _ = options;
    #[cfg(all(feature = "mas", target_os = "macos"))]
    {
        Ok(json!({ "transactions": bridge::take() }))
    }
    #[cfg(not(all(feature = "mas", target_os = "macos")))]
    {
        Err(failure(
            "unavailable",
            "This copy of Lexpad does not sell through the App Store.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_is_its_value_or_the_plugins_error() {
        assert_eq!(
            parse_reply(r#"{"ok":{"allowed":true}}"#),
            Ok(json!({"allowed": true}))
        );
        assert_eq!(parse_reply(r#"{"ok":null}"#), Ok(Value::Null));
        let err =
            parse_reply(r#"{"error":{"code":"cancelled","message":"m","data":{"detail":"d"}}}"#)
                .unwrap_err();
        assert_eq!(err["code"], "cancelled");
        assert_eq!(err["data"]["detail"], "d");
        assert_eq!(parse_reply("not json").unwrap_err()["code"], "failed");
        assert_eq!(parse_reply("{}").unwrap_err()["code"], "failed");
    }

    /// The web app reads `isEligibleForIntroOffer` per product and shows no
    /// Apple offer price without it, so the core passes it through untouched.
    #[test]
    fn a_products_reply_keeps_the_intro_offer_eligibility() {
        let products = parse_reply(
            r#"{"ok":{"products":[{"id":"pro_yearly_welcome","isEligibleForIntroOffer":false}]}}"#,
        )
        .unwrap();
        assert_eq!(
            products["products"][0]["isEligibleForIntroOffer"],
            Value::Bool(false)
        );
    }

    #[test]
    fn options_are_an_object_or_nothing() {
        assert_eq!(options_of(None), json!({}));
        assert_eq!(options_of(Some(json!([1]))), json!({}));
        assert_eq!(options_of(Some(json!({"a": 1}))), json!({"a": 1}));
    }

    #[test]
    fn arrived_transactions_are_kept_once_and_capped() {
        let mut w = Vec::new();
        keep(&mut w, json!({"transactionId": "1"}));
        keep(&mut w, json!({"transactionId": "1"}));
        assert_eq!(w.len(), 1);
        for i in 2..=(MAX_WAITING as u32 + 5) {
            keep(&mut w, json!({"transactionId": i.to_string()}));
        }
        assert_eq!(w.len(), MAX_WAITING);
        assert_eq!(
            w.last().unwrap()["transactionId"],
            (MAX_WAITING as u32 + 5).to_string()
        );
    }

    #[test]
    fn every_command_names_a_plugin_method() {
        for (command, method) in METHODS {
            assert!(command.starts_with("store_"));
            assert!(!method.is_empty());
        }
        assert_eq!(METHODS.len(), 9);
    }

    #[cfg(not(feature = "mas"))]
    #[test]
    fn the_dmg_and_windows_sell_nothing() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt.block_on(store_products(None)).unwrap_err();
        assert_eq!(err["code"], "unavailable");
        let err = rt.block_on(store_take_transactions(None)).unwrap_err();
        assert_eq!(err["code"], "unavailable");
    }
}
