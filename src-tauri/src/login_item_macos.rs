//! Start on login in the Mac App Store build: `SMAppService.mainApp`
//! (ServiceManagement, macOS 13). A sandboxed app may not write a LaunchAgent
//! or script System Events, which is what the autostart plugin does for the
//! dmg, and App Review Guideline 2.4.5(iii) wants a login item to be the
//! learner's choice: it is registered only from Settings, never by itself.
//! macOS lists it under System Settings › General › Login Items as "Lexpad".
//!
//! A start by this login item carries no `--login` argument, so the launch
//! table (`welcome::first_window`) treats it as a start by hand and may open
//! a window; the plan (docs/mac-app-store-plan.md) lists it.

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};

#[link(name = "ServiceManagement", kind = "framework")]
extern "C" {}

/// SMAppServiceStatus: 1 is enabled (2 needs approval, 0/3 not registered).
const STATUS_ENABLED: isize = 1;

fn main_app() -> Option<&'static AnyObject> {
    let class = AnyClass::get(c"SMAppService")?;
    // SAFETY: +mainAppService is a class method returning a shared instance.
    let service: *mut AnyObject = unsafe { msg_send![class, mainAppService] };
    // SAFETY: the shared instance lives for the life of the process.
    unsafe { service.as_ref() }
}

/// Whether the login item is on.
pub fn enabled() -> bool {
    let Some(service) = main_app() else {
        return false;
    };
    // SAFETY: -status returns an NSInteger.
    let status: isize = unsafe { msg_send![service, status] };
    status == STATUS_ENABLED
}

/// Turns the login item on or off; says why when macOS refuses.
pub fn set(on: bool) -> Result<(), String> {
    let service = main_app().ok_or("ServiceManagement is not available")?;
    let mut error: *mut AnyObject = std::ptr::null_mut();
    // SAFETY: -registerAndReturnError: / -unregisterAndReturnError: take an
    // NSError** and return BOOL.
    let ok: Bool = unsafe {
        if on {
            msg_send![service, registerAndReturnError: &mut error]
        } else {
            msg_send![service, unregisterAndReturnError: &mut error]
        }
    };
    if ok.as_bool() || (!on && !enabled()) {
        Ok(())
    } else {
        Err("macOS refused the login item".into())
    }
}
