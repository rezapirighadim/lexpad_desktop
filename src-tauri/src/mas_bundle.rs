//! Checks of the Mac App Store build's bundle files (tests only): its
//! Info.plist is the dmg's plus the Mac App Store's keys, so nothing the dmg
//! declares may be missing from it, and its configuration must stay sandboxed.

#[cfg(test)]
mod tests {
    const DMG: &str = include_str!("../Info.plist");
    const MAS: &str = include_str!("../Info.mas.plist");
    const MAS_CONF: &str = include_str!("../tauri.mas.conf.json");
    const ENTITLEMENTS: &str = include_str!("../mas/Lexpad.entitlements");
    const HELPER: &str = include_str!("../mas/ffmpeg.entitlements");

    fn strings(plist: &str) -> Vec<&str> {
        plist
            .split("<string>")
            .skip(1)
            .filter_map(|s| s.split("</string>").next())
            .collect()
    }

    /// codesign parses entitlements strictly (AMFI): a `--` inside an XML
    /// comment is not XML and stops the signing, while plutil accepts it.
    #[test]
    fn the_bundle_files_are_strict_xml() {
        for (name, text) in [
            ("Info.mas.plist", MAS),
            ("mas/Lexpad.entitlements", ENTITLEMENTS),
            ("mas/ffmpeg.entitlements", HELPER),
        ] {
            for comment in text.split("<!--").skip(1) {
                let body = comment.split("-->").next().unwrap_or("");
                assert!(!body.contains("--"), "{name}: a comment holds `--`");
            }
        }
    }

    #[test]
    fn the_mas_info_plist_keeps_everything_the_dmg_declares() {
        for s in strings(DMG) {
            assert!(
                MAS.contains(&format!("<string>{s}</string>")),
                "Info.mas.plist lacks {s}"
            );
        }
        assert!(MAS.contains("<key>NSKeyEquivalent</key>"));
        assert!(MAS.contains("<key>NSIsRelatedItemType</key>"));
        // A Windows checkout may turn the file's line ends into CRLF.
        assert!(MAS
            .replace('\r', "")
            .contains("<key>ITSAppUsesNonExemptEncryption</key>\n  <false/>"));
    }

    #[test]
    fn the_mas_document_types_are_the_dmgs_file_associations() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.macos.conf.json")).unwrap();
        for assoc in conf["bundle"]["fileAssociations"].as_array().unwrap() {
            for ext in assoc["ext"].as_array().unwrap() {
                let ext = ext.as_str().unwrap();
                assert!(
                    MAS.contains(&format!("<string>{ext}</string>")),
                    "missing {ext}"
                );
            }
        }
    }

    #[test]
    fn the_mas_build_is_sandboxed_and_asks_for_nothing_more() {
        let conf: serde_json::Value = serde_json::from_str(MAS_CONF).unwrap();
        assert_eq!(conf["identifier"], crate::config::BUNDLE_ID_MAS);
        assert_eq!(conf["app"]["macOSPrivateApi"], false);
        assert_eq!(conf["bundle"]["macOS"]["minimumSystemVersion"], "13.0");
        let keys: Vec<&str> = ENTITLEMENTS
            .split("<key>")
            .skip(1)
            .filter_map(|s| s.split("</key>").next())
            .collect();
        assert_eq!(
            keys,
            [
                "com.apple.security.app-sandbox",
                "com.apple.security.network.client",
                "com.apple.security.network.server",
                "com.apple.security.files.user-selected.read-only",
                "com.apple.security.files.downloads.read-write",
                "com.apple.application-identifier",
                "com.apple.developer.team-identifier",
            ]
        );
        assert!(ENTITLEMENTS.contains("<string>6Z95MR26LZ.app.lexpad.ios</string>"));
        let helper: Vec<&str> = HELPER
            .split("<key>")
            .skip(1)
            .filter_map(|s| s.split("</key>").next())
            .collect();
        assert_eq!(
            helper,
            [
                "com.apple.security.app-sandbox",
                "com.apple.security.inherit"
            ]
        );
    }
}
