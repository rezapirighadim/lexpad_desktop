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

use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSPasteboard, NSPasteboardTypeString, NSWorkspace};
use objc2_foundation::NSString;
use tauri::AppHandle;

use crate::capture::{tidy, Capture, Via};

static APP: OnceLock<AppHandle> = OnceLock::new();

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
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    fn NSUpdateDynamicServices();
}
