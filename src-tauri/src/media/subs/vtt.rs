//! WebVTT: the header, NOTE/STYLE/REGION blocks (skipped), cue ids, cue
//! settings (`line:` and `align:` give a top or bottom place), voice and
//! class tags (their text kept).

use super::{parse_markup, parse_time, plain, Cue};

fn timing(line: &str) -> Option<(i64, i64, &str)> {
    let (a, rest) = line.split_once("-->")?;
    let rest = rest.trim_start();
    let (b, settings) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    Some((parse_time(a)?, parse_time(b)?, settings))
}

/// Where the cue's settings put it, as a keypad position.
fn align_from(settings: &str) -> Option<u8> {
    let mut top = None;
    let mut horiz = 2u8;
    for s in settings.split_whitespace() {
        if let Some(v) = s.strip_prefix("line:") {
            let v = v.split(',').next().unwrap_or(v);
            if let Some(p) = v.strip_suffix('%') {
                if let Ok(p) = p.parse::<f64>() {
                    top = Some(p < 50.0);
                }
            } else if let Ok(n) = v.parse::<i64>() {
                // A line number: 0, 1, ... from the top; -1, -2 from the bottom.
                top = Some(n >= 0);
            }
        } else if let Some(v) = s.strip_prefix("align:") {
            horiz = match v {
                "left" | "start" => 1,
                "right" | "end" => 3,
                _ => 2,
            };
        }
    }
    match (top, horiz) {
        (Some(true), h) => Some(6 + h),
        (_, 2) => None,
        (_, h) => Some(h),
    }
}

pub fn parse(text: &str) -> Vec<Cue> {
    let text = text.trim_start_matches('\u{feff}');
    let blocks = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in blocks.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(t) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let first = lines.first().map(|l| l.trim()).unwrap_or("");
        if first.starts_with("NOTE") || first.starts_with("STYLE") || first.starts_with("REGION") {
            continue;
        }
        let Some((start, end, settings)) = timing(lines[t]) else {
            continue;
        };
        let raw = lines[t + 1..]
            .iter()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join("\n");
        let (spans, tag_align) = parse_markup(&raw);
        cues.push(Cue {
            start,
            end,
            text: plain(&spans),
            spans,
            align: tag_align.or_else(|| align_from(settings)),
            position: None,
            style: None,
        });
    }
    cues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_cues_settings_and_voices() {
        let cues = parse(
            "WEBVTT - title\n\nNOTE a note\n\nSTYLE\n::cue { color: red }\n\nid-1\n00:01.000 --> 00:02.000 line:10% align:start\n<v Anna>Hi &amp; bye</v>\n\n00:00:03.000 --> 00:00:04.000\n<i>two</i>\nlines\n",
        );
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, 1000);
        assert_eq!(cues[0].text, "Hi & bye");
        assert_eq!(cues[0].align, Some(7));
        assert_eq!(cues[1].text, "two\nlines");
        assert!(cues[1].spans[0].italic);
        assert_eq!(cues[1].align, None);
    }
}
