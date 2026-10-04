use std::ops::Range;

use crate::parser::parse_time_tag;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub start_ms: u32,
    pub end_ms: Option<u32>,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Backing {
    pub text: String,
    pub words: Vec<Word>,
}

pub fn locate_words<'a>(
    text: &str,
    cues: impl IntoIterator<Item = (u32, Option<u32>, &'a str)>,
) -> Vec<Word> {
    let mut words = Vec::new();
    let mut cursor = 0;
    for (start_ms, end_ms, value) in cues {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let Some(found) = text[cursor..].find(value) else {
            return Vec::new();
        };
        let from = cursor + found;
        cursor = from + value.len();
        words.push(Word {
            start_ms,
            end_ms,
            range: from..cursor,
        });
    }
    words
}

pub(crate) fn split_enhanced(raw: &str) -> (String, Vec<Word>) {
    let mut text = String::with_capacity(raw.len());
    let mut words: Vec<Word> = Vec::new();
    let mut started: Option<u32> = None;
    let mut rest = raw;
    loop {
        let (segment, marker, after) = match next_marker(rest) {
            Some((before, ms, after)) => (before, Some(ms), after),
            None => (rest, None, ""),
        };
        let segment = if text.ends_with(char::is_whitespace) {
            segment.trim_start()
        } else {
            segment
        };
        let body = segment.trim();
        if let Some(start_ms) = started.take() {
            if body.is_empty() {
                if let Some(last) = words.last_mut().filter(|last| last.end_ms.is_none()) {
                    last.end_ms = Some(start_ms);
                }
            } else {
                let from = text.len() + segment.len() - segment.trim_start().len();
                words.push(Word {
                    start_ms,
                    end_ms: None,
                    range: from..from + body.len(),
                });
            }
        }
        text.push_str(segment);
        let Some(ms) = marker else {
            break;
        };
        started = Some(ms);
        rest = after;
    }
    let lead = text.len() - text.trim_start().len();
    let trimmed = text.trim().to_string();
    for word in &mut words {
        word.range = word.range.start - lead..word.range.end - lead;
    }
    (trimmed, words)
}

fn next_marker(s: &str) -> Option<(&str, u32, &str)> {
    let mut from = 0;
    while let Some(open) = s[from..].find('<').map(|ix| from + ix) {
        let close = open + s[open..].find('>')?;
        if let Some(ms) = parse_time_tag(&s[open + 1..close]) {
            return Some((&s[..open], ms, &s[close + 1..]));
        }
        from = open + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(start_ms: u32, end_ms: Option<u32>, range: Range<usize>) -> Word {
        Word {
            start_ms,
            end_ms,
            range,
        }
    }

    fn spelled<'a>(text: &'a str, words: &[Word]) -> Vec<&'a str> {
        words.iter().map(|w| &text[w.range.clone()]).collect()
    }

    #[test]
    fn enhanced_markers_become_words_and_leave_the_text_clean() {
        let (text, words) = split_enhanced(" <00:01.00>Hello <00:01.50>world<00:02.00> ");
        assert_eq!(text, "Hello world");
        assert_eq!(
            words,
            vec![word(1_000, None, 0..5), word(1_500, Some(2_000), 6..11)]
        );
    }

    #[test]
    fn spaces_on_both_sides_of_a_marker_stay_one_space() {
        let (text, words) = split_enhanced("<00:12.04> When <00:12.16> the <00:12.82> truth");
        assert_eq!(text, "When the truth");
        assert_eq!(spelled(&text, &words), vec!["When", "the", "truth"]);
        assert_eq!(split_enhanced("one  <00:01.00>  two").0, "one  two");
    }

    #[test]
    fn a_marker_before_blank_text_ends_the_previous_word() {
        let (text, words) = split_enhanced("<00:01.00>one<00:01.40> <00:01.50>two");
        assert_eq!(text, "one two");
        assert_eq!(words[0].end_ms, Some(1_400));
        assert_eq!(words[1], word(1_500, None, 4..7));
    }

    #[test]
    fn text_that_only_looks_like_a_marker_is_kept() {
        let (text, words) = split_enhanced("<3 you <00:02.00>always");
        assert_eq!(text, "<3 you always");
        assert_eq!(spelled(&text, &words), vec!["always"]);
        assert_eq!(split_enhanced("a < b > c").0, "a < b > c");
        assert_eq!(split_enhanced("open <00:01.00").0, "open <00:01.00");
    }

    #[test]
    fn multibyte_words_keep_char_boundaries() {
        let (text, words) = split_enhanced("<00:01.00>Привет, <00:01.50>мир <00:02.00>눈을");
        assert_eq!(text, "Привет, мир 눈을");
        assert_eq!(spelled(&text, &words), vec!["Привет,", "мир", "눈을"]);
    }

    #[test]
    fn located_cues_follow_each_other_through_the_text() {
        let text = "la la land";
        let words = locate_words(
            text,
            [
                (0, Some(1), "la "),
                (1, Some(2), " "),
                (2, Some(3), "la"),
                (3, None, "land"),
            ],
        );
        assert_eq!(
            words,
            vec![
                word(0, Some(1), 0..2),
                word(2, Some(3), 3..5),
                word(3, None, 6..10)
            ]
        );
        assert_eq!(
            spelled(
                "눈을 뜬",
                &locate_words("눈을 뜬", [(0, None, "을"), (1, None, "뜬")])
            ),
            vec!["을", "뜬"]
        );
    }

    #[test]
    fn a_cue_missing_from_the_text_drops_every_word() {
        assert!(locate_words("Hello there", [(0, None, "Hello"), (1, None, "world")]).is_empty());
        assert!(locate_words("b a", [(0, None, "a"), (1, None, "b")]).is_empty());
    }
}
