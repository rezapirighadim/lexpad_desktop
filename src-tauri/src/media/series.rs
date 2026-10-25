//! Series: which episode a file is (`S01E02`, `s1e2`, `1x02`, `Season 1
//! Episode 2`, `Episode 3`, `Ep03`, an anime `- 03`), the other episodes of
//! the same show in its folder in order, and which subtitle files belong to
//! which episode.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Episode {
    pub season: Option<u32>,
    pub episode: u32,
}

/// The episode and the show's name before it (normalized), from a file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub show: String,
    pub ep: Episode,
}

fn patterns() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        [
            // S01E02, s1e2, S01.E02, S01_E02, S01E02E03 (the first)
            r"(?i)(?:^|[^a-z0-9])s(\d{1,2})[ ._-]?e(\d{1,4})(?:[^0-9]|$)",
            // 1x02, 01x02
            r"(?i)(?:^|[^a-z0-9])(\d{1,2})x(\d{2,3})(?:[^0-9]|$)",
            // Season 1 Episode 2, Season 1 - Ep 2
            r"(?i)season[ ._-]*(\d{1,2})[ ._-]*(?:-[ ._-]*)?(?:episode|ep)[ ._-]*(\d{1,4})",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("valid pattern"))
        .collect()
    })
}

fn episode_only() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        [
            // Episode 3, Ep03, Ep.3, E03 (alone)
            r"(?i)(?:^|[^a-z0-9])(?:episode|ep|e)[ ._-]*(\d{1,4})(?:[^0-9]|$)",
            // Show - 03 [1080p], Show - 03v2
            r"(?:^|\s)-\s*(\d{1,4})(?:v\d)?(?:\s|\[|\(|$)",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("valid pattern"))
        .collect()
    })
}

/// Lowercase words only, so `Show.Name`, `show name` and `Show_Name -` match.
pub fn normalize_show(s: &str) -> String {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn stem(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((s, ext)) if ext.len() <= 5 && !ext.contains(' ') => s,
        _ => name,
    }
}

pub fn parse(file_name: &str) -> Option<Parsed> {
    let s = stem(file_name);
    for re in patterns() {
        if let Some(c) = re.captures(s) {
            let whole = c.get(0)?;
            return Some(Parsed {
                show: normalize_show(&s[..whole.start()]),
                ep: Episode {
                    season: c[1].parse().ok(),
                    episode: c[2].parse().ok()?,
                },
            });
        }
    }
    for re in episode_only() {
        if let Some(c) = re.captures(s) {
            let whole = c.get(0)?;
            let n: u32 = c[1].parse().ok()?;
            // A year or a resolution is not an episode.
            if (1900..=2100).contains(&n) || [480, 576, 720, 1080, 2160].contains(&n) {
                continue;
            }
            return Some(Parsed {
                show: normalize_show(&s[..whole.start()]),
                ep: Episode {
                    season: None,
                    episode: n,
                },
            });
        }
    }
    None
}

/// Natural order: "Episode 2" before "Episode 10".
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na = a.iter().take_while(|c| c.is_ascii_digit()).count();
                let nb = b.iter().take_while(|c| c.is_ascii_digit()).count();
                let (da, db) = (&a[..na], &b[..nb]);
                let ta = da.iter().skip_while(|c| **c == b'0').count();
                let tb = db.iter().skip_while(|c| **c == b'0').count();
                let o = ta.cmp(&tb).then_with(|| da[na - ta..].cmp(&db[nb - tb..]));
                if o != Ordering::Equal {
                    return o;
                }
                a = &a[na..];
                b = &b[nb..];
            }
            (Some(x), Some(y)) => {
                let o = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

/// The episodes of `current`'s show among `videos` (file paths in the same
/// folder), in order. A file with no episode number is a series of one.
pub fn episodes_of(current: &Path, videos: &[PathBuf]) -> Vec<PathBuf> {
    let name = |p: &Path| {
        p.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned()
    };
    let Some(me) = parse(&name(current)) else {
        return vec![current.to_path_buf()];
    };
    let mut list: Vec<(Episode, String, PathBuf)> = videos
        .iter()
        .filter_map(|p| {
            let n = name(p);
            let parsed = parse(&n)?;
            // The same show; a folder of one show often names episodes
            // with no show name at all ("Episode 3.mkv").
            if parsed.show != me.show && !(parsed.show.is_empty() || me.show.is_empty()) {
                return None;
            }
            // Other seasons in the same folder stay in the list, in order:
            // the last episode of season 1 is followed by season 2's first.
            Some((parsed.ep, n, p.clone()))
        })
        .collect();
    if !list.iter().any(|(_, _, p)| p == current) {
        list.push((me.ep, name(current), current.to_path_buf()));
    }
    list.sort_by(|a, b| {
        let sa = a.0.season.unwrap_or(0);
        let sb = b.0.season.unwrap_or(0);
        sa.cmp(&sb)
            .then(a.0.episode.cmp(&b.0.episode))
            .then_with(|| natural_cmp(&a.1, &b.1))
    });
    list.dedup_by(|a, b| a.2 == b.2);
    list.into_iter().map(|(_, _, p)| p).collect()
}

/// How well a subtitle file fits a video: 2 its name begins with the video's
/// name; 1 the same show and episode; 0 not this video's.
pub fn subtitle_fit(video: &Path, subtitle: &Path) -> u8 {
    let vname = video.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let sname = subtitle.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let vstem = normalize_show(stem(vname));
    let sstem = normalize_show(stem(sname));
    if !vstem.is_empty() && (sstem == vstem || sstem.starts_with(&format!("{vstem} "))) {
        return 2;
    }
    // In a release's own subtitle folder named after the video (Subs/<video>/x.srt).
    if subtitle
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .is_some_and(|d| normalize_show(d) == vstem)
    {
        return 2;
    }
    match (parse(vname), parse(sname)) {
        (Some(v), Some(s)) => {
            let same_ep = v.ep.episode == s.ep.episode
                && (v.ep.season == s.ep.season || v.ep.season.is_none() || s.ep.season.is_none());
            let same_show = v.show == s.show || v.show.is_empty() || s.show.is_empty();
            u8::from(same_ep && same_show)
        }
        (None, None) => {
            // A film with one subtitle beside it of another name still fits
            // weakly when nothing in either names an episode; the caller
            // decides (only when the folder has a single video).
            0
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(s: Option<u32>, e: u32) -> Episode {
        Episode {
            season: s,
            episode: e,
        }
    }

    #[test]
    fn episode_patterns() {
        let p = parse("Show.Name.S01E02.720p.mkv").unwrap();
        assert_eq!((p.show.as_str(), p.ep), ("show name", ep(Some(1), 2)));
        assert_eq!(parse("show name s1e10.mp4").unwrap().ep, ep(Some(1), 10));
        assert_eq!(
            parse("Show - S02.E03 - Title.mkv").unwrap().ep,
            ep(Some(2), 3)
        );
        assert_eq!(
            parse("show name 1x02 English.srt").unwrap().ep,
            ep(Some(1), 2)
        );
        assert_eq!(
            parse("show name 1x02 English.srt").unwrap().show,
            "show name"
        );
        assert_eq!(
            parse("Show Season 2 Episode 5.avi").unwrap().ep,
            ep(Some(2), 5)
        );
        assert_eq!(parse("Episode 3.mkv").unwrap().ep, ep(None, 3));
        assert_eq!(parse("Show Ep03.mkv").unwrap().ep, ep(None, 3));
        assert_eq!(
            parse("[Group] Anime Title - 03 [1080p].mkv").unwrap().ep,
            ep(None, 3)
        );
        assert_eq!(parse("Show.S01E01E02.mkv").unwrap().ep, ep(Some(1), 1));
        assert!(parse("Movie.2019.1080p.mkv").is_none());
        assert!(parse("holiday.mp4").is_none());
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["Episode 10", "Episode 2", "episode 1"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["episode 1", "Episode 2", "Episode 10"]);
    }

    #[test]
    fn episodes_in_order_of_the_same_show() {
        let dir = PathBuf::from("/tv");
        let videos: Vec<PathBuf> = [
            "Show.Name.S01E03.mkv",
            "Show.Name.S01E01.mkv",
            "Other.Show.S01E01.mkv",
            "Show.Name.S01E02.mkv",
            "Show.Name.S01E10.mkv",
            "Show.Name.S02E01.mkv",
        ]
        .iter()
        .map(|n| dir.join(n))
        .collect();
        let list = episodes_of(&dir.join("Show.Name.S01E02.mkv"), &videos);
        let names: Vec<_> = list
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "Show.Name.S01E01.mkv",
                "Show.Name.S01E02.mkv",
                "Show.Name.S01E03.mkv",
                "Show.Name.S01E10.mkv",
                "Show.Name.S02E01.mkv"
            ]
        );
        let single = episodes_of(&dir.join("film.mkv"), &videos);
        assert_eq!(single.len(), 1);
    }

    #[test]
    fn subtitles_per_episode() {
        let v = Path::new("/tv/Show.Name.S01E02.720p.mkv");
        assert_eq!(
            subtitle_fit(v, Path::new("/tv/Show.Name.S01E02.720p.fa.srt")),
            2
        );
        assert_eq!(subtitle_fit(v, Path::new("/tv/Show.Name.S01E02.fa.srt")), 1);
        assert_eq!(
            subtitle_fit(v, Path::new("/tv/show name 1x02 English.srt")),
            1
        );
        assert_eq!(subtitle_fit(v, Path::new("/tv/Show.Name.S01E03.fa.srt")), 0);
        assert_eq!(
            subtitle_fit(v, Path::new("/tv/Subs/Show.Name.S01E02.720p/2_English.srt")),
            2
        );
        let film = Path::new("/films/movie.mkv");
        assert_eq!(subtitle_fit(film, Path::new("/films/movie.fa.srt")), 2);
        assert_eq!(subtitle_fit(film, Path::new("/films/other.srt")), 0);
    }
}
