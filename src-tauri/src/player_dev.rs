//! The video player's development page (`src/player-dev.html`), only in a
//! build with the `player-dev` feature, never shipped: a `<video>` with
//! Media Source Extensions and a plain subtitle overlay (three tracks, each
//! with its own offset), to prove the media engine before the real player
//! is designed; and a benchmark (`scripts/player-bench.sh`).
//!
//!   LEXPAD_PLAYER_DEV=1 pnpm tauri dev --features player-dev
//!
//! It opens at start, and files opened with Lexpad go to it.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "player-dev";

pub fn open(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        #[cfg(target_os = "macos")]
        let _ = handle.set_activation_policy(tauri::ActivationPolicy::Regular);
        if let Some(w) = handle.get_webview_window(LABEL) {
            let _ = w.show();
            let _ = w.set_focus();
            return;
        }
        let built =
            WebviewWindowBuilder::new(&handle, LABEL, WebviewUrl::App("player-dev.html".into()))
                .title("Lexpad player (development)")
                .inner_size(1280.0, 860.0)
                .min_inner_size(640.0, 480.0)
                .build();
        if let Err(e) = built {
            log::warn!("could not open the player's development page: {e}");
        }
    });
}
