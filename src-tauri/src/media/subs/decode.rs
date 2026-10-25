//! A subtitle file's bytes to text. Old subtitle files are rarely UTF-8:
//! Persian and Arabic ones are usually Windows-1256, European ones
//! Windows-1252 or ISO-8859-x, and some editors save UTF-16. In order:
//! a byte order mark; UTF-16 without one (NUL bytes in every other place);
//! valid UTF-8; then Mozilla's `chardetng` (the detector Firefox uses for
//! pages that do not say their encoding), which tells Windows-1256 from
//! ISO-8859-6, Windows-1252 from ISO-8859-2, and so on. A language hint
//! (from the file name) is passed to it the way Firefox passes a domain.

use encoding_rs::{Encoding, UTF_16BE, UTF_16LE, UTF_8};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum How {
    /// A byte order mark said so.
    Bom,
    /// Valid UTF-8 (or plain ASCII).
    Utf8,
    /// UTF-16 found by its NUL bytes.
    Utf16,
    /// Guessed by the detector.
    Detected,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Decoded {
    #[serde(skip)]
    pub text: String,
    /// The WHATWG name: "UTF-8", "windows-1256", "UTF-16LE", ...
    pub encoding: &'static str,
    pub how: How,
    /// Bytes that did not decode and became U+FFFD.
    pub replaced: bool,
}

/// The country code `chardetng` takes as its hint, for a language that
/// tells it something (scripts with several legacy encodings).
fn tld_for(lang: &str) -> Option<&'static [u8]> {
    Some(match lang {
        "fa" => b"ir",
        "ar" => b"sa",
        "ur" => b"pk",
        "he" => b"il",
        "ru" | "uk" | "bg" | "sr" => b"ru",
        "el" => b"gr",
        "tr" => b"tr",
        "pl" | "cs" | "sk" | "hu" | "ro" | "hr" | "sl" => b"pl",
        "th" => b"th",
        "vi" => b"vn",
        "ja" => b"jp",
        "zh" => b"cn",
        "ko" => b"kr",
        "lt" | "lv" | "et" => b"lt",
        _ => return None,
    })
}

fn utf16_without_bom(bytes: &[u8]) -> Option<&'static Encoding> {
    if bytes.len() < 8 {
        return None;
    }
    let sample = &bytes[..bytes.len().min(4096) & !1];
    let (mut even, mut odd) = (0usize, 0usize);
    for pair in sample.chunks_exact(2) {
        if pair[0] == 0 {
            even += 1;
        }
        if pair[1] == 0 {
            odd += 1;
        }
    }
    let half = sample.len() / 2;
    // Text in a Latin script has a NUL in nearly every other byte; other
    // scripts fewer, but still far more than any 8-bit file has.
    if odd * 3 > half && even * 10 < half {
        Some(UTF_16LE)
    } else if even * 3 > half && odd * 10 < half {
        Some(UTF_16BE)
    } else {
        None
    }
}

pub fn decode(bytes: &[u8], lang_hint: Option<&str>) -> Decoded {
    if let Some((enc, bom_len)) = Encoding::for_bom(bytes) {
        let (text, replaced) = enc.decode_without_bom_handling(&bytes[bom_len..]);
        return Decoded {
            text: text.into_owned(),
            encoding: enc.name(),
            how: How::Bom,
            replaced,
        };
    }
    if let Some(enc) = utf16_without_bom(bytes) {
        let (text, replaced) = enc.decode_without_bom_handling(bytes);
        return Decoded {
            text: text.into_owned(),
            encoding: enc.name(),
            how: How::Utf16,
            replaced,
        };
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Decoded {
            text: text.to_owned(),
            encoding: UTF_8.name(),
            how: How::Utf8,
            replaced: false,
        };
    }
    let mut det = chardetng::EncodingDetector::new();
    det.feed(bytes, true);
    let enc = det.guess(lang_hint.and_then(tld_for), false);
    let (text, replaced) = enc.decode_without_bom_handling(bytes);
    Decoded {
        text: text.into_owned(),
        encoding: enc.name(),
        how: How::Detected,
        replaced,
    }
}

/// Persian text from an old Windows-1256 file carries the Arabic yeh and
/// kaf (the code page has no Persian yeh, and old keyboards typed the
/// Arabic kaf): written as Persian, so a word looks up and matches the
/// notebook. Only for a track known to be Persian.
pub fn persian_letters(text: &str) -> (String, bool) {
    if !text.contains(['\u{064a}', '\u{0643}', '\u{0649}']) {
        return (text.to_owned(), false);
    }
    let out: String = text
        .chars()
        .map(|c| match c {
            '\u{064a}' | '\u{0649}' => '\u{06cc}', // ي ى → ی
            '\u{0643}' => '\u{06a9}',              // ك → ک
            c => c,
        })
        .collect();
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FA: &str = "صبح بخیر، امروز حالت چطور است؟ کلیدهایم را روی میز آشپزخانه گذاشتم. قطار شهر ساعت نه حرکت می‌کند.";
    const AR: &str = "صباح الخير، كيف حالك اليوم؟ تركت مفاتيحي على طاولة المطبخ. القطار إلى المدينة يغادر في التاسعة.";

    fn enc(name: &str, text: &str) -> Vec<u8> {
        let e = Encoding::for_label(name.as_bytes()).unwrap();
        let (bytes, _, bad) = e.encode(text);
        assert!(!bad, "{name} cannot hold the sample");
        bytes.into_owned()
    }

    #[test]
    fn utf8_with_and_without_bom() {
        let mut b = vec![0xef, 0xbb, 0xbf];
        b.extend_from_slice(FA.as_bytes());
        let d = decode(&b, None);
        assert_eq!(
            (d.encoding, d.how, d.text.as_str()),
            ("UTF-8", How::Bom, FA)
        );
        let d = decode(FA.as_bytes(), None);
        assert_eq!((d.encoding, d.how), ("UTF-8", How::Utf8));
    }

    #[test]
    fn utf16_with_and_without_bom() {
        let mut le: Vec<u8> = vec![0xff, 0xfe];
        for u in "Hello there, friend".encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode(&le, None).encoding, "UTF-16LE");
        let d = decode(&le[2..], None);
        assert_eq!((d.encoding, d.how), ("UTF-16LE", How::Utf16));
        assert_eq!(d.text, "Hello there, friend");
        let mut be = Vec::new();
        for u in "1\n00:00:01,000 --> 00:00:02,000\nHi".encode_utf16() {
            be.extend_from_slice(&u.to_be_bytes());
        }
        assert_eq!(decode(&be, None).encoding, "UTF-16BE");
    }

    #[test]
    fn old_persian_and_arabic_files_are_windows_1256() {
        let (fa_old, _) = (FA.replace('\u{06cc}', "\u{064a}"), ());
        let d = decode(&enc("windows-1256", &fa_old), None);
        assert_eq!(d.encoding, "windows-1256");
        assert_eq!(d.text, fa_old);
        let d = decode(&enc("windows-1256", AR), Some("ar"));
        assert_eq!(d.encoding, "windows-1256");
        assert_eq!(d.text, AR);
    }

    #[test]
    fn european_legacy_encodings() {
        let fr =
            "Le garçon a mangé une pêche à la fenêtre. Où est l'hôtel? Très bien, merci beaucoup.";
        let d = decode(&enc("windows-1252", fr), None);
        assert_eq!(d.encoding, "windows-1252");
        assert_eq!(d.text, fr);
        let ru = "Доброе утро, как дела сегодня? Я оставил ключи на кухонном столе у окна.";
        let d = decode(&enc("windows-1251", ru), Some("ru"));
        assert_eq!(d.encoding, "windows-1251");
        assert_eq!(d.text, ru);
        let el = "Καλημέρα, τι κάνεις σήμερα; Άφησα τα κλειδιά μου στο τραπέζι της κουζίνας.";
        let d = decode(&enc("iso-8859-7", el), Some("el"));
        assert!(d.encoding == "ISO-8859-7" || d.encoding == "windows-1253");
        assert_eq!(d.text, el);
    }

    #[test]
    fn persian_letters_fix_arabic_yeh_and_kaf() {
        let (t, changed) = persian_letters("كتاب خيلي خوبي");
        assert!(changed);
        assert_eq!(t, "کتاب خیلی خوبی");
        assert!(!persian_letters("کتاب").1);
    }
}
