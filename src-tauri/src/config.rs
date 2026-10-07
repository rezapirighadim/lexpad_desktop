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
