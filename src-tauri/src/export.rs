//! Saving a file the learner exports from Lexpad's window (Settings →
//! Import & export → Export) into the Downloads folder.
//!
//! This is the only thing Lexpad writes outside its own data, and in the Mac
//! App Store copy it is what `com.apple.security.files.downloads.read-write`
//! is for (mas/Lexpad.entitlements). The page names the file and hands over
//! its bytes; the core decides where it goes:
//!
//! - always the Downloads folder, never a path the page chose;
//! - only a plain file name ending in one of [`KINDS`], so the page can never
//!   leave something that runs there;
//! - never over a file that is already there: a second export on the same day
//!   becomes `name (2).json`.
//!
//! The page then says where the file is, and may ask for it to be shown in
//! Finder or Explorer ([`show_export`]), by that same checked name.

use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt as _;

use crate::api::Failure;

/// The kinds of file an export may be, by extension (lower case).
const KINDS: [&str; 3] = ["json", "csv", "txt"];
/// The longest name accepted, in bytes: well inside every file system's 255.
const NAME_MAX: usize = 120;
/// How many numbered names are tried before giving up.
const TRIES: u32 = 999;

/// The name as it may be written: a plain file name of one of [`KINDS`],
/// or nothing if it is anything else (a path, a hidden file, another kind).
pub fn safe_name(name: &str) -> Option<&str> {
    let plain = !name.is_empty()
        && name.len() <= NAME_MAX
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ' | '(' | ')'));
    let (stem, kind) = name.rsplit_once('.')?;
    (plain && !stem.is_empty() && KINDS.contains(&kind.to_ascii_lowercase().as_str()))
        .then_some(name)
}

/// The `n`th name for a file: itself first, then `stem (2).kind`, …
fn numbered(name: &str, n: u32) -> String {
    if n <= 1 {
        return name.to_owned();
    }
    match name.rsplit_once('.') {
        Some((stem, kind)) if !stem.is_empty() => format!("{stem} ({n}).{kind}"),
        _ => format!("{name} ({n})"),
    }
}

/// A path in `dir` for `name` that nothing has yet: the name itself, or the
/// first numbered one that is free. For a download the web view writes itself
/// (main_window.rs), which fails on macOS if the file is already there.
pub fn free_path(dir: &Path, name: &str) -> PathBuf {
    (1..=TRIES)
        .map(|n| dir.join(numbered(name, n)))
        .find(|path| !path.exists())
        .unwrap_or_else(|| dir.join(name))
}

/// Writes `bytes` to a new file in `dir` and gives back the name it got.
/// The file is made with "create new", so two exports at once cannot write
/// over each other.
pub fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<String> {
    for n in 1..=TRIES {
        let candidate = numbered(name, n);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(&candidate))
        {
            Ok(mut file) => {
                file.write_all(bytes)?;
                file.sync_all()?;
                return Ok(candidate);
            }
            Err(cause) if cause.kind() == io::ErrorKind::AlreadyExists => {}
            Err(cause) => return Err(cause),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free name for the export",
    ))
}

/// Saves an export from Lexpad's window to the Downloads folder and answers
/// with the file's name there (it may have been numbered). `data` is the
/// file's bytes in base64.
#[tauri::command]
pub fn save_export(app: AppHandle, name: String, data: String) -> Result<String, String> {
    let name = safe_name(&name).ok_or_else(refused)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| refused())?;
    let dir = app.path().download_dir().map_err(|_| refused())?;
    write_new(&dir, name, &bytes).map_err(|_| refused())
}

/// Shows an export in the system's file manager (Finder, Explorer) with the
/// file selected: the "Show in Finder" beside the page's "Saved" line. Only a
/// name [`save_export`] could have written, and only if that file is there.
#[tauri::command]
pub fn show_export(app: AppHandle, name: String) -> Result<(), String> {
    let name = safe_name(&name).ok_or_else(refused)?;
    let path = app.path().download_dir().map_err(|_| refused())?.join(name);
    if !path.is_file() {
        return Err(refused());
    }
    app.opener().reveal_item_in_dir(path).map_err(|_| refused())
}

/// The windows' word for "it did not work" (`Failure::Error`).
fn refused() -> String {
    Failure::Error.as_str().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lexpad-export-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_a_plain_export_name_is_accepted() {
        for good in [
            "lexpad-english-2026-11-22.json",
            "lexpad-notebook-2026-11-22.JSON",
            "words (2).csv",
            "notes_1.txt",
            "lexpad-français-2026-11-22.json",
        ] {
            assert_eq!(safe_name(good), Some(good), "{good}");
        }
        for bad in [
            "",
            ".json",
            ".hidden.json",
            "export",
            "export.",
            "run.sh",
            "Lexpad.app",
            "page.html",
            "../export.json",
            "a/b.json",
            "a\\b.json",
            "C:evil.json",
            "export.json\0",
            "export\n.json",
        ] {
            assert_eq!(safe_name(bad), None, "{bad:?}");
        }
        assert_eq!(safe_name(&format!("{}.json", "a".repeat(NAME_MAX))), None);
    }

    #[test]
    fn a_second_export_gets_a_number_and_nothing_is_written_over() {
        let dir = scratch("number");
        assert_eq!(
            write_new(&dir, "lexpad-english.json", b"one").unwrap(),
            "lexpad-english.json"
        );
        assert_eq!(
            write_new(&dir, "lexpad-english.json", b"two").unwrap(),
            "lexpad-english (2).json"
        );
        assert_eq!(
            write_new(&dir, "lexpad-english.json", b"three").unwrap(),
            "lexpad-english (3).json"
        );
        assert_eq!(
            std::fs::read(dir.join("lexpad-english.json")).unwrap(),
            b"one"
        );
        assert_eq!(
            std::fs::read(dir.join("lexpad-english (2).json")).unwrap(),
            b"two"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_download_is_given_a_path_nothing_has() {
        let dir = scratch("free");
        assert_eq!(free_path(&dir, "export.json"), dir.join("export.json"));
        std::fs::write(dir.join("export.json"), b"x").unwrap();
        assert_eq!(free_path(&dir, "export.json"), dir.join("export (2).json"));
        assert_eq!(free_path(&dir, "Lexpad export"), dir.join("Lexpad export"));
        std::fs::write(dir.join("Lexpad export"), b"x").unwrap();
        assert_eq!(
            free_path(&dir, "Lexpad export"),
            dir.join("Lexpad export (2)")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_folder_that_is_not_there_is_an_error_not_a_loop() {
        let gone = std::env::temp_dir().join("lexpad-export-no-such-folder");
        let _ = std::fs::remove_dir_all(&gone);
        assert!(write_new(&gone, "export.json", b"x").is_err());
    }
}
