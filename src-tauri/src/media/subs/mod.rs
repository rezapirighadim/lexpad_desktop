//! Subtitles: one model for every text format the player reads (SubRip,
//! WebVTT, ASS/SSA, and mov_text through FFmpeg), the encodings old files come
//! in, and which language a track is in. Everything here runs on the device:
//! no subtitle text leaves it.
//!
//! - `decode`: bytes to text (BOM, UTF-8, UTF-16, then Mozilla's detector for
//!   Windows-1256, Windows-1252, ISO-8859-x and the rest).
//! - `srt`, `vtt`, `ass`: the parsers, all into `Cue`s.
//! - `lang`: the language of a track from its text, its file name and its
//!   container's tag, with a confidence.
//!
//! Image subtitles (PGS, VobSub, DVB) are not text and are reported as
//! unsupported (`probe`), never parsed.

pub mod ass;
pub mod decode;
pub mod lang;
pub mod srt;
pub mod vtt;

use serde::Serialize;

/// A piece of a line with one style. Line breaks are `\n` inside `text`.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub text: String,
    #[serde(skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub strike: bool,
    /// `#rrggbb`, when the file gave one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One subtitle on screen, in milliseconds of the video's timeline.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cue {
    pub start: i64,
    pub end: i64,
    /// The plain text, lines joined with `\n`: what selection and the
    /// transcript use.
    pub text: String,
    pub spans: Vec<Span>,
    /// Where on screen, as on a phone keypad (1 bottom left, 2 bottom
    /// centre, 8 top centre, ...); `None` is the track's default (2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<u8>,
    /// An exact place, as fractions of the frame (ASS `\pos`), 0..1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<(f64, f64)>,
    /// The ASS style's name, when the track has styles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
}

/// An ASS style's basics, which the player may use or override.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Style {
    pub name: String,
    pub font: String,
    /// As a fraction of the frame's height (ASS sizes are in PlayResY units).
    pub size: f64,
    pub color: String,
    pub outline_color: String,
    pub bold: bool,
    pub italic: bool,
    pub align: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Srt,
    Vtt,
    Ass,
}

impl Format {
    /// By file extension (lowercase, no dot).
    pub fn from_ext(ext: &str) -> Option<Format> {
        match ext {
            "srt" => Some(Format::Srt),
            "vtt" | "webvtt" => Some(Format::Vtt),
            "ass" | "ssa" => Some(Format::Ass),
            _ => None,
        }
    }

    /// By content, for a file whose name says nothing useful.
    pub fn sniff(text: &str) -> Option<Format> {
        let head = text.trim_start_matches('\u{feff}').trim_start();
        if head.starts_with("WEBVTT") {
            Some(Format::Vtt)
        } else if head.starts_with("[Script Info]") || head.contains("\n[Events]") {
            Some(Format::Ass)
        } else if head.contains("-->") {
            Some(Format::Srt)
        } else {
            None
        }
    }
}

/// A parsed track.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Parsed {
    pub format: Format,
    pub cues: Vec<Cue>,
    pub styles: Vec<Style>,
}

/// Parses text in `format` (or the one it looks like).
pub fn parse(text: &str, format: Option<Format>) -> Option<Parsed> {
    let format = format.or_else(|| Format::sniff(text))?;
    let (mut cues, styles) = match format {
        Format::Srt => (srt::parse(text), Vec::new()),
        Format::Vtt => (vtt::parse(text), Vec::new()),
        Format::Ass => ass::parse(text),
    };
    cues.retain(|c| c.end > c.start && !c.text.trim().is_empty());
    cues.sort_by_key(|c| (c.start, c.end));
    Some(Parsed {
        format,
        cues,
        styles,
    })
}

/// `hh:mm:ss,mmm`, `hh:mm:ss.mmm`, `mm:ss.mmm` (WebVTT), or ASS's
/// `h:mm:ss.cc`, to milliseconds. A missing fraction is zero.
pub fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let (clock, frac) = match s.rfind([',', '.']) {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    };
    let parts: Vec<&str> = clock.split(':').collect();
    let num = |p: &str| -> Option<i64> {
        let p = p.trim();
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) || p.len() > 6 {
            return None;
        }
        p.parse().ok()
    };
    let (h, m, sec) = match parts.as_slice() {
        [h, m, s] => (num(h)?, num(m)?, num(s)?),
        [m, s] => (0, num(m)?, num(s)?),
        _ => return None,
    };
    if m >= 60 || sec >= 60 {
        return None;
    }
    let ms = if frac.is_empty() {
        0
    } else {
        if !frac.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        // Scale to milliseconds whatever the precision (cc, mmm, or more).
        let mut digits: String = frac.chars().take(3).collect();
        while digits.len() < 3 {
            digits.push('0');
        }
        digits.parse::<i64>().ok()?
    };
    Some(((h * 60 + m) * 60 + sec) * 1000 + ms)
}

/// Joins spans' text into the plain text of a cue.
pub fn plain(spans: &[Span]) -> String {
    let mut s = String::new();
    for span in spans {
        s.push_str(&span.text);
    }
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Adjacent spans with the same style become one.
pub fn merge(spans: Vec<Span>) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        if span.text.is_empty() {
            continue;
        }
        if let Some(last) = out.last_mut() {
            if last.bold == span.bold
                && last.italic == span.italic
                && last.underline == span.underline
                && last.strike == span.strike
                && last.color == span.color
            {
                last.text.push_str(&span.text);
                continue;
            }
        }
        out.push(span);
    }
    // Trim the outer whitespace of the cue, keeping inner line breaks.
    if let Some(first) = out.first_mut() {
        first.text = first.text.trim_start().to_owned();
    }
    if let Some(last) = out.last_mut() {
        last.text = last.text.trim_end().to_owned();
    }
    out.retain(|s| !s.text.is_empty());
    out
}

/// The HTML-like tags SubRip and WebVTT share (`<b>`, `<i>`, `<u>`, `<s>`,
/// `<font color>`; WebVTT's `<c>`, `<v>`, `<lang>`, `<ruby>` and timestamps
/// are dropped, their text kept), plus the `{\an8}` and `{\i1}` tags many
/// SubRip files carry. Returns the spans and the alignment, if any.
pub fn parse_markup(text: &str) -> (Vec<Span>, Option<u8>) {
    let mut spans = Vec::new();
    let mut cur = Span::default();
    let mut color_stack: Vec<Option<String>> = Vec::new();
    let mut align = None;
    let mut buf = String::new();
    let mut rest = text;
    let flush = |buf: &mut String, cur: &Span, spans: &mut Vec<Span>| {
        if !buf.is_empty() {
            let mut s = cur.clone();
            s.text = decode_entities(buf);
            spans.push(s);
            buf.clear();
        }
    };
    while let Some(c) = rest.chars().next() {
        if c == '<' {
            if let Some(end) = rest.find('>') {
                let tag = rest[1..end].trim().to_ascii_lowercase();
                rest = &rest[end + 1..];
                flush(&mut buf, &cur, &mut spans);
                let (closing, name) = match tag.strip_prefix('/') {
                    Some(n) => (true, n.trim()),
                    None => (false, tag.as_str()),
                };
                let head = name.split([' ', '.', '\t']).next().unwrap_or("");
                match head {
                    "b" => cur.bold = !closing,
                    "i" => cur.italic = !closing,
                    "u" => cur.underline = !closing,
                    "s" => cur.strike = !closing,
                    "br" => buf.push('\n'),
                    "font" if closing => cur.color = color_stack.pop().flatten(),
                    "font" => {
                        color_stack.push(cur.color.clone());
                        if let Some(c) = font_color(name) {
                            cur.color = Some(c);
                        }
                    }
                    _ => {}
                }
                continue;
            }
        }
        if c == '{' {
            if let Some(end) = rest.find('}') {
                let block = &rest[1..end];
                if block.starts_with('\\') {
                    rest = &rest[end + 1..];
                    flush(&mut buf, &cur, &mut spans);
                    for tag in block.split('\\').filter(|t| !t.is_empty()) {
                        if let Some(n) = tag.strip_prefix("an") {
                            align = n.trim().parse::<u8>().ok().filter(|n| (1..=9).contains(n));
                        } else if tag == "i1" {
                            cur.italic = true;
                        } else if tag == "i0" {
                            cur.italic = false;
                        } else if tag == "b1" {
                            cur.bold = true;
                        } else if tag == "b0" {
                            cur.bold = false;
                        } else if tag == "u1" {
                            cur.underline = true;
                        } else if tag == "u0" {
                            cur.underline = false;
                        }
                    }
                    continue;
                }
            }
        }
        buf.push(c);
        rest = &rest[c.len_utf8()..];
    }
    flush(&mut buf, &cur, &mut spans);
    (merge(spans), align)
}

fn font_color(tag: &str) -> Option<String> {
    let i = tag.find("color")?;
    let v = tag[i + 5..].trim_start().strip_prefix('=')?.trim();
    let v = v.trim_matches(|c| c == '"' || c == '\'');
    let v = v.split_whitespace().next()?;
    css_color(v)
}

/// A `#rgb`/`#rrggbb` colour or one of the basic names, as `#rrggbb`.
fn css_color(v: &str) -> Option<String> {
    let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
    if let Some(hex) = v.strip_prefix('#') {
        let hex = hex.get(..6).unwrap_or(hex);
        if hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(format!("#{}", hex.to_ascii_lowercase()));
        }
        if hex.len() == 3 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            let d: String = hex.chars().flat_map(|c| [c, c]).collect();
            return Some(format!("#{}", d.to_ascii_lowercase()));
        }
        return None;
    }
    let named = match v.to_ascii_lowercase().as_str() {
        "white" => "#ffffff",
        "black" => "#000000",
        "red" => "#ff0000",
        "green" => "#008000",
        "lime" => "#00ff00",
        "blue" => "#0000ff",
        "yellow" => "#ffff00",
        "cyan" | "aqua" => "#00ffff",
        "magenta" | "fuchsia" => "#ff00ff",
        "gray" | "grey" => "#808080",
        "silver" => "#c0c0c0",
        "orange" => "#ffa500",
        _ => return None,
    };
    Some(named.into())
}

/// Right-to-left subtitle files made for players that showed every line
/// left to right carry their final punctuation at the start (".سلام" so it
/// looked right there). A line that begins with such punctuation and then a
/// right-to-left letter gets it back at its end; a line that also ends in
/// punctuation, or begins with a dialogue dash, is left alone.
pub fn rtl_punctuation(text: &str) -> String {
    const MOVABLE: &[char] = &['.', '!', '?', '،', '؛', ':', '…', '؟', ','];
    let is_rtl = |c: char| matches!(c, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}');
    text.lines()
        .map(|line| {
            let lead: String = line.chars().take_while(|c| MOVABLE.contains(c)).collect();
            let rest = &line[lead.len()..];
            let next_is_rtl = rest.trim_start().chars().next().is_some_and(is_rtl);
            let ends_punct = rest
                .trim_end()
                .chars()
                .last()
                .is_some_and(|c| MOVABLE.contains(&c));
            if lead.is_empty() || !next_is_rtl || ends_punct {
                line.to_owned()
            } else {
                format!("{}{}", rest.trim_start(), lead)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The few entities subtitle files use.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    s.replace("&nbsp;", "\u{a0}")
        .replace("&lrm;", "\u{200e}")
        .replace("&rlm;", "\u{200f}")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_in_every_format() {
        assert_eq!(parse_time("00:01:02,345"), Some(62_345));
        assert_eq!(parse_time("01:00:00.5"), Some(3_600_500));
        assert_eq!(parse_time("1:02.250"), Some(62_250));
        assert_eq!(parse_time("0:00:01.23"), Some(1_230));
        assert_eq!(parse_time("10:00:00"), Some(36_000_000));
        assert_eq!(parse_time("00:61:00,000"), None);
        assert_eq!(parse_time("aa:00:00,000"), None);
        assert_eq!(parse_time(""), None);
    }

    #[test]
    fn markup_keeps_basic_styles_and_drops_the_rest() {
        let (spans, align) =
            parse_markup("{\\an8}<i>Hello</i> <font color=\"#FF0000\">red</font> <c.yellow>x</c>");
        assert_eq!(align, Some(8));
        assert_eq!(spans[0].text, "Hello");
        assert!(spans[0].italic);
        assert_eq!(spans[1].text, " ");
        assert_eq!(spans[2].color.as_deref(), Some("#ff0000"));
        assert_eq!(plain(&spans), "Hello red x");
    }

    #[test]
    fn rtl_punctuation_typed_first_goes_back_to_the_end() {
        assert_eq!(rtl_punctuation(".تو یه احمقی، تام"), "تو یه احمقی، تام.");
        assert_eq!(rtl_punctuation("!?سلام\nخوبی؟"), "سلام!?\nخوبی؟");
        // Already right, Latin, or a dialogue line: untouched.
        assert_eq!(rtl_punctuation("سلام."), "سلام.");
        assert_eq!(rtl_punctuation("...and then"), "...and then");
        assert_eq!(rtl_punctuation("- سلام"), "- سلام");
        assert_eq!(rtl_punctuation(".سلام."), ".سلام.");
    }

    #[test]
    fn sniffs_the_format() {
        assert_eq!(Format::sniff("WEBVTT\n\n"), Some(Format::Vtt));
        assert_eq!(
            Format::sniff("\u{feff}[Script Info]\nTitle: x"),
            Some(Format::Ass)
        );
        assert_eq!(
            Format::sniff("1\n00:00:01,000 --> 00:00:02,000\nhi"),
            Some(Format::Srt)
        );
        assert_eq!(Format::sniff("hello"), None);
    }
}
