//! ASS and SSA: the styles' basics (font, size, colour, bold, italic,
//! alignment) and, in the text, `\b \i \u \s \c \1c \an \a \pos \r \N \h`.
//! Everything else (karaoke, transforms, clips, fades, blur, rotation) is
//! dropped and its text kept; drawings (`\p1`) are dropped whole, since
//! they are shapes, not words.

use super::{merge, parse_time, plain, Cue, Span, Style};

/// `&HAABBGGRR` or `&HBBGGRR` (ASS colours are blue-green-red) to `#rrggbb`.
fn color(v: &str) -> Option<String> {
    let v = v.trim().trim_start_matches('&');
    let hex = v.starts_with(['H', 'h']);
    let v = v.trim_start_matches(['H', 'h']).trim_end_matches('&');
    let n = if hex {
        u32::from_str_radix(v, 16).ok()?
    } else if let Some(dec) = v.strip_prefix('-') {
        // Old SSA files write colours as (signed) decimals.
        (-(dec.parse::<i64>().ok()?)) as u32
    } else {
        v.parse::<u32>().ok()?
    };
    let (b, g, r) = ((n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff);
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

/// SSA's legacy `\a` alignment (1-3 bottom, 5-7 top, 9-11 middle) to keypad.
fn legacy_align(n: u8) -> Option<u8> {
    Some(match n {
        1..=3 => n,
        5..=7 => n + 2,
        9..=11 => n - 5,
        _ => return None,
    })
}

struct Fields {
    names: Vec<String>,
}

impl Fields {
    fn parse(line: &str) -> Fields {
        Fields {
            names: line
                .split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .collect(),
        }
    }
    fn index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }
    /// Splits a line's values; the last field (Text) keeps its commas.
    fn split<'a>(&self, values: &'a str) -> Vec<&'a str> {
        values.splitn(self.names.len().max(1), ',').collect()
    }
}

pub fn parse(text: &str) -> (Vec<Cue>, Vec<Style>) {
    let text = text.trim_start_matches('\u{feff}');
    let mut section = String::new();
    let mut play_res_y = 288.0_f64;
    let mut play_res_x = 384.0_f64;
    let mut has_res_x = false;
    let mut style_fields: Option<Fields> = None;
    let mut event_fields: Option<Fields> = None;
    let mut styles: Vec<Style> = Vec::new();
    let mut raw_events: Vec<(i64, i64, String, String)> = Vec::new();
    let mut ssa = false;

    for line in text.lines() {
        let line = line.trim_end_matches('\r').trim_start();
        if line.starts_with('[') && line.ends_with(']') {
            section = line.to_ascii_lowercase();
            if section == "[v4 styles]" {
                ssa = true;
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim_start();
        match section.as_str() {
            "[script info]" => match key.to_ascii_lowercase().as_str() {
                "playresy" => play_res_y = value.trim().parse().unwrap_or(play_res_y),
                "playresx" => {
                    play_res_x = value.trim().parse().unwrap_or(play_res_x);
                    has_res_x = true;
                }
                "scripttype"
                    if value.to_ascii_lowercase().contains("v4.00") && !value.contains('+') =>
                {
                    ssa = true
                }
                _ => {}
            },
            "[v4+ styles]" | "[v4 styles]" => {
                if key.eq_ignore_ascii_case("format") {
                    style_fields = Some(Fields::parse(value));
                } else if key.eq_ignore_ascii_case("style") {
                    if let Some(f) = &style_fields {
                        if let Some(s) = style(f, value, ssa) {
                            styles.push(s);
                        }
                    }
                }
            }
            "[events]" => {
                if key.eq_ignore_ascii_case("format") {
                    event_fields = Some(Fields::parse(value));
                } else if key.eq_ignore_ascii_case("dialogue") {
                    if let Some(f) = &event_fields {
                        let v = f.split(value);
                        let get = |n: &str| f.index(n).and_then(|i| v.get(i)).copied();
                        let (Some(start), Some(end), Some(text)) = (
                            get("start").and_then(parse_time),
                            get("end").and_then(parse_time),
                            get("text"),
                        ) else {
                            continue;
                        };
                        let style = get("style")
                            .unwrap_or("Default")
                            .trim()
                            .trim_start_matches('*');
                        raw_events.push((start, end, style.to_owned(), text.to_owned()));
                    }
                }
            }
            _ => {}
        }
    }
    // A missing PlayResX follows the height at 4:3, as renderers do.
    if !has_res_x {
        play_res_x = play_res_y * 4.0 / 3.0;
    }
    for s in &mut styles {
        s.size /= play_res_y.max(1.0);
    }
    let cues = raw_events
        .into_iter()
        .filter_map(|(start, end, style_name, text)| {
            let base = styles
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(&style_name))
                .or_else(|| {
                    styles
                        .iter()
                        .find(|s| s.name.eq_ignore_ascii_case("default"))
                });
            let (spans, align, pos) = parse_text(&text, base, &styles);
            if spans.is_empty() {
                return None;
            }
            Some(Cue {
                start,
                end,
                text: plain(&spans),
                spans,
                align: align.or(base.map(|b| b.align)).filter(|a| *a != 2),
                position: pos.map(|(x, y)| {
                    (
                        (x / play_res_x).clamp(0.0, 1.0),
                        (y / play_res_y).clamp(0.0, 1.0),
                    )
                }),
                style: base.map(|b| b.name.clone()),
            })
        })
        .collect();
    (cues, styles)
}

fn style(f: &Fields, value: &str, ssa: bool) -> Option<Style> {
    let v: Vec<&str> = value.split(',').map(str::trim).collect();
    let get = |n: &str| f.index(n).and_then(|i| v.get(i)).copied();
    let flag = |n: &str| get(n).map(|x| x != "0" && !x.is_empty()).unwrap_or(false);
    let align: u8 = get("alignment").and_then(|a| a.parse().ok()).unwrap_or(2);
    let align = if ssa {
        legacy_align(align).unwrap_or(2)
    } else {
        align.clamp(1, 9)
    };
    Some(Style {
        name: get("name")?.to_owned(),
        font: get("fontname").unwrap_or("").to_owned(),
        size: get("fontsize").and_then(|s| s.parse().ok()).unwrap_or(20.0),
        color: get("primarycolour")
            .and_then(color)
            .unwrap_or_else(|| "#ffffff".into()),
        outline_color: get("outlinecolour")
            .or(get("tertiarycolour"))
            .and_then(color)
            .unwrap_or_else(|| "#000000".into()),
        bold: flag("bold"),
        italic: flag("italic"),
        align,
    })
}

/// A Dialogue line's text: spans, an `\an` alignment and a `\pos`.
fn parse_text(
    text: &str,
    base: Option<&Style>,
    styles: &[Style],
) -> (Vec<Span>, Option<u8>, Option<(f64, f64)>) {
    let reset = |s: Option<&Style>| Span {
        bold: s.is_some_and(|s| s.bold),
        italic: s.is_some_and(|s| s.italic),
        ..Span::default()
    };
    let mut cur = reset(base);
    let mut spans = Vec::new();
    let mut align = None;
    let mut pos = None;
    let mut drawing = false;
    let mut buf = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('{') {
            let Some(end) = stripped.find('}') else {
                break;
            };
            let block = &stripped[..end];
            rest = &stripped[end + 1..];
            if !buf.is_empty() {
                let mut s = cur.clone();
                s.text = std::mem::take(&mut buf);
                spans.push(s);
            }
            for tag in split_tags(block) {
                apply(
                    tag,
                    &mut cur,
                    &mut align,
                    &mut pos,
                    &mut drawing,
                    base,
                    styles,
                );
            }
            continue;
        }
        if let Some(r) = rest
            .strip_prefix("\\N")
            .or_else(|| rest.strip_prefix("\\n"))
        {
            if !drawing {
                buf.push('\n');
            }
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("\\h") {
            if !drawing {
                buf.push('\u{a0}');
            }
            rest = r;
            continue;
        }
        let c = rest.chars().next().unwrap_or(' ');
        if !drawing {
            buf.push(c);
        }
        rest = &rest[c.len_utf8()..];
    }
    if !buf.is_empty() {
        let mut s = cur.clone();
        s.text = buf;
        spans.push(s);
    }
    (merge(spans), align, pos)
}

/// The tags in an override block: `\b1\i1\pos(1,2)` → ["b1", "i1", "pos(1,2)"],
/// keeping backslashes inside parentheses (`\t(\fs20)`) with their tag.
fn split_tags(block: &str) -> Vec<&str> {
    let mut tags = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    for (i, c) in block.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            '\\' if depth <= 0 => {
                if let Some(s) = start {
                    tags.push(block[s..i].trim());
                }
                start = Some(i + 1);
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        tags.push(block[s..].trim());
    }
    tags.into_iter().filter(|t| !t.is_empty()).collect()
}

fn apply(
    tag: &str,
    cur: &mut Span,
    align: &mut Option<u8>,
    pos: &mut Option<(f64, f64)>,
    drawing: &mut bool,
    base: Option<&Style>,
    styles: &[Style],
) {
    let on = |v: &str| v.trim() != "0";
    if let Some(v) = tag.strip_prefix("an") {
        *align = v.trim().parse::<u8>().ok().filter(|n| (1..=9).contains(n));
    } else if let Some(v) = tag.strip_prefix("pos(") {
        let v = v.trim_end_matches(')');
        let mut it = v.split(',').map(|n| n.trim().parse::<f64>());
        if let (Some(Ok(x)), Some(Ok(y))) = (it.next(), it.next()) {
            *pos = Some((x, y));
        }
    } else if let Some(v) = tag.strip_prefix('p') {
        if v.bytes().all(|b| b.is_ascii_digit()) && !v.is_empty() {
            *drawing = v != "0";
        }
    } else if let Some(v) = tag.strip_prefix("1c").or_else(|| tag.strip_prefix('c')) {
        if v.is_empty() {
            cur.color = None;
        } else if let Some(c) = color(v) {
            cur.color = Some(c);
        }
    } else if let Some(v) = tag.strip_prefix('r') {
        let next = if v.trim().is_empty() {
            base
        } else {
            styles.iter().find(|s| s.name == v.trim()).or(base)
        };
        *cur = Span {
            bold: next.is_some_and(|s| s.bold),
            italic: next.is_some_and(|s| s.italic),
            ..Span::default()
        };
    } else if let Some(v) = tag
        .strip_prefix('a')
        .filter(|v| v.bytes().all(|b| b.is_ascii_digit()) && !v.is_empty())
    {
        *align = v.parse::<u8>().ok().and_then(legacy_align);
    } else if let Some(v) = tag
        .strip_prefix('b')
        .filter(|v| v.bytes().all(|b| b.is_ascii_digit()) && !v.is_empty())
    {
        // \b1, \b0, or a weight (\b700).
        let n: u32 = v.parse().unwrap_or(0);
        cur.bold = n == 1 || n >= 600;
    } else if let Some(v) = tag.strip_prefix('i').filter(|v| v.len() == 1) {
        cur.italic = on(v);
    } else if let Some(v) = tag.strip_prefix('u').filter(|v| v.len() == 1) {
        cur.underline = on(v);
    } else if let Some(v) = tag.strip_prefix('s').filter(|v| v.len() == 1) {
        cur.strike = on(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,54,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,3,1,2,40,40,50,1\nStyle: Sign,Arial,48,&H0000FFFF,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,0,8,40,40,40,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.50,Default,,0,0,0,,{\\b1}Hello{\\b0}, world, again\nDialogue: 0,0:00:04.00,0:00:05.00,Default,,0,0,0,,{\\i1}one{\\i0}\\N{\\c&H00FF00&}two{\\fad(200,200)\\blur3}\nDialogue: 1,0:00:01.00,0:00:09.00,Sign,,0,0,0,,{\\an8\\pos(960,80)}A SIGN\nDialogue: 0,0:00:06.00,0:00:07.00,Default,,0,0,0,,{\\p1}m 0 0 l 100 0 100 100{\\p0}\nComment: 0,0:00:06.00,0:00:07.00,Default,,0,0,0,,not shown\n";

    #[test]
    fn reads_styles_and_events() {
        let (mut cues, styles) = parse(FILE);
        cues.sort_by_key(|c| c.start);
        assert_eq!(styles.len(), 2);
        assert!((styles[0].size - 0.05).abs() < 1e-9);
        assert_eq!(styles[1].color, "#ffff00");
        assert!(styles[1].bold);
        assert_eq!(styles[1].align, 8);
        // The drawing is gone, the comment never read.
        assert_eq!(cues.len(), 3);
        let hello = cues.iter().find(|c| c.text.starts_with("Hello")).unwrap();
        assert_eq!(hello.text, "Hello, world, again");
        assert!(hello.spans[0].bold && !hello.spans[1].bold);
        let sign = cues.iter().find(|c| c.text == "A SIGN").unwrap();
        assert_eq!(sign.align, Some(8));
        let (x, y) = sign.position.unwrap();
        assert!((x - 0.5).abs() < 1e-9 && (y - 80.0 / 1080.0).abs() < 1e-9);
        assert_eq!(sign.style.as_deref(), Some("Sign"));
        let two = cues.iter().find(|c| c.start == 4000).unwrap();
        assert_eq!(two.text, "one\ntwo");
        assert_eq!(two.spans.last().unwrap().color.as_deref(), Some("#00ff00"));
    }

    #[test]
    fn ssa_legacy_alignment_and_colours() {
        assert_eq!(legacy_align(6), Some(8));
        assert_eq!(legacy_align(10), Some(5));
        assert_eq!(color("&H0000FF&").as_deref(), Some("#ff0000"));
        assert_eq!(color("16777215").as_deref(), Some("#ffffff"));
    }
}
