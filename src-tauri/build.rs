fn main() {
    // The media engine finds its development FFmpeg by target triple
    // (src-tauri/binaries/ffmpeg-<triple>, as Tauri names sidecars).
    println!(
        "cargo:rustc-env=LEXPAD_TARGET_TRIPLE={}",
        std::env::var("TARGET").expect("cargo sets TARGET")
    );
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
            "open_report",
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
            "latest_release",
            // The video player's media engine (src/media/commands.rs).
            "media_engine",
            "media_pick",
            "media_take_pending",
            "media_probe",
            "media_open",
            "media_seek",
            "media_set_audio",
            "media_close",
            "media_subtitles",
            "media_subtitle_load",
            "media_subtitle_offset",
            "media_episodes",
            "media_resume_save",
            // Only in a build with the `player-dev` feature (never shipped).
            "media_dev_report",
            "media_dev_bench_files",
            "media_default_player_status",
            "media_default_player_request",
            // Words saved before they can reach the account (word_queue.rs).
            "words_save",
            "words_pending",
            "words_flush",
            "words_discard",
        ]),
    ))
    .expect("failed to run tauri-build");
}
