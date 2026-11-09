//! The Mac copies' sign-in through `lexpad://auth/callback` (auth.rs), the
//! parts that ask macOS:
//!
//! - whether this bundle registers the `lexpad` scheme (Info.plist and
//!   Info.mas.plist do; a development run from `cargo` or `pnpm app:dev` has
//!   no bundle, and the player test build does not), so `connect` uses the
//!   scheme only where macOS can deliver it and the loopback otherwise;
//! - passing a callback on to the other Mac copy. The site's dmg
//!   (`app.lexpad.desktop`) and the Mac App Store copy (`app.lexpad.ios`) both
//!   register `lexpad`, and macOS gives a link to one of them. When the one
//!   that got it was not waiting and the other is running, the link goes to
//!   the other, marked so it is never passed back (`auth::forwarded`).

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_foundation::{NSArray, NSBundle, NSDictionary, NSString, NSURL};

/// The scheme in auth::APP_REDIRECT.
fn scheme() -> &'static str {
    crate::auth::APP_REDIRECT
        .split(':')
        .next()
        .unwrap_or_default()
}

/// Whether the running bundle's Info.plist lists the sign-in's scheme under
/// CFBundleURLTypes.
pub fn scheme_registered() -> bool {
    let bundle = NSBundle::mainBundle();
    let Some(types) = bundle.objectForInfoDictionaryKey(&NSString::from_str("CFBundleURLTypes"))
    else {
        return false;
    };
    let Ok(types) = types.downcast::<NSArray>() else {
        return false;
    };
    let key = NSString::from_str("CFBundleURLSchemes");
    types.iter().any(|t: Retained<AnyObject>| {
        let Ok(t) = t.downcast::<NSDictionary>() else {
            return false;
        };
        let Some(schemes) = t.objectForKey(&key) else {
            return false;
        };
        let Ok(schemes) = schemes.downcast::<NSArray>() else {
            return false;
        };
        schemes.iter().any(|s: Retained<AnyObject>| {
            s.downcast::<NSString>()
                .is_ok_and(|s| s.to_string() == scheme())
        })
    })
}

/// The other Mac copy's bundle identifier.
fn other_copy() -> &'static str {
    if crate::config::MAS {
        "app.lexpad.desktop"
    } else {
        crate::config::BUNDLE_ID_MAS
    }
}

/// Passes a callback nobody here waits for to the other Mac copy, when it
/// is running and the link was not already passed on once.
pub fn to_other_copy(raw: &str) {
    let Some(marked) = crate::auth::forwarded(raw) else {
        return;
    };
    let running = NSRunningApplication::runningApplicationsWithBundleIdentifier(
        &NSString::from_str(other_copy()),
    );
    let Some(other) = running.iter().next() else {
        return;
    };
    let (Some(app_url), Some(url)) = (
        other.bundleURL(),
        NSURL::URLWithString(&NSString::from_str(&marked)),
    ) else {
        return;
    };
    let workspace = NSWorkspace::sharedWorkspace();
    let config = objc2_app_kit::NSWorkspaceOpenConfiguration::configuration();
    workspace.openURLs_withApplicationAtURL_configuration_completionHandler(
        &NSArray::from_retained_slice(&[url]),
        &app_url,
        &config,
        None,
    );
    log::info!("a sign-in callback went to the other Lexpad copy, which is running");
}
