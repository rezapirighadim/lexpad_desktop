//! Which language a subtitle track is in, found on the device (no AI, no
//! network): from its text (`whatlang`'s trigram models, then a letter
//! check for the languages written in Arabic script, which share most
//! letters: Persian has پ چ ژ گ and ی, Urdu has ٹ ڈ ڑ ں ے, Arabic has ة and
//! the article ال in nearly every line), from its file name (`movie.fa.srt`,
//! `.per.`, `.farsi.`, `English.srt`), and from the container's tag (`per`,
//! `fas`, `eng`). The player pre-fills its question with the best of these
//! and says how sure it is.

use serde::Serialize;

/// ISO 639-1 code, English name, and every other name a file or a tag uses
/// for it (ISO 639-2 B and T codes, English and native names), lowercase.
const LANGUAGES: &[(&str, &str, &[&str])] = &[
    ("en", "English", &["eng", "english"]),
    (
        "fa",
        "Persian",
        &[
            "fas",
            "per",
            "pes",
            "farsi",
            "persian",
            "فارسی",
            "prs",
            "dari",
        ],
    ),
    ("ar", "Arabic", &["ara", "arb", "arabic", "عربي", "العربية"]),
    ("ur", "Urdu", &["urd", "urdu", "اردو"]),
    ("ps", "Pashto", &["pus", "pbt", "pashto"]),
    ("ku", "Kurdish", &["kur", "ckb", "kurdish", "sorani"]),
    ("tr", "Turkish", &["tur", "turkish", "türkçe", "turkce"]),
    ("de", "German", &["ger", "deu", "german", "deutsch"]),
    (
        "fr",
        "French",
        &["fre", "fra", "french", "français", "francais"],
    ),
    (
        "es",
        "Spanish",
        &[
            "spa",
            "spanish",
            "español",
            "espanol",
            "castellano",
            "latino",
        ],
    ),
    ("it", "Italian", &["ita", "italian", "italiano"]),
    (
        "pt",
        "Portuguese",
        &[
            "por",
            "portuguese",
            "português",
            "portugues",
            "brazilian",
            "ptbr",
            "pt-br",
        ],
    ),
    ("nl", "Dutch", &["dut", "nld", "dutch", "nederlands"]),
    ("ru", "Russian", &["rus", "russian", "русский"]),
    ("uk", "Ukrainian", &["ukr", "ukrainian"]),
    ("pl", "Polish", &["pol", "polish", "polski"]),
    ("cs", "Czech", &["cze", "ces", "czech"]),
    ("sk", "Slovak", &["slo", "slk", "slovak"]),
    ("hu", "Hungarian", &["hun", "hungarian", "magyar"]),
    ("ro", "Romanian", &["rum", "ron", "romanian"]),
    ("bg", "Bulgarian", &["bul", "bulgarian"]),
    ("el", "Greek", &["gre", "ell", "greek"]),
    ("he", "Hebrew", &["heb", "hebrew", "iw"]),
    ("hi", "Hindi", &["hin", "hindi"]),
    ("bn", "Bengali", &["ben", "bengali", "bangla"]),
    (
        "zh",
        "Chinese",
        &["chi", "zho", "cmn", "chinese", "chs", "cht", "mandarin"],
    ),
    ("ja", "Japanese", &["jpn", "japanese"]),
    ("ko", "Korean", &["kor", "korean"]),
    ("vi", "Vietnamese", &["vie", "vietnamese"]),
    ("th", "Thai", &["tha", "thai"]),
    ("id", "Indonesian", &["ind", "indonesian", "bahasa"]),
    ("ms", "Malay", &["may", "msa", "malay"]),
    ("sv", "Swedish", &["swe", "swedish", "svenska"]),
    (
        "no",
        "Norwegian",
        &["nor", "nob", "nno", "norwegian", "norsk", "nb"],
    ),
    ("da", "Danish", &["dan", "danish", "dansk"]),
    ("fi", "Finnish", &["fin", "finnish", "suomi"]),
    ("hr", "Croatian", &["hrv", "croatian"]),
    ("sr", "Serbian", &["srp", "serbian"]),
    ("sl", "Slovenian", &["slv", "slovenian"]),
    ("et", "Estonian", &["est", "estonian"]),
    ("lv", "Latvian", &["lav", "latvian"]),
    ("lt", "Lithuanian", &["lit", "lithuanian"]),
    ("az", "Azerbaijani", &["aze", "azerbaijani"]),
    ("hy", "Armenian", &["arm", "hye", "armenian"]),
    ("ka", "Georgian", &["geo", "kat", "georgian"]),
    ("ca", "Catalan", &["cat", "catalan"]),
    ("tl", "Tagalog", &["tgl", "fil", "tagalog", "filipino"]),
    ("ta", "Tamil", &["tam", "tamil"]),
    ("sw", "Swahili", &["swa", "swahili"]),
];

/// Short codes that are also common words or release tags in file names, so
/// they count only as the last part before the extension (`movie.it.srt`),
/// never in the middle of a name ("It.Follows.srt", "No.Time.To.Die").
const AMBIGUOUS: &[&str] = &[
    "it", "no", "is", "as", "in", "be", "me", "to", "so", "id", "hi", "ms", "ca", "am", "an", "or",
    "de", "da", "el", "et", "my", "on",
];

/// The language's English name, for a code from `LANGUAGES`.
pub fn name_of(code: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, n, _)| *n)
}

/// A code or a name (any case) to the ISO 639-1 code.
pub fn normalize(tag: &str) -> Option<&'static str> {
    let t = tag.trim().to_lowercase();
    let t = t.split(['-', '_']).next().unwrap_or(&t).to_owned();
    if t.is_empty() || t == "und" || t == "unk" || t == "mis" || t == "zxx" {
        return None;
    }
    LANGUAGES
        .iter()
        .find(|(c, _, names)| *c == t || names.contains(&t.as_str()))
        .map(|(c, _, _)| *c)
}

/// What a file name says about its subtitle: a language, and whether it is
/// "forced" (signs only) or for the deaf and hard of hearing.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NameHint {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<&'static str>,
    pub forced: bool,
    pub sdh: bool,
}

/// `movie.en.srt`, `movie.fa.forced.srt`, `Movie (2001) - Farsi.srt`,
/// `movie_per.srt`, `show 1x02 English.srt`. The last language-looking part
/// wins, since names begin with the title.
pub fn from_file_name(name: &str) -> NameHint {
    let stem = match name.rsplit_once('.') {
        Some((s, _)) => s,
        None => name,
    };
    let parts: Vec<String> = stem
        .split(|c: char| {
            c == '.'
                || c == '_'
                || c == ' '
                || c == '-'
                || c == '['
                || c == ']'
                || c == '('
                || c == ')'
        })
        .filter(|p| !p.is_empty())
        .map(|p| p.to_lowercase())
        .collect();
    let mut hint = NameHint::default();
    let last = parts.len().saturating_sub(1);
    for (i, p) in parts.iter().enumerate().rev() {
        match p.as_str() {
            "forced" => hint.forced = true,
            "sdh" | "hi" | "cc" | "hoh" if i > 0 => hint.sdh = true,
            _ => {}
        }
        if hint.lang.is_some() || i == 0 && parts.len() > 1 {
            continue;
        }
        // Only the last few parts, after the title.
        if last - i > 3 {
            continue;
        }
        let tail_ok = parts[i + 1..].iter().all(|q| {
            matches!(
                q.as_str(),
                "forced" | "sdh" | "hi" | "cc" | "hoh" | "default"
            )
        });
        if AMBIGUOUS.contains(&p.as_str()) && !tail_ok {
            continue;
        }
        if p.len() < 2 {
            continue;
        }
        if let Some(code) = normalize(p) {
            hint.lang = Some(code);
        }
    }
    hint
}

/// A guess with how sure it is (0..1) and where it came from.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Guess {
    pub code: &'static str,
    pub name: &'static str,
    pub confidence: f64,
    pub source: Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Text,
    FileName,
    Track,
}

/// whatlang's ISO 639-3 codes to ISO 639-1, for the languages it knows.
fn from_whatlang(lang: whatlang::Lang) -> Option<&'static str> {
    use whatlang::Lang::*;
    Some(match lang {
        Eng => "en",
        Pes => "fa",
        Ara => "ar",
        Urd => "ur",
        Tur => "tr",
        Deu => "de",
        Fra => "fr",
        Spa => "es",
        Ita => "it",
        Por => "pt",
        Nld => "nl",
        Rus => "ru",
        Ukr => "uk",
        Pol => "pl",
        Ces => "cs",
        Slk => "sk",
        Hun => "hu",
        Ron => "ro",
        Bul => "bg",
        Ell => "el",
        Heb => "he",
        Hin => "hi",
        Ben => "bn",
        Cmn => "zh",
        Jpn => "ja",
        Kor => "ko",
        Vie => "vi",
        Tha => "th",
        Ind => "id",
        Swe => "sv",
        Nob => "no",
        Dan => "da",
        Fin => "fi",
        Hrv => "hr",
        Srp => "sr",
        Slv => "sl",
        Est => "et",
        Lav => "lv",
        Lit => "lt",
        Aze => "az",
        Hye => "hy",
        Kat => "ka",
        Cat => "ca",
        Tgl => "tl",
        Tam => "ta",
        _ => return None,
    })
}

/// Persian, Arabic or Urdu, from the letters only each uses. `None` when
/// the text is too short or not in Arabic script.
fn arabic_script_language(text: &str) -> Option<(&'static str, f64)> {
    let (mut letters, mut fa, mut ur, mut ar) = (0usize, 0usize, 0usize, 0usize);
    for c in text.chars() {
        if !('\u{0600}'..='\u{06ff}').contains(&c) {
            continue;
        }
        letters += 1;
        match c {
            'پ' | 'چ' | 'ژ' | 'گ' | 'ی' | 'ک' => fa += 1,
            'ٹ' | 'ڈ' | 'ڑ' | 'ں' | 'ے' | 'ھ' | 'ۓ' => ur += 1,
            'ة' | 'ى' | 'إ' | 'أ' | 'ؤ' | 'ئ' => ar += 1,
            _ => {}
        }
    }
    if letters < 20 {
        return None;
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let count = |list: &[&str]| words.iter().filter(|w| list.contains(w)).count();
    // Words that are everywhere in each language's speech.
    fa += 3 * count(&[
        "است", "این", "را", "که", "می", "من", "تو", "نه", "چی", "چه", "هست", "بود", "باید", "خیلی",
        "اون", "هم",
    ]);
    ar += 3 * count(&[
        "في", "من", "على", "إلى", "هذا", "هذه", "ما", "لا", "أنا", "أنت", "كان", "هل", "لم", "التي",
        "الذي",
    ]);
    ur += 3 * count(&[
        "ہے", "میں", "کے", "کی", "کا", "نہیں", "اور", "یہ", "وہ", "ہیں", "تھا", "کیا",
    ]);
    ar += words
        .iter()
        .filter(|w| w.starts_with("ال") && w.chars().count() > 3)
        .count();
    // "من" is both Persian ("I") and Arabic ("from"); the rest decide.
    let total = (fa + ur + ar).max(1) as f64;
    let (code, best) = [("fa", fa), ("ur", ur), ("ar", ar)]
        .into_iter()
        .max_by_key(|(_, n)| *n)?;
    if best == 0 {
        return None;
    }
    let share = best as f64 / total;
    Some((code, share.min(0.99)))
}

/// The language of a track's text: up to the first 30,000 characters.
pub fn from_text(text: &str) -> Option<Guess> {
    let sample: String = text.chars().take(30_000).collect();
    if sample.trim().chars().count() < 20 {
        return None;
    }
    let info = whatlang::detect(&sample);
    let arabic_script = arabic_script_language(&sample);
    if let Some((code, share)) = arabic_script {
        // whatlang knows these three too; agreeing raises the confidence.
        let agrees = info.and_then(|i| from_whatlang(i.lang())) == Some(code);
        let confidence = if agrees {
            (0.6 + share * 0.4).min(0.99)
        } else {
            share * 0.85
        };
        return Some(Guess {
            code,
            name: name_of(code).unwrap_or(code),
            confidence: (confidence * 100.0).round() / 100.0,
            source: Source::Text,
        });
    }
    let info = info?;
    let code = from_whatlang(info.lang())?;
    let mut confidence = info.confidence();
    if !info.is_reliable() {
        confidence = confidence.min(0.5);
    }
    Some(Guess {
        code,
        name: name_of(code).unwrap_or(code),
        confidence: (confidence * 100.0).round() / 100.0,
        source: Source::Text,
    })
}

/// Everything known about a track's language, and the one to pre-fill.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best: Option<Guess>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<Guess>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<&'static str>,
}

/// The text decides when it is sure; a file name or tag that agrees makes
/// it surer; one that disagrees with an unsure text wins (a name is a
/// person's statement, the text a statistic).
pub fn detect(
    text: &str,
    file_name: Option<&'static str>,
    track: Option<&'static str>,
) -> Detection {
    let by_text = from_text(text);
    let declared = track.or(file_name);
    let best = match (&by_text, declared) {
        (Some(t), Some(d)) if t.code == d => Some(Guess {
            confidence: (t.confidence.max(0.6) + 0.3).min(0.99),
            ..t.clone()
        }),
        (Some(t), Some(d)) if t.confidence < 0.7 => Some(Guess {
            code: d,
            name: name_of(d).unwrap_or(d),
            confidence: 0.6,
            source: if track.is_some() {
                Source::Track
            } else {
                Source::FileName
            },
        }),
        (Some(t), _) => Some(t.clone()),
        (None, Some(d)) => Some(Guess {
            code: d,
            name: name_of(d).unwrap_or(d),
            confidence: 0.6,
            source: if track.is_some() {
                Source::Track
            } else {
                Source::FileName
            },
        }),
        (None, None) => None,
    };
    Detection {
        best,
        text: by_text,
        file_name,
        track,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FA: &str = "صبح بخیر، امروز حالت چطور است؟ کلیدهایم را روی میز آشپزخانه گذاشتم. قطار شهر ساعت نه حرکت می‌کند. می‌توانی کمی آهسته‌تر صحبت کنی؟ بعد از ناهار به بازار می‌رویم.";
    const FA_OLD: &str = "صبح بخير، امروز حالت چطور است؟ كليدهايم را روي ميز آشپزخانه گذاشتم. قطار شهر ساعت نه حركت مي‌كند. مي‌تواني كمي آهسته‌تر صحبت كني؟ بعد از ناهار به بازار مي‌رويم.";
    const AR: &str = "صباح الخير، كيف حالك اليوم؟ تركت مفاتيحي على طاولة المطبخ. القطار إلى المدينة يغادر في التاسعة. هل يمكنك أن تتكلم ببطء قليلاً؟ سنذهب إلى السوق بعد الغداء.";
    const UR: &str = "صبح بخیر، آج آپ کیسے ہیں؟ میں نے اپنی چابیاں باورچی خانے کی میز پر چھوڑ دیں۔ شہر جانے والی ٹرین نو بجے روانہ ہوتی ہے۔ کیا آپ تھوڑا آہستہ بول سکتے ہیں؟";

    #[test]
    fn tells_persian_arabic_and_urdu_apart() {
        assert_eq!(from_text(FA).unwrap().code, "fa");
        assert_eq!(from_text(FA_OLD).unwrap().code, "fa");
        assert_eq!(from_text(AR).unwrap().code, "ar");
        assert_eq!(from_text(UR).unwrap().code, "ur");
        assert!(from_text(FA).unwrap().confidence > 0.7);
    }

    #[test]
    fn other_scripts_and_languages() {
        let en = "Good morning, how are you today? I left my keys on the kitchen table. The train to the city leaves at nine.";
        assert_eq!(from_text(en).unwrap().code, "en");
        let de = "Guten Morgen, wie geht es dir heute? Ich habe meine Schlüssel auf dem Küchentisch liegen lassen.";
        assert_eq!(from_text(de).unwrap().code, "de");
        assert!(from_text("ok").is_none());
    }

    #[test]
    fn file_names() {
        assert_eq!(from_file_name("movie.en.srt").lang, Some("en"));
        assert_eq!(from_file_name("movie.fa.srt").lang, Some("fa"));
        assert_eq!(from_file_name("movie.per.srt").lang, Some("fa"));
        assert_eq!(
            from_file_name("Movie.2001.1080p.farsi.srt").lang,
            Some("fa")
        );
        assert_eq!(from_file_name("movie_Persian.srt").lang, Some("fa"));
        assert_eq!(
            from_file_name("show name 1x02 English.srt").lang,
            Some("en")
        );
        let f = from_file_name("movie.en.forced.srt");
        assert_eq!((f.lang, f.forced), (Some("en"), true));
        assert!(from_file_name("movie.eng.sdh.srt").sdh);
        assert_eq!(from_file_name("movie.srt").lang, None);
        // Words in titles are not languages.
        assert_eq!(from_file_name("It.Follows.2014.srt").lang, None);
        assert_eq!(from_file_name("No.Time.To.Die.srt").lang, None);
        assert_eq!(from_file_name("movie.it.srt").lang, Some("it"));
        assert_eq!(from_file_name("english.srt").lang, Some("en"));
    }

    #[test]
    fn track_tags() {
        assert_eq!(normalize("per"), Some("fa"));
        assert_eq!(normalize("fas"), Some("fa"));
        assert_eq!(normalize("ara"), Some("ar"));
        assert_eq!(normalize("pt-BR"), Some("pt"));
        assert_eq!(normalize("und"), None);
        assert_eq!(normalize("xyz"), None);
    }

    #[test]
    fn combining_text_and_names() {
        let d = detect(FA, Some("fa"), None);
        let best = d.best.unwrap();
        assert_eq!(best.code, "fa");
        assert!(best.confidence > 0.9);
        // A name against sure text: the text wins.
        let d = detect(AR, Some("fa"), None);
        assert_eq!(d.best.unwrap().code, "ar");
        // Nothing to read: the name.
        let d = detect("", Some("en"), None);
        assert_eq!(d.best.unwrap().source, Source::FileName);
    }
}
