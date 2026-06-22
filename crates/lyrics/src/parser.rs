#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lyrics {
    pub synced: bool,
    pub lines: Vec<LyricLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LyricLine {
    pub time_ms: Option<u32>,
    pub text: String,
}

pub fn parse_lrc(raw: &str) -> Lyrics {
    let mut lines: Vec<LyricLine> = Vec::new();
    let mut synced = false;

    for raw_line in raw.split(['\n', '\r']) {
        let mut rest = raw_line;
        let mut times: Vec<u32> = Vec::new();

        while let Some((inner, after)) = take_bracket(rest) {
            if let Some(ms) = parse_time_tag(inner) {
                times.push(ms);
            }
            rest = after;
        }

        let text = rest.trim();

        if times.is_empty() {
            if text.is_empty() {
                continue;
            }
            lines.push(LyricLine {
                time_ms: None,
                text: text.to_string(),
            });
        } else {
            synced = true;
            for ms in times {
                lines.push(LyricLine {
                    time_ms: Some(ms),
                    text: text.to_string(),
                });
            }
        }
    }

    if synced {
        lines.retain(|line| line.time_ms.is_some());
        lines.sort_by_key(|line| line.time_ms.unwrap_or(0));
    }

    Lyrics { synced, lines }
}

fn take_bracket(s: &str) -> Option<(&str, &str)> {
    let start = s.find('[')?;
    let end_rel = s[start..].find(']')?;
    let end = start + end_rel;
    if !s[..start].trim().is_empty() {
        return None;
    }
    let inner = &s[start + 1..end];
    let after = &s[end + 1..];
    Some((inner, after))
}

fn parse_time_tag(inner: &str) -> Option<u32> {
    let (min_str, rest) = inner.split_once(':')?;
    if min_str.is_empty() || !min_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let minutes: u32 = min_str.parse().ok()?;

    let (sec_str, frac_str) = match rest.split_once('.') {
        Some((s, f)) => (s, Some(f)),
        None => (rest, None),
    };
    if sec_str.is_empty() || !sec_str.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let seconds: u32 = sec_str.parse().ok()?;

    let frac_ms = match frac_str {
        None => 0,
        Some(f) => {
            if f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            match f.len() {
                2 => f.parse::<u32>().ok()? * 10,
                3 => f.parse::<u32>().ok()?,
                _ => {
                    let hundredths: u32 = f.get(..2)?.parse().ok()?;
                    hundredths * 10
                }
            }
        }
    };

    let total_ms = (minutes as u64) * 60_000 + (seconds as u64) * 1_000 + frac_ms as u64;
    u32::try_from(total_ms).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn multi_timestamp_line_duplicates_text() {
        let parsed = parse_lrc("[00:12.00][00:45.30]hello");
        assert!(parsed.synced);
        assert_eq!(
            parsed.lines,
            vec![
                LyricLine {
                    time_ms: Some(12_000),
                    text: "hello".to_string()
                },
                LyricLine {
                    time_ms: Some(45_300),
                    text: "hello".to_string()
                },
            ]
        );
    }

    #[test]
    fn metadata_tags_are_ignored() {
        let raw = "[ti:Song]\n[ar:Artist]\n[al:Album]\n[by:Someone]\n[offset:500]\n[length:03:21]\n[00:01.00]first";
        let parsed = parse_lrc(raw);
        assert!(parsed.synced);
        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].text, "first");
        assert_eq!(parsed.lines[0].time_ms, Some(1_000));
    }

    #[test]
    fn plain_lyrics_have_no_timestamps() {
        let raw = "first line\nsecond line\nthird line";
        let parsed = parse_lrc(raw);
        assert!(!parsed.synced);
        assert_eq!(parsed.lines.len(), 3);
        assert!(parsed.lines.iter().all(|l| l.time_ms.is_none()));
        assert_eq!(parsed.lines[0].text, "first line");
    }

    #[test]
    fn splits_on_carriage_returns() {
        let parsed = parse_lrc("first\rsecond\r\nthird");
        let texts: Vec<_> = parsed.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, vec!["first", "second", "third"]);
    }

    #[test]
    fn empty_lines_are_dropped() {
        let raw = "first\n\n   \nsecond";
        let parsed = parse_lrc(raw);
        assert_eq!(parsed.lines.len(), 2);
        assert_eq!(parsed.lines[0].text, "first");
        assert_eq!(parsed.lines[1].text, "second");
    }

    #[test]
    fn synced_empty_text_lines_are_kept() {
        let raw = "[00:01.00]a\n[00:02.00]\n[00:03.00]b";
        let parsed = parse_lrc(raw);
        assert_eq!(parsed.lines.len(), 3);
        assert_eq!(parsed.lines[1].time_ms, Some(2_000));
        assert_eq!(parsed.lines[1].text, "");
    }

    #[rstest]
    #[case::hundredths("[01:02.50]x", 62_500)]
    #[case::millis("[01:02.500]x", 62_500)]
    #[case::no_frac("[01:02]x", 62_000)]
    #[case::millis_precise("[00:00.123]x", 123)]
    fn fractions_convert_to_ms(#[case] raw: &str, #[case] expected: u32) {
        let parsed = parse_lrc(raw);
        assert_eq!(parsed.lines[0].time_ms, Some(expected));
    }

    #[test]
    fn lines_are_sorted_by_time() {
        let raw = "[00:30.00]c\n[00:10.00]a\n[00:20.00]b";
        let parsed = parse_lrc(raw);
        let times: Vec<_> = parsed.lines.iter().map(|l| l.time_ms).collect();
        assert_eq!(times, vec![Some(10_000), Some(20_000), Some(30_000)]);
        let texts: Vec<_> = parsed.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, vec!["a", "b", "c"]);
    }

    #[test]
    fn internal_whitespace_is_preserved() {
        let parsed = parse_lrc("[00:01.00]  hello   world  ");
        assert_eq!(parsed.lines[0].text, "hello   world");
    }

    #[test]
    fn overflowing_timestamp_is_rejected_not_panicked() {
        let parsed = parse_lrc("[99999:00.00]x\n[00:01.00]ok");
        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].time_ms, Some(1_000));
    }
}
