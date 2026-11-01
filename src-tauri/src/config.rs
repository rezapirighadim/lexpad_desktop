//! Where the app talks to. Production by default; a development build points
//! at a local stack with `LEXPAD_API_ORIGIN` and `LEXPAD_APP_ORIGIN` at build
//! time, exactly like the browser extension's build.

/// The API, without a trailing slash.
pub const API_ORIGIN: &str = match option_env!("LEXPAD_API_ORIGIN") {
    Some(v) => v,
    None => "https://api.lexpad.app",
};

/// The web app, which serves the connect page.
pub const APP_ORIGIN: &str = match option_env!("LEXPAD_APP_ORIGIN") {
    Some(v) => v,
    None => "https://app.lexpad.app",
};

/// The web app's page that connects this app (RFC 8252 sign-in).
pub const CONNECT_PATH: &str = "/connect-desktop";

/// The app's version, from package.json through tauri.conf.json.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A side-by-side test build of the video player (`--features player-test`,
/// with `src-tauri/tauri.playertest.conf.json`; README, "A player test build").
/// It is its own app next to an installed Lexpad: its own bundle identifier,
/// credential store entry, settings and shortcut, and it never adds itself to
/// the login items. Never in a shipped build.
pub const PLAYER_TEST: bool = cfg!(feature = "player-test");

/// The Mac App Store build (`--features mas`, with src-tauri/tauri.mas.conf.json;
/// docs/mac-app-store-plan.md): sandboxed, sold through Apple's in-app
/// purchase, updated by the App Store, and the selection reaches it through
/// the Services menu's key (⌘⇧L) instead of Accessibility. Only on macOS.
pub const MAS: bool = cfg!(all(feature = "mas", target_os = "macos"));

/// The bundle identifier, for what must name it outside Tauri's config (the
/// credential store's service, the system's notification settings page).
/// It equals `identifier` in tauri.conf.json (or the overlay of the test
/// build or of the Mac App Store build). The Mac App Store build carries the
/// iPhone app's identifier: it is the macOS platform of the same App Store
/// record (Universal Purchase), so the same subscriptions serve both.
pub const BUNDLE_ID: &str = if PLAYER_TEST {
    "app.lexpad.desktop.playertest"
} else if MAS {
    BUNDLE_ID_MAS
} else {
    "app.lexpad.desktop"
};

/// The Mac App Store build's identifier (tauri.mas.conf.json).
pub const BUNDLE_ID_MAS: &str = "app.lexpad.ios";

/// Whether this build talks to anything but production. Such a build never
/// registers itself to start at login: a test build pointing at a laptop's
/// API must not end up in somebody's login items.
pub fn is_development_build() -> bool {
    cfg!(debug_assertions) || API_ORIGIN != "https://api.lexpad.app"
}

/// The operating system, as the API's Signed-in devices list shows it.
pub fn device_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(windows) {
        "Windows"
    } else {
        "Linux"
    }
}

/// Where this copy was installed from, on Windows: the Microsoft Store's MSIX
/// package (it has a package identity) or the installer from lexpad.app. On a
/// Mac, `appstore` for the Mac App Store build (the API sells it through
/// Apple: lexpad_back `everywhere.MacAppStore`), nothing for the dmg.
pub fn install_source() -> Option<&'static str> {
    if MAS {
        return Some("appstore");
    }
    #[cfg(windows)]
    {
        Some(if crate::msix::is_packaged() {
            "store"
        } else {
            "direct"
        })
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// The User-Agent of every call to the API: the app, its build, the system and,
/// on Windows, where it was installed from, e.g. `Lexpad-Desktop/0.3.11
/// (Windows; store)`. The server keeps it with the session it opens, so the
/// operator can tell a Mac from a Windows copy and the Store's from the
/// direct download. Nothing about the person or the machine beyond that.
pub fn user_agent() -> String {
    client_string(VERSION, device_name(), install_source())
}

fn client_string(version: &str, os: &str, install: Option<&str>) -> String {
    match install {
        Some(source) => format!("Lexpad-Desktop/{version} ({os}; {source})"),
        None => format!("Lexpad-Desktop/{version} ({os})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mac_app_store_copy_says_so() {
        assert_eq!(
            client_string("0.3.27", "macOS", Some("appstore")),
            "Lexpad-Desktop/0.3.27 (macOS; appstore)"
        );
        assert_eq!(install_source() == Some("appstore"), MAS);
        assert_eq!(BUNDLE_ID == BUNDLE_ID_MAS, MAS);
    }

    #[test]
    fn the_user_agent_names_the_system_and_the_install() {
        assert_eq!(
            client_string("0.3.11", "Windows", Some("store")),
            "Lexpad-Desktop/0.3.11 (Windows; store)"
        );
        assert_eq!(
            client_string("0.3.11", "Windows", Some("direct")),
            "Lexpad-Desktop/0.3.11 (Windows; direct)"
        );
        assert_eq!(
            client_string("0.3.11", "macOS", None),
            "Lexpad-Desktop/0.3.11 (macOS)"
        );
        assert!(user_agent().starts_with(&format!("Lexpad-Desktop/{VERSION} (")));
    }
}
