//! macOS: the UserNotifications framework. Reminders are handed to the
//! system with a time trigger, so they arrive even while Lexpad's window is
//! shut; a click comes back through the delegate and opens the window.
//!
//! The framework works only for an app in its bundle (Lexpad.app): run as a
//! bare binary (tests, `cargo run`) it would throw, so every call checks for
//! a bundle identifier first and reports `Unsupported` without one.

use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSArray, NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNNotificationSettings, UNNotificationSound, UNTimeIntervalNotificationTrigger,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use tauri::AppHandle;

use super::{reminder_key, Access, Planned, REMINDER_IDS};

/// How long to wait for the system to answer a question about permission.
const ANSWER_WAIT: Duration = Duration::from_secs(10);

static APP: OnceLock<AppHandle> = OnceLock::new();

fn bundled() -> bool {
    NSBundle::mainBundle().bundleIdentifier().is_some()
}

fn center() -> Option<Retained<UNUserNotificationCenter>> {
    bundled().then(UNUserNotificationCenter::currentNotificationCenter)
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "LexpadNotificationDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// A click (or any answer) on one of our notifications.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            handler: &block2::DynBlock<dyn Fn()>,
        ) {
            let key = response.notification().request().identifier().to_string();
            if let Some(app) = APP.get() {
                crate::main_window::open(app, Some(super::path_for(&key)));
            }
            handler.call(());
        }

        /// One arriving while Lexpad is in front still shows as a banner.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            handler.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }
    }
);

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let (Some(mtm), Some(center)) = (MainThreadMarker::new(), center()) else {
        return;
    };
    let delegate: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(mtm), init] };
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // The center keeps only a weak reference; the delegate lives as long as the app.
    std::mem::forget(delegate);
}

pub fn access() -> Access {
    let Some(center) = center() else {
        return Access::Unsupported;
    };
    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |settings: std::ptr::NonNull<UNNotificationSettings>| {
        let status = unsafe { settings.as_ref() }.authorizationStatus();
        let _ = tx.send(status);
    });
    center.getNotificationSettingsWithCompletionHandler(&block);
    match rx.recv_timeout(ANSWER_WAIT) {
        Ok(UNAuthorizationStatus::Authorized | UNAuthorizationStatus::Provisional) => {
            Access::Granted
        }
        Ok(UNAuthorizationStatus::Denied) => Access::Denied,
        Ok(_) => Access::NotDetermined,
        Err(_) => Access::Unsupported,
    }
}

pub fn request() -> Access {
    let Some(center) = center() else {
        return Access::Unsupported;
    };
    if access() != Access::NotDetermined {
        return access();
    }
    let (tx, rx) = mpsc::channel();
    let block = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        let _ = tx.send(granted.as_bool());
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &block,
    );
    // The prompt waits for the learner; give them a minute.
    match rx.recv_timeout(Duration::from_secs(60)) {
        Ok(true) => Access::Granted,
        Ok(false) => Access::Denied,
        Err(_) => access(),
    }
}

fn add(center: &UNUserNotificationCenter, key: &str, title: &str, body: &str, after: f64) {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    // A time trigger must be in the future; "now" is a second away.
    let trigger =
        UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(after.max(1.0), false);
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(key),
        &content,
        Some(&trigger),
    );
    center.addNotificationRequest_withCompletionHandler(&request, None);
}

pub fn replace_reminders(plan: &[Planned]) {
    let Some(center) = center() else {
        return;
    };
    let keys: Vec<Retained<NSString>> = REMINDER_IDS
        .map(|id| NSString::from_str(&reminder_key(id)))
        .collect();
    center.removePendingNotificationRequestsWithIdentifiers(&NSArray::from_retained_slice(&keys));
    let now = chrono::Utc::now();
    for p in plan {
        let after = (p.at - now).num_milliseconds() as f64 / 1000.0;
        if after > 0.0 {
            add(&center, &reminder_key(p.id), &p.title, &p.body, after);
        }
    }
}

pub fn show_now(key: &str, title: &str, body: &str) {
    if let Some(center) = center() {
        add(&center, key, title, body, 1.0);
    }
}
