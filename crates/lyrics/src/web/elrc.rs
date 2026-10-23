use std::fmt::Write as _;

const GAP_MS: u32 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Syllable {
    pub(super) start_ms: u32,
    pub(super) end_ms: Option<u32>,
    pub(super) text: String,
    pub(super) space_before: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Line {
    pub(super) start_ms: Option<u32>,
    pub(super) text: String,
    pub(super) syllables: Vec<Syllable>,
    pub(super) backing: Vec<Syllable>,
    pub(super) backing_text: String,
}

pub(super) fn write(lines: &[Line]) -> String {
    let mut out = String::new();
    for line in lines {
        if !out.is_empty() {
            out.push('\n');
        }
        let Some(start_ms) = line.start_ms else {
            out.push_str(&one_line(&line.text));
            continue;
        };
        out.push('[');
        stamp(&mut out, start_ms);
        out.push(']');
        if line.syllables.is_empty() {
            out.push_str(&one_line(&line.text));
        } else {
            sung(&mut out, &line.syllables);
        }
        if !line.backing.is_empty() {
            out.push_str("\n[bg: ");
            sung(&mut out, &line.backing);
            out.push(']');
        } else if !line.backing_text.trim().is_empty() {
            out.push_str("\n[bg: ");
            out.push_str(&one_line(&line.backing_text));
            out.push(']');
        }
    }
    out
}

fn sung(out: &mut String, syllables: &[Syllable]) {
    for (ix, syllable) in syllables.iter().enumerate() {
        if ix > 0 && syllable.space_before {
            out.push(' ');
        }
        out.push('<');
        stamp(out, syllable.start_ms);
        out.push('>');
        out.push_str(&one_line(&syllable.text));
        let Some(end_ms) = syllable.end_ms else {
            continue;
        };
        let gap_follows = syllables
            .get(ix + 1)
            .is_none_or(|next| next.start_ms > end_ms.saturating_add(GAP_MS));
        if gap_follows {
            out.push('<');
            stamp(out, end_ms);
            out.push('>');
        }
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn stamp(out: &mut String, ms: u32) {
    let _ = write!(
        out,
        "{:02}:{:02}.{:02}",
        ms / 60_000,
        ms / 1000 % 60,
        ms % 1000 / 10
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_lrc;

    fn syllable(start_ms: u32, end_ms: u32, text: &str, space_before: bool) -> Syllable {
        Syllable {
            start_ms,
            end_ms: Some(end_ms),
            text: text.to_string(),
            space_before,
        }
    }

    #[test]
    fn words_end_only_before_a_gap_and_at_the_line_end() {
        let line = Line {
            start_ms: Some(21_710),
            syllables: vec![
                syllable(21_710, 21_970, "I'm", false),
                syllable(21_970, 22_290, "tired", true),
                syllable(22_510, 22_530, "be", true),
                syllable(22_530, 22_810, "ing", false),
            ],
            ..Default::default()
        };
        let text = write(&[line]);
        assert_eq!(
            text,
            "[00:21.71]<00:21.71>I'm <00:21.97>tired<00:22.29> <00:22.51>be<00:22.53>ing<00:22.81>"
        );

        let parsed = parse_lrc(&text);
        let line = &parsed.lines[0];
        assert_eq!(line.text, "I'm tired being");
        let ends: Vec<Option<u32>> = line.words.iter().map(|w| w.end_ms).collect();
        assert_eq!(ends, vec![None, Some(22_290), None, Some(22_810)]);
    }

    #[test]
    fn backing_follows_its_line_and_untimed_lines_stay_plain() {
        let lines = vec![
            Line {
                start_ms: Some(1_000),
                syllables: vec![syllable(1_000, 1_500, "la", false)],
                backing: vec![syllable(1_200, 1_400, "(ooh)", false)],
                ..Default::default()
            },
            Line {
                start_ms: Some(65_430),
                text: "whole  line".to_string(),
                ..Default::default()
            },
        ];
        assert_eq!(
            write(&lines),
            "[00:01.00]<00:01.00>la<00:01.50>\n[bg: <00:01.20>(ooh)<00:01.40>]\n[01:05.43]whole line"
        );
        let parsed = parse_lrc(&write(&lines));
        assert_eq!(parsed.lines[0].background.as_ref().unwrap().text, "(ooh)");

        let plain = vec![Line {
            text: "just words".to_string(),
            ..Default::default()
        }];
        assert_eq!(write(&plain), "just words");
    }
}
