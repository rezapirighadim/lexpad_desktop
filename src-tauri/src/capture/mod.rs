//! Reading what is selected in the app in front, when the shortcut is pressed.
//!
//! Each platform asks the accessibility interface first (macOS: the focused
//! element's `AXSelectedText`; Windows: UI Automation's TextPattern
//! selection), which reads the selection without touching anything. Where
//! that gives nothing, it copies: the clipboard is saved, ⌘C / Ctrl+C is sent
//! to the app in front, the text is read, and the clipboard is put back as it
//! was. A password field is never read.
//!
//! What leaves this module, and why:
//! - the selected text, cut to `MAX_TEXT` characters;
//! - the text around it, when the accessibility interface gives it, cut to a
//!   window around the selection: the popup takes the one sentence the word
//!   is in from it, and only that sentence is ever sent, as the extension
//!   sends the sentence from a page;
//! - the app's display name ("Microsoft Word"), for the private note
//!   "Seen in Microsoft Word". Never a window title: titles carry document
//!   names, e-mail subjects and chat partners.

use serde::Serialize;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// The most of a selection that is passed on. Four words fit many times over;
/// anything longer is offered as a sentence to pick a word from.
pub const MAX_TEXT: usize = 1000;
/// How much text around the selection is kept, each side, in characters.
pub const CONTEXT_EACH_SIDE: usize = 600;

/// Whether the app may read other apps' selections.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// macOS only: Windows needs no permission (NotNeeded).
    #[cfg_attr(windows, allow(dead_code))]
    Granted,
    /// macOS: Accessibility is not allowed yet. The popup explains and offers
    /// the type-a-word box.
    Missing,
    /// Windows needs no permission for either path.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    NotNeeded,
}

/// How the text was read.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Via {
    Accessibility,
    Clipboard,
    /// The macOS Services menu.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Service,
    None,
}

/// A rectangle on screen, in logical points, top-left origin.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Capture {
    pub text: Option<String>,
    pub context: Option<String>,
    pub app: Option<String>,
    pub permission: Permission,
    pub via: Via,
    /// Where the selection is on screen, when the accessibility interface says.
    #[serde(skip)]
    pub anchor: Option<Rect>,
}

impl Capture {
    pub fn empty(permission: Permission, app: Option<String>) -> Self {
        Self {
            text: None,
            context: None,
            app,
            permission,
            via: Via::None,
            anchor: None,
        }
    }
}

/// Reads the selection in the app in front. Blocking (up to about half a
/// second on the clipboard path); call it off the UI thread.
pub fn capture() -> Capture {
    #[cfg(target_os = "macos")]
    return macos::capture();
    #[cfg(windows)]
    return windows::capture();
    #[cfg(not(any(target_os = "macos", windows)))]
    return Capture::empty(Permission::NotNeeded, None);
}

/// Whether reading selections is allowed, without asking.
pub fn permission() -> Permission {
    #[cfg(target_os = "macos")]
    return macos::permission();
    #[cfg(not(target_os = "macos"))]
    return Permission::NotNeeded;
}

/// Asks the system to list the app under Accessibility (macOS shows its own
/// prompt). Returns whether it is allowed already.
pub fn request_permission() -> bool {
    #[cfg(target_os = "macos")]
    return macos::request_permission();
    #[cfg(not(target_os = "macos"))]
    return true;
}

/// The settings page where Accessibility is allowed, if the platform has one.
pub fn permission_settings_url() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
    } else {
        None
    }
}

/// Cuts text to at most `max` characters, on a character boundary.
pub fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => text[..i].to_owned(),
        None => text.to_owned(),
    }
}

/// The part of `all` around the selection that starts at character `start`
/// and runs `len` characters, with `each_side` characters either side.
/// (macOS's accessibility interface gives the text around a selection.)
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn window_around(all: &[char], start: usize, len: usize, each_side: usize) -> String {
    let start = start.min(all.len());
    let end = (start + len).min(all.len());
    let from = start.saturating_sub(each_side);
    let to = (end + each_side).min(all.len());
    all[from..to].iter().collect()
}

/// A selection worth passing on: trimmed, not empty.
pub fn tidy(text: &str) -> Option<String> {
    let t = text.trim();
    (!t.is_empty()).then(|| clip(t, MAX_TEXT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_on_character_boundaries() {
        assert_eq!(clip("کتاب‌ها", 3), "کتا");
        assert_eq!(clip("abc", 10), "abc");
    }

    #[test]
    fn keeps_a_window_around_the_selection() {
        let all: Vec<char> = "one two three four five".chars().collect();
        assert_eq!(window_around(&all, 8, 5, 4), "two three fou");
        assert_eq!(window_around(&all, 0, 3, 100), "one two three four five");
        assert_eq!(window_around(&all, 99, 3, 2), "ve");
    }

    #[test]
    fn an_empty_selection_is_none() {
        assert_eq!(tidy("  \n "), None);
        assert_eq!(tidy("  word "), Some("word".into()));
    }

    #[test]
    fn the_anchor_never_leaves_the_device() {
        let c = Capture {
            anchor: Some(Rect {
                x: 1.0,
                y: 2.0,
                w: 3.0,
                h: 4.0,
            }),
            ..Capture::empty(Permission::Granted, Some("TextEdit".into()))
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(!json.contains("anchor"));
        assert!(json.contains("\"app\":\"TextEdit\""));
    }
}
