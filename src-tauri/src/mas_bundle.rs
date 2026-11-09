//! Checks of the Mac bundle files (tests only): the Mac App Store build's
//! Info.plist is the dmg's plus the Mac App Store's keys, so nothing the dmg
//! declares may be missing from it, and its configuration must stay sandboxed;
//! both builds ask for the microphone and speech recognition the Player's Say
//! it back needs, and nothing more.

#[cfg(test)]
mod tests {
    const DMG: &str = include_str!("../Info.plist");
    const MAS: &str = include_str!("../Info.mas.plist");
    const MAS_CONF: &str = include_str!("../tauri.mas.conf.json");
    const ENTITLEMENTS: &str = include_str!("../mas/Lexpad.entitlements");
    const HELPER: &str = include_str!("../mas/ffmpeg.entitlements");
    const DMG_ENTITLEMENTS: &str = include_str!("../Lexpad.entitlements");
    const MACOS_CONF: &str = include_str!("../tauri.macos.conf.json");
    /// The microphone, under the sandbox and under the hardened runtime.
    const AUDIO_INPUT: &str = "com.apple.security.device.audio-input";

    fn keys(plist: &str) -> Vec<&str> {
        plist
            .split("<key>")
            .skip(1)
            .filter_map(|s| s.split("</key>").next())
            .collect()
    }

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
            ("Lexpad.entitlements", DMG_ENTITLEMENTS),
        ] {
            for comment in text.split("<!--").skip(1) {
                let body = comment.split("-->").next().unwrap_or("");
                assert!(!body.contains("--"), "{name}: a comment holds `--`");
            }
        }
    }

    #[test]
    fn the_mas_info_plist_keeps_everything_the_dmg_declares() {
        // The URL type's name is each copy's own (both_mac_copies_register_the_sign_in_scheme).
        for s in strings(DMG)
            .into_iter()
            .filter(|s| *s != "app.lexpad.desktop.signin")
        {
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

    /// The sign-in's scheme (auth::APP_REDIRECT) is registered by both Mac
    /// copies, each under its own URL name; the player test build keeps the
    /// loopback (signin_macos::scheme_registered).
    #[test]
    fn both_mac_copies_register_the_sign_in_scheme() {
        let scheme = crate::auth::APP_REDIRECT.split(':').next().unwrap();
        let want = format!("<key>CFBundleURLSchemes</key><array><string>{scheme}</string></array>");
        for (plist, name) in [
            (MAS, "app.lexpad.ios.signin"),
            (DMG, "app.lexpad.desktop.signin"),
        ] {
            let flat: String = plist.split_whitespace().collect();
            assert!(flat.contains("<key>CFBundleURLTypes</key>"));
            assert!(flat.contains(&want));
            assert!(flat.contains(&format!(
                "<key>CFBundleURLName</key><string>{name}</string>"
            )));
        }
        assert!(!include_str!("../Info.playertest.plist").contains("CFBundleURLTypes"));
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
        assert_eq!(
            keys(ENTITLEMENTS),
            [
                "com.apple.security.app-sandbox",
                "com.apple.security.network.client",
                "com.apple.security.files.user-selected.read-only",
                "com.apple.security.files.downloads.read-write",
                AUDIO_INPUT,
                "com.apple.application-identifier",
                "com.apple.developer.team-identifier",
            ]
        );
        assert!(ENTITLEMENTS.contains("<string>6Z95MR26LZ.app.lexpad.ios</string>"));
        // App Review 2.4.5(i): no listening socket in this copy.
        assert!(!ENTITLEMENTS.contains("<key>com.apple.security.network.server</key>"));
        assert_eq!(
            keys(HELPER),
            [
                "com.apple.security.app-sandbox",
                "com.apple.security.inherit"
            ]
        );
    }

    /// Say it back (spec `uiux/2026-11-09-player-line-tools/spec.md` §9, §F1):
    /// WebKit fails with `not-allowed` without the microphone string and with
    /// `service-not-allowed` without the speech one. Apple shows both in its
    /// prompts, so they say only what happens (owner, 9 Nov 2026: no promise
    /// about keeping or storing anything in privacy copy for now).
    #[test]
    fn both_mac_copies_explain_the_microphone_and_speech_recognition() {
        let want = [
            (
                "NSMicrophoneUsageDescription",
                "Say it back listens to you say a line, so Lexpad can show which words came through.",
            ),
            (
                "NSSpeechRecognitionUsageDescription",
                "Say it back turns what you say into text to check it against the subtitle line.",
            ),
        ];
        for (name, plist) in [("Info.plist", DMG), ("Info.mas.plist", MAS)] {
            let flat = plist.replace('\r', "");
            for (key, text) in want {
                assert!(
                    flat.contains(&format!("<key>{key}</key>\n  <string>{text}</string>")),
                    "{name}: {key}"
                );
            }
            let lower = flat.to_lowercase();
            for promise in [
                "keeps nothing",
                "never stores",
                "never keeps",
                "stores nothing",
            ] {
                assert!(!lower.contains(promise), "{name}: \"{promise}\"");
            }
        }
    }

    /// The dmg is signed with the hardened runtime, which keeps the microphone
    /// closed without the entitlement; it asks for that and nothing else. The
    /// Mac App Store build keeps its own file (merged over the macOS config).
    #[test]
    fn the_dmg_asks_for_the_microphone_only() {
        assert_eq!(keys(DMG_ENTITLEMENTS), [AUDIO_INPUT]);
        let flat: String = DMG_ENTITLEMENTS.split_whitespace().collect();
        assert!(flat.contains(&format!("<key>{AUDIO_INPUT}</key><true/>")));
        let flat: String = ENTITLEMENTS.split_whitespace().collect();
        assert!(flat.contains(&format!("<key>{AUDIO_INPUT}</key><true/>")));
        let macos: serde_json::Value = serde_json::from_str(MACOS_CONF).unwrap();
        assert_eq!(
            macos["bundle"]["macOS"]["entitlements"],
            "Lexpad.entitlements"
        );
        let mas: serde_json::Value = serde_json::from_str(MAS_CONF).unwrap();
        assert_eq!(
            mas["bundle"]["macOS"]["entitlements"],
            "mas/Lexpad.entitlements"
        );
    }
}
