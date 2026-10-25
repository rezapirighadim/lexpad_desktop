//! SubRip (.srt), as files really are: numbers missing or wrong, `.` for
//! `,`, Windows line ends, blank lines inside a cue, `{\an8}` tags.

use super::{parse_markup, parse_time, plain, Cue};

/// A timing line: `00:00:01,000 --> 00:00:02,500`, anything after it ignored.
pub fn timing(line: &str) -> Option<(i64, i64)> {
    let (a, b) = line.split_once("-->")?;
    let b = b.split_whitespace().next()?;
    let start = parse_time(a)?;
    let end = parse_time(b)?;
    Some((start, end))
}

pub fn parse(text: &str) -> Vec<Cue> {
    let text = text.trim_start_matches('\u{feff}');
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut cues = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some((start, end)) = timing(lines[i]) else {
            i += 1;
            continue;
        };
        i += 1;
        // The text runs to the next timing line, less the cue number before
        // it and the blank lines between.
        let mut body: Vec<&str> = Vec::new();
        while i < lines.len() && timing(lines[i]).is_none() {
            body.push(lines[i]);
            i += 1;
        }
        if i < lines.len() {
            // The line before the next timing is its number, when it is one.
            if let Some(last) = body.last() {
                if last.trim().bytes().all(|b| b.is_ascii_digit()) && !last.trim().is_empty() {
                    body.pop();
                }
            }
        }
        while body.last().is_some_and(|l| l.trim().is_empty()) {
            body.pop();
        }
        let raw = body.iter().map(|l| l.trim()).collect::<Vec<_>>().join("\n");
        let (spans, align) = parse_markup(&raw);
        let text = plain(&spans);
        cues.push(Cue {
            start,
            end,
            text,
            spans,
            align,
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
    fn reads_a_normal_file() {
        let cues = parse(
            "1\r\n00:00:01,000 --> 00:00:02,500\r\nHello\r\nthere\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000\r\n<b>Bye</b>\r\n",
        );
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, 1000);
        assert_eq!(cues[0].end, 2500);
        assert_eq!(cues[0].text, "Hello\nthere");
        assert!(cues[1].spans[0].bold);
    }

    #[test]
    fn survives_missing_numbers_dots_and_blank_lines_in_a_cue() {
        let cues = parse(
            "00:00:01.000 --> 00:00:02.000 X1:0\nfirst\n\nstill first\n\n7\n00:00:05,000 --> 00:00:06,000\n{\\an8}top\n",
        );
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "first\nstill first");
        assert_eq!(cues[1].align, Some(8));
        assert_eq!(cues[1].text, "top");
    }

    #[test]
    fn rtl_text_is_kept_as_is() {
        let cues = parse("1\n00:00:01,000 --> 00:00:02,000\nسلام، دنیا!\n");
        assert_eq!(cues[0].text, "سلام، دنیا!");
    }
}
