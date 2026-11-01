//! macOS: "Add to Lexpad" in the Services menu (right-click a selection,
//! Services, Add to Lexpad), in every app that offers Services for text.
//!
//! Info.plist declares the service (`NSServices`: message `addToLexpad`,
//! text in); this registers the object that receives it. The selected text
//! arrives on a private pasteboard, so the user's clipboard is not touched
//! and no Accessibility permission is needed for this path.
//!
//! macOS lists a new service only after it has seen the app in
//! /Applications (or ~/Applications) once; a build run from elsewhere may not
//! show it until it is installed. The user can also switch it off or give it
//! a key in System Settings, Keyboard, Keyboard Shortcuts, Services.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSPasteboard, NSPasteboardTypeString, NSRunningApplication, NSWorkspace,
};
use objc2_foundation::{NSBundle, NSString};
use tauri::AppHandle;

use crate::capture::{tidy, Capture, Via};
use crate::services_live::{self, ServiceState, Step};

static APP: OnceLock<AppHandle> = OnceLock::new();
/// This run's probe token (services_live.rs); empty until probing starts.
static TOKEN: OnceLock<String> = OnceLock::new();
static ANSWERED: AtomicBool = AtomicBool::new(false);
static LIVE: Mutex<ServiceState> = Mutex::new(ServiceState::Pending);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "LexpadServicesProvider"]
    struct ServicesProvider;

    unsafe impl NSObjectProtocol for ServicesProvider {}

    impl ServicesProvider {
        #[unsafe(method(addToLexpad:userData:error:))]
        fn add_to_lexpad(&self, pboard: &NSPasteboard, _user_data: Option<&NSString>, _error: *mut *mut NSString) {
            let text = pboard.stringForType(unsafe { NSPasteboardTypeString }).map(|s| s.to_string());
            // This run's own probe (services_live.rs): the service is live; open nothing.
            if services_live::is_probe(text.as_deref(), TOKEN.get().map(String::as_str).unwrap_or("")) {
                ANSWERED.store(true, Ordering::SeqCst);
                return;
            }
            let app = NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .filter(|a| a.processIdentifier() as u32 != std::process::id())
                .and_then(|a| a.localizedName())
                .map(|n| n.to_string());
            let capture = Capture {
                text: text.as_deref().and_then(tidy),
                context: None,
                app,
                permission: crate::capture::permission(),
                via: Via::Service,
                anchor: None,
            };
            if let Some(handle) = APP.get() {
                crate::popup::open(handle, capture);
            }
        }
    }
);

/// Registers the provider. Call once, on the main thread, after launch.
pub fn register(app: &AppHandle) {
    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("services provider must be registered on the main thread");
        return;
    };
    let _ = APP.set(app.clone());
    let provider: Retained<ServicesProvider> =
        unsafe { msg_send![ServicesProvider::alloc(mtm), init] };
    let ns_app = NSApplication::sharedApplication(mtm);
    unsafe { ns_app.setServicesProvider(Some(&provider)) };
    // The application keeps only a weak reference; the provider lives as long as the app.
    std::mem::forget(provider);
    unsafe { NSUpdateDynamicServices() };
    // The Mac App Store build's ⌘⇧L is this service's key: make sure it is
    // live on the first launch too (services_live.rs). The dmg keeps its
    // global shortcut and needs none of it.
    if crate::config::MAS {
        start_probe(app);
    }
}

/// Where the service stands this run (always `ServiceState` outside the Mac App Store build).
pub fn live() -> ServiceState {
    if !crate::config::MAS {
        return ServiceState::Live;
    }
    LIVE.lock().map(|l| *l).unwrap_or(ServiceState::Pending)
}

fn set_live(app: &AppHandle, now: ServiceState) {
    let changed = LIVE
        .lock()
        .map(|mut l| std::mem::replace(&mut *l, now) != now)
        .unwrap_or(false);
    if changed {
        use tauri::Emitter as _;
        let _ = app.emit("services:changed", now);
    }
}

/// The probe loop, off the main thread; each AppKit call runs on it.
fn start_probe(app: &AppHandle) {
    let _ = TOKEN.set(format!("lexpad-probe-{}", uuid::Uuid::new_v4().simple()));
    let app = app.clone();
    std::thread::spawn(move || {
        let mut tries = 0u32;
        loop {
            let other =
                on_main(&app, || shares_the_dmgs_name() && other_copy_running()).unwrap_or(false);
            let step = services_live::next_step(tries, ANSWERED.load(Ordering::SeqCst), other);
            set_live(&app, services_live::live_for(&step, other));
            match step {
                Step::Done => {
                    // What a second launch did: rebuild the list now that it has us.
                    let _ = on_main(&app, || unsafe { NSUpdateDynamicServices() });
                    log::info!("services: Add to Lexpad is live after {tries} probes");
                    // Also on stderr, for scripts/mas-firstrun-test.sh.
                    eprintln!("lexpad: services live after {tries} probes");
                    return;
                }
                Step::GiveUp => {
                    log::warn!("services: Add to Lexpad did not answer");
                    return;
                }
                Step::Wait => {}
                Step::Probe { refresh } => {
                    let _ = on_main(&app, move || {
                        if refresh {
                            unsafe { NSUpdateDynamicServices() };
                        }
                        send_probe();
                    });
                }
            }
            tries += 1;
            std::thread::sleep(services_live::PROBE_EVERY);
        }
    });
}

/// Runs `f` on the main thread and waits for it (at most a few seconds).
fn on_main<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .ok()?;
    rx.recv_timeout(std::time::Duration::from_secs(5)).ok()
}

/// The copy of Lexpad from lexpad.app (the dmg), running now.
fn other_copy_running() -> bool {
    let id = NSString::from_str(DMG_BUNDLE_ID);
    !NSRunningApplication::runningApplicationsWithBundleIdentifier(&id).is_empty()
}

/// The dmg's bundle identifier (tauri.conf.json), which the Mac App Store
/// build cannot read from its own configuration.
const DMG_BUNDLE_ID: &str = "app.lexpad.desktop";

/// The dmg's service name (src-tauri/Info.plist `NSMenuItem`). A probe by
/// this name could reach the dmg's copy, so it waits while that copy runs; a
/// build whose service has another name (scripts/mas-firstrun-test.sh's copy)
/// cannot be confused with it.
const DMG_SERVICE_NAME: &str = "Add to Lexpad";

fn shares_the_dmgs_name() -> bool {
    service_name().is_some_and(|n| n.to_string() == DMG_SERVICE_NAME)
}

/// The service's name as this build's Info.plist gives it (`NSMenuItem`).
fn service_name() -> Option<Retained<NSString>> {
    use objc2::runtime::AnyObject;
    let bundle = NSBundle::mainBundle();
    let services = bundle.objectForInfoDictionaryKey(&NSString::from_str("NSServices"))?;
    // SAFETY: NSServices is an array of dictionaries (Info.plist); each call
    // checks for nil before going on.
    unsafe {
        let first: *mut AnyObject = msg_send![&*services, firstObject];
        let first = first.as_ref()?;
        let item: *mut AnyObject =
            msg_send![first, objectForKey: &*NSString::from_str("NSMenuItem")];
        let item = item.as_ref()?;
        let name: *mut NSString = msg_send![item, objectForKey: &*NSString::from_str("default")];
        Retained::retain(name)
    }
}

/// Sends this run's token to our own service on a pasteboard of its own.
fn send_probe() {
    let (Some(token), Some(name)) = (TOKEN.get(), service_name()) else {
        return;
    };
    let pboard = NSPasteboard::pasteboardWithUniqueName();
    pboard.clearContents();
    let ok = pboard.setString_forType(&NSString::from_str(token), unsafe {
        NSPasteboardTypeString
    });
    if ok {
        // SAFETY: a valid name and pasteboard; the reply is asynchronous.
        unsafe { NSPerformService(&name, Some(&pboard)) };
    }
    // The reply reads the pasteboard later, so the previous probe's is let go
    // now, not this one.
    let previous = LAST_PROBE.with(|last| last.replace(Some(pboard)));
    if let Some(previous) = previous {
        // SAFETY: -releaseGlobally takes nothing and returns nothing.
        let _: () = unsafe { msg_send![&*previous, releaseGlobally] };
    }
}

thread_local! {
    /// The last probe's pasteboard (main thread only).
    static LAST_PROBE: std::cell::RefCell<Option<Retained<NSPasteboard>>> = const { std::cell::RefCell::new(None) };
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    fn NSUpdateDynamicServices();
    fn NSPerformService(
        item_name: &NSString,
        pboard: Option<&NSPasteboard>,
    ) -> objc2::runtime::Bool;
}
