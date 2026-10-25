//! "Use Lexpad for video files": whether Lexpad opens a type by default,
//! and making it so, only when the learner asks (the page confirms first).
//!
//! - macOS: Launch Services. The types are declared (Info.plist and
//!   tauri.macos.conf.json, rank Alternate, so installing never takes them);
//!   `request` sets Lexpad as the handler of each chosen type's UTI with
//!   `LSSetDefaultRoleHandlerForContentType`, which works from macOS 11 (the
//!   app's minimum; `NSWorkspace.setDefaultApplication(at:toOpen:)` is 12+).
//! - Windows: setting a default silently is forbidden. The installer
//!   registers Lexpad's ProgID, "Open with" entries and its Default Apps
//!   capabilities (windows/media-open-with.nsh; the MSIX manifest for the
//!   Store's copy); `request` opens Settings > Default apps at Lexpad's page,
//!   where the learner picks. `status` reads what Explorer would run for
//!   each type (`AssocQueryStringW`).

use serde::Serialize;

/// The types the player offers to open by default. `ts` is also TypeScript
/// source: the page offers it unticked (`caution`).
pub const VIDEO_TYPES: &[&str] = &[
    "mkv", "mp4", "avi", "mov", "webm", "m4v", "ts", "wmv", "flv",
];
pub const SUBTITLE_TYPES: &[&str] = &["srt", "ass", "vtt"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeStatus {
    pub ext: &'static str,
    pub subtitle: bool,
    /// Lexpad opens it by default now.
    pub is_default: bool,
    /// The app that does, when it is another (its name or bundle id).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    /// Shared with something else people may not mean (TypeScript's `.ts`).
    pub caution: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub types: Vec<TypeStatus>,
    /// "direct": `request` sets it (macOS). "systemSettings": `request`
    /// opens the system's page and the learner picks there (Windows).
    pub method: &'static str,
}

/// The known types among `asked` (all when empty), lowercase, no dot.
pub fn known(asked: &[String]) -> Vec<&'static str> {
    let all = VIDEO_TYPES.iter().chain(SUBTITLE_TYPES);
    if asked.is_empty() {
        return all.copied().collect();
    }
    all.copied()
        .filter(|t| {
            asked
                .iter()
                .any(|a| a.trim().trim_start_matches('.').eq_ignore_ascii_case(t))
        })
        .collect()
}

fn type_status(ext: &'static str, current: Option<String>, mine: bool) -> TypeStatus {
    TypeStatus {
        ext,
        subtitle: SUBTITLE_TYPES.contains(&ext),
        is_default: mine,
        current: current.filter(|_| !mine),
        caution: ext == "ts",
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        static kUTTagClassFilenameExtension: CFStringRef;
        fn UTTypeCreatePreferredIdentifierForTag(
            tag_class: CFStringRef,
            tag: CFStringRef,
            conforming_to: CFStringRef,
        ) -> CFStringRef;
        fn LSCopyDefaultRoleHandlerForContentType(
            content_type: CFStringRef,
            role: u32,
        ) -> CFStringRef;
        fn LSSetDefaultRoleHandlerForContentType(
            content_type: CFStringRef,
            role: u32,
            bundle_id: CFStringRef,
        ) -> i32;
    }
    /// kLSRolesAll: viewer and editor alike.
    const ROLES_ALL: u32 = 0xFFFF_FFFF;

    pub fn uti(ext: &str) -> Option<CFString> {
        let tag = CFString::new(ext);
        // SAFETY: CoreServices' documented call; a +1 reference is returned
        // (or null), owned by the wrapper.
        let r = unsafe {
            UTTypeCreatePreferredIdentifierForTag(
                kUTTagClassFilenameExtension,
                tag.as_concrete_TypeRef(),
                std::ptr::null(),
            )
        };
        (!r.is_null()).then(|| unsafe { CFString::wrap_under_create_rule(r) })
    }

    pub fn handler(ext: &str) -> Option<String> {
        let t = uti(ext)?;
        // SAFETY: as above; a copied (+1) string or null.
        let r =
            unsafe { LSCopyDefaultRoleHandlerForContentType(t.as_concrete_TypeRef(), ROLES_ALL) };
        (!r.is_null()).then(|| unsafe { CFString::wrap_under_create_rule(r) }.to_string())
    }

    pub fn set(ext: &str, bundle_id: &str) -> Result<(), i32> {
        let t = uti(ext).ok_or(-1)?;
        let id = CFString::new(bundle_id);
        // SAFETY: both strings live for the call.
        let status = unsafe {
            LSSetDefaultRoleHandlerForContentType(
                t.as_concrete_TypeRef(),
                ROLES_ALL,
                id.as_concrete_TypeRef(),
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(status)
        }
    }
}

#[cfg(windows)]
mod imp {
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_NONE, ASSOCSTR_EXECUTABLE};

    /// What Explorer runs for `.ext`, as a path.
    pub fn handler(ext: &str) -> Option<String> {
        let ext = HSTRING::from(format!(".{ext}"));
        let verb = HSTRING::from("open");
        let mut len: u32 = 1024;
        let mut buf = vec![0u16; len as usize];
        // SAFETY: the buffer and its length are ours; the strings live for the call.
        let hr = unsafe {
            AssocQueryStringW(
                ASSOCF_NONE,
                ASSOCSTR_EXECUTABLE,
                PCWSTR(ext.as_ptr()),
                PCWSTR(verb.as_ptr()),
                Some(PWSTR(buf.as_mut_ptr())),
                &mut len,
            )
        };
        if hr.is_err() {
            return None;
        }
        let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..n]))
    }

    pub fn is_me(path: &str) -> bool {
        let Ok(me) = std::env::current_exe() else {
            return false;
        };
        let me = me.to_string_lossy().to_lowercase();
        let path = path.to_lowercase();
        // The Store's copy runs from WindowsApps under another path name
        // than the one Explorer reports: the file name decides then.
        path == me
            || std::path::Path::new(&path).file_name() == std::path::Path::new(&me).file_name()
                && path.contains("lexpad")
    }
}

/// Whether Lexpad opens each of `types` by default.
pub fn status(bundle_id: &str, types: &[&'static str]) -> Status {
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = bundle_id;
    let types = types
        .iter()
        .map(|ext| {
            #[cfg(target_os = "macos")]
            {
                let h = imp::handler(ext);
                let mine = h
                    .as_deref()
                    .is_some_and(|h| h.eq_ignore_ascii_case(bundle_id));
                type_status(ext, h, mine)
            }
            #[cfg(windows)]
            {
                let _ = bundle_id;
                let h = imp::handler(ext);
                let mine = h.as_deref().is_some_and(imp::is_me);
                type_status(ext, h, mine)
            }
            #[cfg(not(any(target_os = "macos", windows)))]
            {
                type_status(ext, None, false)
            }
        })
        .collect();
    Status {
        types,
        method: if cfg!(windows) {
            "systemSettings"
        } else {
            "direct"
        },
    }
}

/// macOS: makes Lexpad the default for `types`; the types it could not
/// set come back with their Launch Services error. Elsewhere: nothing
/// (Windows goes through `settings_page`).
pub fn set_default(bundle_id: &str, types: &[&'static str]) -> Vec<(&'static str, i32)> {
    #[cfg(target_os = "macos")]
    {
        types
            .iter()
            .filter_map(|ext| imp::set(ext, bundle_id).err().map(|e| (*ext, e)))
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (bundle_id, types);
        Vec::new()
    }
}

/// Windows: the Default apps page, at Lexpad's own entry when the installer
/// registered it (RegisteredApplications "Lexpad"), else the page's top.
pub fn settings_page(packaged: bool) -> &'static str {
    if packaged {
        "ms-settings:defaultapps"
    } else {
        "ms-settings:defaultapps?registeredAppUser=Lexpad"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_types_only() {
        assert_eq!(
            known(&[".MKV".into(), "exe".into(), "srt".into()]),
            ["mkv", "srt"]
        );
        assert_eq!(known(&[]).len(), VIDEO_TYPES.len() + SUBTITLE_TYPES.len());
        let s = status("app.lexpad.desktop", &["ts", "srt"]);
        assert!(s.types[0].caution && !s.types[1].caution);
        assert!(s.types[1].subtitle);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_knows_the_video_types() {
        // Every video type has a real UTI on any Mac (mkv and webm through
        // the app's own import once installed; through other apps here).
        assert_eq!(imp::uti("mp4").unwrap().to_string(), "public.mpeg-4");
        assert_eq!(
            imp::uti("mov").unwrap().to_string(),
            "com.apple.quicktime-movie"
        );
    }
}
