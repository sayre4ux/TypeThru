//! The same rules as `Tests/Tests.swift`, plus the shared cases both apps must pass.
#![allow(clippy::unwrap_used)]

use serde_json::Value;
use std::collections::BTreeMap;
use typethru_core::notes::{Piece, layout, parse_changelog, parse_inline, since};
use typethru_core::settings::{Settings, TypingMethod};
use typethru_core::text_rules::{
    self, MarkdownMode, TextFormat, elements, is_line_break, markdown, replace_untypable, unsupported, without_trailing_line_breaks,
};
use typethru_core::typing::{Modifiers, SPEEDS, Shortcut, TypingTest, settle_after, timing};
use typethru_core::update_feed::{self, is_newer, is_release_page, is_signed, newest, numbers, public_key, summary};
use url::Url;

fn repo_file(path: &str) -> String {
    std::fs::read_to_string(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn cases() -> Value {
    serde_json::from_str(&repo_file("shared/test-cases.json")).unwrap()
}

/// A US keyboard: printable ASCII, Tab, and line breaks have keys.
fn typeable(element: &str) -> bool {
    is_line_break(element) || element == "\t" || (element.len() == 1 && (' '..='~').contains(&element.chars().next().unwrap()))
}

mod shared {
    use super::*;

    #[test]
    fn markdown_cases() {
        for case in cases()["markdown"].as_array().unwrap() {
            let result = markdown(case["input"].as_str().unwrap());
            assert_eq!(result.text, case["text"].as_str().unwrap());
            let expected: BTreeMap<usize, Vec<TextFormat>> = case["formats"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| {
                    let formats = v.as_array().unwrap().iter().map(|f| if f == "bold" { TextFormat::Bold } else { TextFormat::Italic }).collect();
                    (k.parse().unwrap(), formats)
                })
                .collect();
            assert_eq!(result.formats, expected, "{case}");
            assert!(result.spans > 0);
        }
    }

    #[test]
    fn markdown_unchanged() {
        for case in cases()["markdownUnchanged"].as_array().unwrap() {
            let input = case.as_str().unwrap();
            let result = markdown(input);
            assert_eq!(result.text, input);
            assert!(result.formats.is_empty(), "{input}");
            assert_eq!(result.spans, 0);
        }
    }

    #[test]
    fn trailing_line_breaks() {
        for case in cases()["trailingLineBreaks"].as_array().unwrap() {
            assert_eq!(without_trailing_line_breaks(case["input"].as_str().unwrap()), case["output"].as_str().unwrap());
        }
    }

    #[test]
    fn replacements() {
        for case in cases()["replacements"].as_array().unwrap() {
            assert_eq!(replace_untypable(case["input"].as_str().unwrap(), typeable).0, case["output"].as_str().unwrap());
        }
    }
}

mod text {
    use super::*;

    #[test]
    fn line_break_pair_is_one_character() {
        assert_eq!(elements("a\r\nb👨‍👩‍👧‍👦e\u{301}"), ["a", "\r\n", "b", "👨‍👩‍👧‍👦", "e\u{301}"]);
    }

    #[test]
    fn unsupported_characters_are_listed_once() {
        assert_eq!(unsupported("a中b中🙂", |e| e.is_ascii()), ["中", "🙂"]);
    }

    #[test]
    fn replacement_only_when_every_character_has_a_key() {
        assert_eq!(replace_untypable("—", |e| e == "a"), ("—".to_owned(), 0));
    }

    #[test]
    fn every_typing_test_is_typeable_on_us_keyboard() {
        for test in TypingTest::ALL {
            let text = replace_untypable(test.text(), typeable).0;
            assert!(unsupported(&text, typeable).is_empty(), "{test:?}");
        }
    }

    #[test]
    fn modes_and_keys() {
        assert_eq!(MarkdownMode::default(), MarkdownMode::Format);
        assert_eq!(TextFormat::Bold.key(), 'b');
        assert_eq!(text_rules::replacement("é"), None);
    }

    #[test]
    fn timing_values() {
        assert_eq!(timing(0.24), (80, 160));
        assert_eq!(timing(0.02), (10, 10));
        assert_eq!(timing(f64::INFINITY), (80, 500));
        assert_eq!(timing(f64::NAN), (80, 160));
        assert_eq!(timing(-5.0), (10, 10));
        assert_eq!(settle_after("\r\n"), 300);
        assert_eq!(settle_after("a"), 0);
        // Every speed gives whole milliseconds, so rounding never matters.
        for speed in SPEEDS {
            let (hold, gap) = timing(speed.interval);
            assert_eq!(f64::from(hold + gap), (speed.interval * 1000.0).round());
        }
    }

    #[test]
    fn shortcuts() {
        assert_eq!(Shortcut::STANDARD.problem(), None);
        assert_eq!(Shortcut::STANDARD.caution(), None);
        assert_eq!(Shortcut::COPY.problem(), None);
        assert_ne!(Shortcut::STANDARD, Shortcut::COPY);
        assert!(Shortcut { modifiers: Modifiers::CONTROL, key: Shortcut::ESCAPE }.problem().is_some());
        assert!(Shortcut { modifiers: Modifiers::SHIFT, key: 0x41 }.problem().is_some());
        assert_eq!(Shortcut { modifiers: Modifiers::NONE, key: Shortcut::F1 + 12 }.problem(), None);
        assert!(Shortcut { modifiers: Modifiers::CONTROL | Modifiers::ALT, key: 0x41 }.caution().is_some());
        assert_eq!(Shortcut::STANDARD.title(Some('\\')), "Ctrl+\\");
        assert_eq!(Shortcut::COPY.title(Some(']')), "Ctrl+]");
        assert_eq!(
            Shortcut { modifiers: Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT | Modifiers::WINDOWS, key: 0x41 }.title(Some('a')),
            "Ctrl+Alt+Shift+Win+A"
        );
        assert_eq!(Shortcut { modifiers: Modifiers::NONE, key: Shortcut::F1 + 12 }.title(None), "F13");
        assert_eq!(Shortcut { modifiers: Modifiers::CONTROL, key: 0xE2 }.title(None), "Ctrl+Key E2");
    }
}

mod settings {
    use super::*;

    #[test]
    fn round_trip() {
        let mut settings = Settings::default();
        settings.speed_index = 3;
        settings.markdown = MarkdownMode::RemoveMarks;
        settings.method = TypingMethod::Unicode;
        settings.last_run_version = Some("0.7".into());
        settings.last_update_check = Some(1_790_000_000);
        settings.set_shortcut(Shortcut { modifiers: Modifiers::CONTROL | Modifiers::ALT, key: 0x54 });
        assert_eq!(Settings::parse(Some(&settings.serialize())), settings);
    }

    #[test]
    fn damaged_file_gives_safe_values() {
        let damaged = [
            None,
            Some(""),
            Some("not json"),
            Some("[1,2]"),
            Some(r#"{"SpeedIndex": 999, "Method": "Nope"}"#),
            Some(r#"{"SpeedIndex": -4, "ShortcutKey": 27, "ShortcutModifiers": 2}"#),
            Some(r#"{"Markdown": 77, "ShortcutKey": 65, "ShortcutModifiers": 0}"#),
            Some(r#"{"ShortcutKey": 4294967296, "ShortcutModifiers": -1, "SpeedIndex": 1.5, "LastRunVersion": 7}"#),
            Some(r#"{"ShortcutKey": 65, "ShortcutModifiers": 4098}"#),
        ];
        for json in damaged {
            let settings = Settings::parse(json);
            assert!(settings.speed_index < SPEEDS.len(), "{json:?}");
            assert_eq!(settings.shortcut().problem(), None, "{json:?}");
            assert!(settings.shortcut().modifiers.0 <= Modifiers::ALL.0);
        }
        assert_eq!(Settings::parse(Some(r#"{"SpeedIndex": 999, "Method": "Nope"}"#)).speed_index, SPEEDS.len() - 1);
    }

    #[test]
    fn one_bad_value_keeps_the_rest() {
        let settings = Settings::parse(Some(r#"{"Method": "Nope", "ShiftReturn": false, "Markdown": "Keep", "SpeedIndex": 2}"#));
        assert_eq!(settings.method, TypingMethod::ScanCodes);
        assert!(!settings.shift_return);
        assert_eq!(settings.markdown, MarkdownMode::Keep);
        assert_eq!(settings.speed_index, 2);
        assert_eq!(Settings::parse(Some(r#"{"Method": 1}"#)).method, TypingMethod::VirtualKeys);
    }
}

mod updates {
    use super::*;
    use serde_json::json;

    fn item(tag: &str, body: &str, draft: bool, page: Option<&str>, exe: bool) -> Value {
        let base = format!("https://github.com/sayre4ux/TypeThru/releases/download/{tag}");
        let assets = if exe {
            json!([
                { "name": format!("TypeThru-{tag}-windows.exe"), "browser_download_url": format!("{base}/TypeThru-{tag}-windows.exe") },
                { "name": format!("TypeThru-{tag}-windows.exe.sig"), "browser_download_url": format!("{base}/TypeThru-{tag}-windows.exe.sig") },
                { "name": format!("TypeThru-{tag}.dmg"), "browser_download_url": format!("{base}/TypeThru-{tag}.dmg") },
            ])
        } else {
            json!([])
        };
        let page = page.map_or_else(|| format!("https://github.com/sayre4ux/TypeThru/releases/tag/{tag}"), str::to_owned);
        json!({ "tag_name": tag, "draft": draft, "body": body, "html_url": page, "assets": assets })
    }

    fn release(tag: &str) -> Value {
        item(tag, "## New\n- Something", false, None, true)
    }

    fn feed(items: &[Value]) -> String {
        Value::Array(items.to_vec()).to_string()
    }

    #[test]
    fn versions() {
        assert_eq!(numbers("v0.3.0-beta"), [0, 3, 0]);
        assert!(is_newer("0.10", "0.9"));
        assert!(!is_newer("0.7.0", "0.7"));
        assert!(!is_newer("garbage", "0.1"));
        assert_eq!(update_feed::display("0.7.0"), "0.7");
        assert_eq!(update_feed::display("0.6.1"), "0.6.1");
        assert_eq!(update_feed::display("1.0"), "1.0");
    }

    #[test]
    fn newest_with_windows_download() {
        let found = newest(&feed(&[release("v0.7.0-beta"), release("v0.6.1-beta")]), "0.6.1").unwrap();
        assert_eq!(found.version, "0.7.0");
        assert!(found.download.unwrap().path().ends_with("-windows.exe"));
        assert!(found.signature.unwrap().path().ends_with("-windows.exe.sig"));
        assert_eq!(found.notes.len(), 1);
    }

    #[test]
    fn mac_only_release_has_no_install() {
        let found = newest(&feed(&[item("v0.8", "## New\n- x", false, None, false)]), "0.7").unwrap();
        assert_eq!(found.download, None);
        assert!(is_release_page(&found.page));
    }

    #[test]
    fn only_this_project_and_no_drafts() {
        let elsewhere = item("v9.0", "", false, Some("https://example.com/sayre4ux/TypeThru/releases/tag/v9"), true);
        assert_eq!(newest(&feed(&[elsewhere, item("v8.0", "", true, None, true)]), "0.7"), None);
        assert_eq!(newest(&feed(&[release("v0.7")]), "0.7"), None);
        assert_eq!(newest("not json", "0.1"), None);
        assert_eq!(newest(r#"{"a":1}"#, "0.1"), None);
    }

    #[test]
    fn release_urls_are_checked_after_parsing() {
        let ok = |s: &str| is_release_page(&Url::parse(s).unwrap());
        assert!(ok("https://github.com/sayre4ux/TypeThru/releases/tag/v1"));
        assert!(ok("https://GitHub.com/sayre4ux/TypeThru/releases/tag/v1"));
        assert!(!ok("https://github.com/sayre4ux/TypeThru/releases/../../evil/releases/tag/v1"));
        assert!(!ok("https://github.com/sayre4ux/TypeThru/releases/%2e%2e/%2e%2e/x"));
        assert!(!ok("https://github.com@evil.example/sayre4ux/TypeThru/releases/tag/v1"));
        assert!(!ok("https://user@github.com/sayre4ux/TypeThru/releases/tag/v1"));
        assert!(!ok("https://github.com:8443/sayre4ux/TypeThru/releases/tag/v1"));
        assert!(!ok("http://github.com/sayre4ux/TypeThru/releases/tag/v1"));
        assert!(!ok("https://github.com.evil.example/sayre4ux/TypeThru/releases/tag/v1"));
        assert!(!update_feed::is_release_file(&Url::parse("https://github.com/sayre4ux/TypeThru/releases/tag/v1").unwrap()));
    }

    #[test]
    fn notes() {
        let body = "Intro line.\n\n## New\n- **Copy Text** from screen\n- Bold\n\n## Fixes\n- No surprise commands\n\n## Install\n1. Download\n- not a change\n\nSHA-256: `abc`";
        assert_eq!(summary(body), ["- **Copy Text** from screen", "- Bold", "- No surprise commands"]);
        assert_eq!(summary(&"- x\n".repeat(50)).len(), 20);
        assert_eq!(summary(&format!("- {}", "y".repeat(900)))[0].chars().count(), 400);
        let items = [
            item("v0.7.0-beta", "## New\n- Seven", false, None, true),
            item("v0.6.1-beta", "## Fixes\n- Settings", false, None, true),
            release("v0.6.0-beta"),
            item("v0.6.2", "## Install\n- only steps", false, None, true),
        ];
        let titles: Vec<String> = update_feed::notes(&feed(&items), "0.6").into_iter().map(|n| n.title).collect();
        assert_eq!(titles, ["0.7", "0.6.1"]);
    }

    #[test]
    fn signature_matches_the_mac_signing_script() {
        use base64::Engine;
        use ed25519_dalek::Signer;
        // The signature format of scripts/update-key.swift: base64 text of a raw Ed25519 signature.
        let signing = ed25519_dalek::SigningKey::from_bytes(&[7; 32]);
        let key = signing.verifying_key().to_bytes();
        let data = b"TypeThru update";
        let signature = format!("{}\n", base64::engine::general_purpose::STANDARD.encode(signing.sign(data).to_bytes()));
        assert!(is_signed(data, signature.as_bytes(), &key));
        assert!(!is_signed(b"TypeThru update!", signature.as_bytes(), &key));
        assert!(!is_signed(data, signature.as_bytes(), &public_key()));
        assert!(!is_signed(data, b"not base64", &key));
        assert!(!is_signed(data, &[0xFF, 0xFE], &key));
        assert!(!is_signed(data, b"", &key));
        assert_ne!(public_key(), [0; 32]);
    }

    #[test]
    fn same_public_key_as_the_mac_app() {
        let mac = repo_file("Sources/UpdateKey.swift");
        assert!(mac.contains(update_feed::PUBLIC_KEY), "Sources/UpdateKey.swift has a different key");
    }
}

mod notes {
    use super::*;

    #[test]
    fn bundled_changelog() {
        let entries = parse_changelog(&repo_file("CHANGELOG.md"));
        assert!(entries.len() >= 7);
        assert!(entries.iter().all(|e| e.lines.iter().all(|l| l.starts_with("- "))));
        let versions: Vec<&str> = since("0.4", "0.6.1", &entries).iter().map(|e| e.version.as_str()).collect();
        assert_eq!(versions, ["0.6.1", "0.6", "0.5.1", "0.5"]);
        assert!(since("0.6", "0.6", &entries).is_empty());
    }

    #[test]
    fn continuation_lines() {
        let entries = parse_changelog("## 1.2 · today\n- **a** b\n  c\n\n## no version\n- x");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].lines, ["- **a** b c"]);
        assert_eq!(parse_changelog("## 1.0\r\n- crlf\r\n")[0].lines, ["- crlf"]);
    }

    #[test]
    fn inline_styles_without_links() {
        let parts = parse_inline("**Bold** and *it* with `**code**` and [a link](https://example.com).");
        assert_eq!(parts.iter().map(|p| p.text.as_str()).collect::<String>(), "Bold and it with **code** and a link.");
        assert!(parts.iter().any(|p| p.text == "Bold" && p.bold));
        assert!(parts.iter().any(|p| p.text == "it" && p.italic));
        assert!(parts.iter().any(|p| p.text == "**code**" && p.code && !p.bold));
        assert_eq!(parse_inline("[no link here").iter().map(|p| p.text.as_str()).collect::<String>(), "[no link here");
    }

    fn piece(text: &str, x: f64, y: f64, width: f64) -> Piece {
        Piece { text: text.into(), x, y, width, height: 40.0 }
    }

    #[test]
    fn reading_order() {
        // Pixel boxes, origin at the top left: an email with paragraph gaps.
        let email = [
            piece("Best regards,", 50.0, 800.0, 300.0),
            piece("Dear Chek,", 50.0, 100.0, 300.0),
            piece("Kyle", 50.0, 850.0, 300.0),
            piece("Noted the updated server reboot arrangements.", 50.0, 220.0, 800.0),
        ];
        assert_eq!(layout(&email), "Dear Chek,\n\nNoted the updated server reboot arrangements.\n\nBest regards,\nKyle");
        let table = [piece("srv-db-01", 50.0, 500.0, 150.0), piece("10.0.0.21", 550.0, 495.0, 150.0), piece("Reboot", 300.0, 503.0, 100.0)];
        assert_eq!(layout(&table), "srv-db-01\tReboot\t10.0.0.21");
        assert_eq!(layout(&[piece("Hello", 100.0, 500.0, 100.0), piece("world", 210.0, 500.0, 100.0)]), "Hello world");
        assert_eq!(layout(&[]), "");
    }
}

/// Random input never crashes the rules, with a fixed seed so failures reproduce.
mod fuzz {
    use super::*;

    const PIECES: [&str; 35] = [
        "a",
        "Z",
        "9",
        " ",
        "\n",
        "\r\n",
        "\t",
        "'",
        "\"",
        "\\",
        "$",
        "*",
        "**",
        "***",
        "_",
        "•",
        "—",
        "“",
        "…",
        "\u{2028}",
        "\u{200B}",
        "\u{A0}",
        "é",
        "e\u{301}",
        "中",
        "🙂",
        "👨‍👩‍👧‍👦",
        "\0",
        "\u{7F}",
        "[",
        "](",
        ")",
        "`",
        "#",
        "- ",
    ];

    /// SplitMix64: small, fixed, and the same on every platform.
    struct Random(u64);

    impl Random {
        fn below(&mut self, n: usize) -> usize {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            ((z ^ (z >> 31)) % n as u64) as usize
        }
    }

    #[test]
    fn text_rules_never_crash() {
        let mut random = Random(0x5459_5045);
        for _ in 0..20_000 {
            let length = random.below(24);
            let text: String = (0..length).map(|_| PIECES[random.below(PIECES.len())]).collect();
            let result = markdown(&text);
            let count = elements(&result.text).len();
            assert!(result.formats.keys().all(|&k| k <= count), "{text:?}");
            // Every switch is undone: formatting never stays on after the text.
            for format in [TextFormat::Bold, TextFormat::Italic] {
                let switches = result.formats.values().flatten().filter(|&&f| f == format).count();
                assert_eq!(switches % 2, 0, "{text:?}");
            }
            let trimmed = without_trailing_line_breaks(&text);
            assert!(elements(&trimmed).last().is_none_or(|e| !is_line_break(e) && *e != "\u{2028}"), "{text:?}");
            replace_untypable(&text, str::is_ascii);
            parse_inline(&text);
            parse_changelog(&text);
            summary(&text);
            newest(&text, "0.1");
            Settings::parse(Some(&text));
        }
    }
}

/// A real release, when its files are in the repository folder (they are git-ignored): CryptoKit's
/// signature from scripts/update-key.swift must pass the strict check.
#[test]
fn real_release_signature() {
    let folder = format!("{}/../..", env!("CARGO_MANIFEST_DIR"));
    let (Ok(data), Ok(signature)) =
        (std::fs::read(format!("{folder}/TypeThru-0.6.1.dmg")), std::fs::read(format!("{folder}/TypeThru-0.6.1.dmg.sig")))
    else {
        return;
    };
    assert!(is_signed(&data, &signature, &public_key()));
    assert!(!is_signed(&data[1..], &signature, &public_key()));
}
