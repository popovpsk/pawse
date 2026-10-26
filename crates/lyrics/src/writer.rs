use std::fmt::Write as _;

use crate::parser::Lyrics;
use crate::words::Word;

pub fn to_lrc(lyrics: &Lyrics) -> String {
    let mut out = String::new();
    for line in &lyrics.lines {
        if !out.is_empty() {
            out.push('\n');
        }
        match line.time_ms {
            Some(ms) => {
                out.push('[');
                stamp(&mut out, ms);
                out.push(']');
                sung(&mut out, &line.text, &line.words);
            }
            None => out.push_str(&line.text),
        }
        if let Some(backing) = line.background.as_ref().filter(|b| !b.text.is_empty()) {
            out.push_str("\n[bg: ");
            sung(&mut out, &backing.text, &backing.words);
            out.push(']');
        }
    }
    out
}

fn sung(out: &mut String, text: &str, words: &[Word]) {
    match marked(text, words) {
        Some(marked) => out.push_str(&marked),
        None => out.push_str(text),
    }
}

fn marked(text: &str, words: &[Word]) -> Option<String> {
    let first = words.first()?;
    let mut out = String::with_capacity(text.len() + words.len() * 22);
    out.push_str(text.get(..first.range.start)?);
    for (ix, word) in words.iter().enumerate() {
        let until = words
            .get(ix + 1)
            .map_or(text.len(), |next| next.range.start);
        let body = text.get(word.range.clone())?;
        let between = text.get(word.range.end..until)?;
        out.push('<');
        stamp(&mut out, word.start_ms);
        out.push('>');
        out.push_str(body);
        if let Some(end_ms) = word.end_ms.filter(|_| between.trim().is_empty()) {
            out.push('<');
            stamp(&mut out, end_ms);
            out.push('>');
        }
        out.push_str(between);
    }
    Some(out)
}

fn stamp(out: &mut String, ms: u32) {
    let _ = write!(
        out,
        "{:02}:{:02}.{:03}",
        ms / 60_000,
        ms / 1000 % 60,
        ms % 1000
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{LyricLine, parse_lrc};
    use crate::words::Backing;

    fn word(start_ms: u32, end_ms: Option<u32>, range: std::ops::Range<usize>) -> Word {
        Word {
            start_ms,
            end_ms,
            range,
        }
    }

    fn timed(time_ms: u32, text: &str, words: Vec<Word>) -> LyricLine {
        LyricLine {
            time_ms: Some(time_ms),
            text: text.to_string(),
            words,
            background: None,
        }
    }

    #[test]
    fn words_and_backing_survive_a_round_trip() {
        let mut first = timed(
            1_234,
            "Hello, world (yeah)",
            vec![
                word(1_234, Some(1_500), 0..6),
                word(1_600, None, 7..12),
                word(2_001, Some(2_999), 13..19),
            ],
        );
        first.background = Some(Backing {
            text: "(ooh ahh)".to_string(),
            words: vec![word(1_300, Some(1_700), 0..4), word(1_800, None, 5..9)],
        });
        let lyrics = Lyrics {
            synced: true,
            lines: vec![
                first,
                timed(4_000, "", Vec::new()),
                timed(
                    7_005,
                    "Привет, мир 눈을",
                    vec![
                        word(7_005, None, 0..13),
                        word(7_500, Some(7_900), 14..20),
                        word(8_000, Some(8_400), 21..27),
                    ],
                ),
                timed(65_430, "no words  here", Vec::new()),
                timed(
                    70_000,
                    "being",
                    vec![
                        word(70_000, Some(70_200), 0..2),
                        word(70_200, Some(70_500), 2..5),
                    ],
                ),
            ],
        };
        let written = to_lrc(&lyrics);
        assert!(written.starts_with("[00:01.234]<00:01.234>Hello,<00:01.500> <00:01.600>world "));
        assert_eq!(parse_lrc(&written), lyrics);
    }

    #[test]
    fn plain_lyrics_stay_plain_with_their_backing() {
        let lyrics = Lyrics {
            synced: false,
            lines: vec![
                LyricLine {
                    text: "first".to_string(),
                    background: Some(Backing {
                        text: "(echo)".to_string(),
                        words: Vec::new(),
                    }),
                    ..Default::default()
                },
                LyricLine {
                    text: "second".to_string(),
                    ..Default::default()
                },
            ],
        };
        let written = to_lrc(&lyrics);
        assert_eq!(written, "first\n[bg: (echo)]\nsecond");
        assert_eq!(parse_lrc(&written), lyrics);
    }

    #[test]
    fn words_that_do_not_fit_the_text_are_dropped_not_panicked() {
        let lyrics = Lyrics {
            synced: true,
            lines: vec![timed(
                0,
                "мир",
                vec![word(0, None, 1..2), word(10, None, 0..1)],
            )],
        };
        assert_eq!(to_lrc(&lyrics), "[00:00.000]мир");
    }
}
