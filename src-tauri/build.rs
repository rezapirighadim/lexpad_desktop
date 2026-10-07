fn main() {
    // Every command the webviews (popup, panel, Settings, Lexpad's window) may call is listed here, so each window gets
    // only the ones its capability grants (capabilities/*.json). A command
    // not listed is not callable from any window at all.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "state",
            "lookup",
            "add_word",
            "set_notebook",
            "connect",
            "cancel_connect",
            "disconnect",
            "hide_popup",
            "fit_popup",
            "accessibility",
            "open_accessibility_settings",
            "get_settings",
            "set_shortcut",
            "set_start_on_login",
            "open_settings",
            "app_info",
            "recent",
            "panel_add",
            "hide_panel",
            "fit_panel",
            "open_web",
            "quit",
            "api_fetch",
            "main_signed_in",
            "main_take_pending",
            "open_in_browser",
            "set_open_on_launch",
            "set_open_in_browser",
            "desktop_settings",
            "set_notifications",
            "schedule_reminders",
            "open_notification_settings",
        ]),
    ))
    .expect("failed to run tauri-build");
}
