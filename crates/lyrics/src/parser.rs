use crate::words::{Backing, Word, split_enhanced};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lyrics {
    pub synced: bool,
    pub lines: Vec<LyricLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LyricLine {
    pub time_ms: Option<u32>,
    pub text: String,
    pub words: Vec<Word>,
    pub background: Option<Backing>,
}

const BACKING_TAG: &str = "bg:";

pub fn parse_lrc(raw: &str) -> Lyrics {
    let mut lines: Vec<LyricLine> = Vec::new();
    let mut synced = false;
    let mut last_group = 0..0;

    for raw_line in raw.split(['\n', '\r']) {
        let mut rest = raw_line;
        let mut times: Vec<u32> = Vec::new();
        let mut backing: Option<&str> = None;

        while let Some((inner, after)) = take_bracket(rest) {
            if let Some(ms) = parse_time_tag(inner) {
                times.push(ms);
            } else if let Some(sung) = inner.trim_start().strip_prefix(BACKING_TAG) {
                backing = Some(sung);
            }
            rest = after;
        }

        if let Some(sung) = backing.filter(|_| times.is_empty()) {
            let (text, words) = split_enhanced(sung);
            let words = if last_group.len() == 1 {
                words
            } else {
                Vec::new()
            };
            for line in &mut lines[last_group.clone()] {
                add_backing(line, &text, &words);
            }
            continue;
        }

        let (text, words) = split_enhanced(rest);

        if times.is_empty() {
            if text.is_empty() {
                continue;
            }
            last_group = lines.len()..lines.len() + 1;
            lines.push(LyricLine {
                time_ms: None,
                text,
                words: Vec::new(),
                background: None,
            });
        } else {
            synced = true;
            let words = if times.len() == 1 { words } else { Vec::new() };
            last_group = lines.len()..lines.len() + times.len();
            for ms in times {
                lines.push(LyricLine {
                    time_ms: Some(ms),
                    text: text.clone(),
                    words: words.clone(),
                    background: None,
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

fn add_backing(line: &mut LyricLine, text: &str, words: &[Word]) {
    if text.is_empty() {
        return;
    }
    if line.text.is_empty() && line.background.is_none() {
        line.text = text.to_string();
        line.words = words.to_vec();
        return;
    }
    let backing = line.background.get_or_insert_with(Backing::default);
    let shift = if backing.text.is_empty() {
        0
    } else {
        backing.text.push(' ');
        backing.text.len()
    };
    backing.text.push_str(text);
    backing.words.extend(words.iter().map(|word| Word {
        range: word.range.start + shift..word.range.end + shift,
        ..word.clone()
    }));
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

pub(crate) fn parse_time_tag(inner: &str) -> Option<u32> {
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
                    text: "hello".to_string(),
                    ..Default::default()
                },
                LyricLine {
                    time_ms: Some(45_300),
                    text: "hello".to_string(),
                    ..Default::default()
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

    #[test]
    fn enhanced_lines_keep_their_words() {
        let parsed =
            parse_lrc("[00:01.00]<00:01.00>Hello <00:01.60>world<00:02.20>\n[00:03.00]plain");
        assert!(parsed.synced);
        assert_eq!(parsed.lines[0].text, "Hello world");
        assert_eq!(
            parsed.lines[0].words,
            vec![
                Word {
                    start_ms: 1_000,
                    end_ms: None,
                    range: 0..5
                },
                Word {
                    start_ms: 1_600,
                    end_ms: Some(2_200),
                    range: 6..11
                },
            ]
        );
        assert!(parsed.lines[1].words.is_empty());
    }

    #[test]
    fn words_are_dropped_where_they_cannot_belong_to_one_line() {
        let repeated = parse_lrc("[00:01.00][00:09.00]<00:01.00>chorus");
        assert_eq!(repeated.lines.len(), 2);
        assert!(
            repeated
                .lines
                .iter()
                .all(|l| l.words.is_empty() && l.text == "chorus")
        );
        let plain = parse_lrc("<00:01.00>no line stamp");
        assert!(!plain.synced);
        assert_eq!(plain.lines[0].text, "no line stamp");
        assert!(plain.lines[0].words.is_empty());
    }

    #[test]
    fn a_bg_line_is_backing_vocals_of_the_line_before() {
        let raw = "[00:01.00]<00:01.00>Hello\n[bg: <00:01.50>(hello <00:02.00>there)]\n[bg:(again)]\n[00:03.00]Next";
        let parsed = parse_lrc(raw);
        assert_eq!(parsed.lines.len(), 2);
        let backing = parsed.lines[0].background.as_ref().unwrap();
        assert_eq!(backing.text, "(hello there) (again)");
        let spelled: Vec<&str> = backing
            .words
            .iter()
            .map(|w| &backing.text[w.range.clone()])
            .collect();
        assert_eq!(spelled, vec!["(hello", "there)"]);
        assert_eq!(parsed.lines[1].background, None);
        assert_eq!(parse_lrc("[bg: orphan]\n[00:01.00]a").lines.len(), 1);
    }

    #[test]
    fn a_repeated_line_gets_its_backing_on_every_copy_without_words() {
        let parsed = parse_lrc("[00:10.00][01:10.00]Chorus\n[bg: <00:10.50>(ooh)]");
        assert_eq!(parsed.lines.len(), 2);
        for line in &parsed.lines {
            let backing = line.background.as_ref().unwrap();
            assert_eq!(backing.text, "(ooh)");
            assert!(backing.words.is_empty());
        }
    }

    #[test]
    fn backing_vocals_after_a_blank_line_become_its_text() {
        let parsed = parse_lrc("[00:20.00]\n[bg: <00:20.10>(ahh)]\n[bg: (oh)]");
        let line = &parsed.lines[0];
        assert_eq!(line.text, "(ahh)");
        assert_eq!(line.words.len(), 1);
        assert_eq!(
            line.background.as_ref().map(|b| b.text.as_str()),
            Some("(oh)")
        );
    }
}
