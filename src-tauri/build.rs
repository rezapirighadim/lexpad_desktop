fn main() {
    swift_store();
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
            "phone_used",
            "open_everywhere",
            "panel_watch",
            "video_hint",
            "open_player_pick",
            "open_report",
            "open_plan",
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
            // Apple's in-app purchase, in the Mac App Store build (appstore.rs);
            // elsewhere each answers "unavailable".
            "store_can_make_payments",
            "store_products",
            "store_purchase",
            "store_finish",
            "store_unfinished",
            "store_current_entitlements",
            "store_sync",
            "store_manage_subscriptions",
            "store_app_transaction_id",
            "store_take_transactions",
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
            "media_default_player_restore",
            "media_recent",
            "media_recent_remove",
            "media_recent_clear",
            "media_set_remember_recent",
            "media_prefs",
            // Words saved before they can reach the account (word_queue.rs).
            "words_save",
            "words_pending",
            "words_flush",
            "words_discard",
            "words_send_one",
            // The Player's window and the two windows' ways to each other
            // (player_window.rs, app_menu.rs), and Player home (player_window.rs).
            "open_lexpad",
            "open_player",
            "open_window",
            "player_account",
            "player_fullscreen",
            "player_mini_open",
            "player_back_to_big",
            "player_mini_close",
            "player_on_top",
            "player_buttons",
            "player_take_play",
            "player_context_menu",
            "player_copy_text",
            "set_open_at",
            "open_at",
            "player_home",
            "player_home_note_seen",
            "player_open_clip",
        ]),
    ))
    .expect("failed to run tauri-build");
}

/// The Mac App Store build's Swift: the StoreKit 2 bridge (src/appstore.rs) and
/// the subtitles beside a film (Related.swift, media/files.rs), in
/// swift/LexpadStore, compiled with swiftc into a static library for the
/// target's architecture and linked with StoreKit. Only with `--features mas`
/// on macOS; the dmg and Windows carry no Swift at all.
fn swift_store() {
    let mas = std::env::var_os("CARGO_FEATURE_MAS").is_some();
    let target = std::env::var("TARGET").expect("cargo sets TARGET");
    println!("cargo:rerun-if-changed=swift/LexpadStore/Sources");
    if !mas || !target.ends_with("apple-darwin") {
        return;
    }
    // The oldest macOS the Mac App Store build runs on: StoreKit 2's
    // AppTransaction and SMAppService (start on login) need macOS 13. It is
    // tauri.mas.conf.json's minimumSystemVersion.
    const MACOS_MIN: &str = "13.0";
    let arch = target.split('-').next().expect("a target triple");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let xcrun = |args: &[&str]| -> String {
        let o = std::process::Command::new("xcrun")
            .args(args)
            .output()
            .expect("xcrun (Xcode's command line tools) is needed for the mas feature");
        assert!(o.status.success(), "xcrun {args:?} failed");
        String::from_utf8(o.stdout)
            .expect("utf-8")
            .trim()
            .to_string()
    };
    let sdk = xcrun(&["--sdk", "macosx", "--show-sdk-path"]);
    let status = std::process::Command::new("xcrun")
        .args([
            "swiftc",
            "-parse-as-library",
            "-emit-library",
            "-static",
            "-O",
            "-whole-module-optimization",
            "-module-name",
            "LexpadStore",
            "-target",
            &format!("{arch}-apple-macos{MACOS_MIN}"),
            "-sdk",
            &sdk,
            "swift/LexpadStore/Sources/LexpadStore/LexpadStore.swift",
            "swift/LexpadStore/Sources/LexpadStore/Related.swift",
            "-o",
        ])
        .arg(out.join("libLexpadStore.a"))
        .status()
        .expect("swiftc");
    assert!(status.success(), "swiftc failed for the StoreKit bridge");
    // The Swift runtime is part of macOS since 10.14.4; the toolchain's
    // folder holds the small static compatibility libraries swiftc asks for.
    let swiftc = xcrun(&["--find", "swiftc"]);
    let toolchain = std::path::Path::new(&swiftc)
        .parent()
        .and_then(|p| p.parent())
        .expect("the toolchain")
        .join("lib/swift/macosx");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-search=native={}", toolchain.display());
    println!("cargo:rustc-link-search=native={sdk}/usr/lib/swift");
    println!("cargo:rustc-link-lib=static=LexpadStore");
    println!("cargo:rustc-link-lib=framework=StoreKit");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}
