use roxmltree::{Document, Node};

use super::elrc::{self, Line, Syllable};

const METADATA_NS: &str = "http://www.w3.org/ns/ttml#metadata";
const BACKGROUND: &str = "x-bg";
const ASIDES: [&str; 2] = ["x-translation", "x-roman"];

pub(super) fn to_lrc(xml: &str) -> Option<String> {
    let document = Document::parse(xml).ok()?;
    let root = document.root_element();
    if !named(root, "tt") {
        return None;
    }
    let body = root.children().find(|node| named(*node, "body"))?;
    let lines: Vec<Line> = body
        .descendants()
        .filter(|node| named(*node, "p"))
        .filter_map(line)
        .collect();
    Some(elrc::write(&lines))
}

#[derive(Default)]
struct Sung {
    syllables: Vec<Syllable>,
    text: String,
    lead: String,
}

#[derive(Default)]
struct Collected {
    main: Sung,
    backing: Sung,
}

impl Collected {
    fn sung(&mut self, in_backing: bool) -> &mut Sung {
        if in_backing {
            &mut self.backing
        } else {
            &mut self.main
        }
    }
}

fn line(paragraph: Node) -> Option<Line> {
    let mut collected = Collected::default();
    let mut space = false;
    collect(paragraph, false, &mut collected, &mut space);
    let Collected { main, backing } = collected;
    let start_ms = paragraph
        .attribute("begin")
        .and_then(clock)
        .or_else(|| main.syllables.first().map(|s| s.start_ms))
        .or_else(|| backing.syllables.first().map(|s| s.start_ms));
    if start_ms.is_none() && main.text.trim().is_empty() {
        return None;
    }
    let backing_text = if backing.syllables.is_empty() {
        backing.text
    } else {
        String::new()
    };
    Some(Line {
        start_ms,
        text: main.text,
        syllables: main.syllables,
        backing: backing.syllables,
        backing_text,
    })
}

fn collect(parent: Node, in_backing: bool, out: &mut Collected, space: &mut bool) {
    for child in parent.children() {
        if child.is_text() {
            let text = child.text().unwrap_or_default();
            let sung = out.sung(in_backing);
            sung.text.push_str(text);
            if sung.syllables.is_empty() {
                sung.lead.push_str(text);
            }
            if text.trim().is_empty() {
                *space |= !text.is_empty();
                continue;
            }
            if let Some(last) = sung.syllables.last_mut() {
                if std::mem::take(space) || text.starts_with(char::is_whitespace) {
                    last.text.push(' ');
                }
                last.text.push_str(text.trim());
            }
            *space = text.ends_with(char::is_whitespace);
            continue;
        }
        if named(child, "br") {
            let sung = out.sung(in_backing);
            sung.text.push(' ');
            if sung.syllables.is_empty() {
                sung.lead.push(' ');
            }
            *space = true;
            continue;
        }
        if !named(child, "span") {
            continue;
        }
        match child.attribute((METADATA_NS, "role")) {
            Some(role) if ASIDES.contains(&role) => continue,
            Some(BACKGROUND) => {
                let backing = &mut out.backing;
                let mut backing_space = !backing.syllables.is_empty();
                if !backing.text.trim().is_empty() {
                    backing.text.push(' ');
                    if backing.syllables.is_empty() {
                        backing.lead.push(' ');
                    }
                }
                collect(child, true, out, &mut backing_space);
                continue;
            }
            _ => {}
        }
        let Some(start_ms) = child.attribute("begin").and_then(clock) else {
            collect(child, in_backing, out, space);
            continue;
        };
        let text: String = child
            .descendants()
            .filter(|node| node.is_text())
            .filter_map(|node| node.text())
            .collect();
        let sung = out.sung(in_backing);
        sung.text.push_str(&text);
        if text.trim().is_empty() {
            *space = true;
            continue;
        }
        let space_before = std::mem::take(space)
            || text.starts_with(char::is_whitespace)
            || sung
                .syllables
                .last()
                .is_some_and(|last| last.text.ends_with(char::is_whitespace));
        if let Some(last) = sung.syllables.last_mut() {
            last.text.truncate(last.text.trim_end().len());
        }
        let lead = std::mem::take(&mut sung.lead);
        sung.syllables.push(Syllable {
            start_ms,
            end_ms: child.attribute("end").and_then(clock),
            text: format!("{lead}{text}").trim_start().to_string(),
            space_before,
        });
    }
}

fn named(node: Node, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

fn clock(value: &str) -> Option<u32> {
    let value = value.trim();
    if let Some(ms) = value.strip_suffix("ms") {
        let (whole, fraction) = ms.split_once('.').unwrap_or((ms, ""));
        if whole.is_empty() || !digits(whole) || !digits(fraction) {
            return None;
        }
        return u32::try_from(whole.parse::<u64>().ok()?).ok();
    }
    if let Some(seconds) = value.strip_suffix('s') {
        return seconds_ms(seconds);
    }
    let mut parts = value.rsplit(':');
    let mut total = u64::from(seconds_ms(parts.next()?)?);
    for unit in [60_000u64, 3_600_000] {
        let Some(part) = parts.next() else {
            break;
        };
        if part.is_empty() || !digits(part) {
            return None;
        }
        total = total.checked_add(part.parse::<u64>().ok()?.checked_mul(unit)?)?;
    }
    if parts.next().is_some() {
        return None;
    }
    u32::try_from(total).ok()
}

fn seconds_ms(value: &str) -> Option<u32> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty() || !digits(whole) || !digits(fraction) {
        return None;
    }
    let mut ms = whole.parse::<u64>().ok()?.checked_mul(1000)?;
    for (digit, scale) in fraction.bytes().zip([100, 10, 1]) {
        ms = ms.checked_add(u64::from(digit - b'0') * scale)?;
    }
    u32::try_from(ms).ok()
}

fn digits(value: &str) -> bool {
    value.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_lrc;

    fn document(body: &str) -> String {
        format!(
            r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata" xmlns:itunes="http://music.apple.com/lyric-ttml-internal"><head><metadata><ttm:agent type="person" xml:id="v1"/><iTunesMetadata><songwriters><songwriter>Somebody</songwriter></songwriters><translations><translation><text for="L1">nope</text></translation></translations></iTunesMetadata></metadata><p>head text</p></head><body><div>{body}</div></body></tt>"#
        )
    }

    #[test]
    fn syllables_join_into_words_and_backing_gets_its_own_line() {
        let xml = document(concat!(
            r#"<p begin="0:21.718" end="0:25.643" itunes:key="L1" ttm:agent="v1">"#,
            r#"<span begin="0:21.718" end="0:21.972">La</span> "#,
            r#"<span begin="0:21.972" end="0:22.296">la</span><span begin="0:22.296" end="0:22.515">ing</span>"#,
            r#"<span ttm:role="x-bg"><span begin="0:23.000" end="0:23.500">(ooh</span> <span begin="0:23.500" end="0:24.000">ooh)</span></span>"#,
            r#"<span ttm:role="x-translation" xml:lang="zh">no</span>"#,
            r#"</p>"#,
        ));
        let text = to_lrc(&xml).unwrap();
        assert_eq!(
            text,
            "[00:21.71]<00:21.71>La <00:21.97>la<00:22.29>ing<00:22.51>\n[bg: <00:23.00>(ooh <00:23.50>ooh)<00:24.00>]"
        );

        let line = &parse_lrc(&text).lines[0];
        assert_eq!(line.text, "La laing");
        assert_eq!(line.words.len(), 3);
        assert_eq!(line.background.as_ref().unwrap().text, "(ooh ooh)");
    }

    #[test]
    fn text_between_and_before_spans_keeps_its_place() {
        let xml = document(concat!(
            r#"<p begin="1s" end="3s">Oh <span begin="1s" end="1.5s">yeah</span>, "#,
            r#"<span begin="2s" end="3s">la</span></p>"#,
        ));
        let text = to_lrc(&xml).unwrap();
        assert_eq!(parse_lrc(&text).lines[0].text, "Oh yeah, la");
    }

    #[test]
    fn spaces_around_untimed_text_and_between_backing_spans_survive() {
        let xml = document(concat!(
            r#"<p begin="1s" end="4s"><span begin="1s" end="1.5s">yeah</span> oh "#,
            r#"<span begin="2s" end="2.5s">la</span>"#,
            r#"<span ttm:role="x-bg"><span begin="2s" end="2.5s">(ooh)</span></span> "#,
            r#"<span ttm:role="x-bg"><span begin="3s" end="3.5s">(yeah)</span></span>"#,
            r#"one<br/>two</p>"#,
        ));
        let line = &parse_lrc(&to_lrc(&xml).unwrap()).lines[0];
        assert_eq!(line.text, "yeah oh la one two");
        assert_eq!(line.background.as_ref().unwrap().text, "(ooh) (yeah)");

        let untimed = document(concat!(
            r#"<p begin="1s">la<span ttm:role="x-bg">(ooh)</span>"#,
            r#"<span ttm:role="x-bg">(yeah)</span></p>"#,
        ));
        assert_eq!(
            to_lrc(&untimed).unwrap(),
            "[00:01.00]la\n[bg: (ooh) (yeah)]"
        );
    }

    #[test]
    fn line_timed_and_untimed_paragraphs() {
        let timed = document(r#"<p begin="1:02:03.4" end="1:02:05">Whole line here</p>"#);
        assert_eq!(to_lrc(&timed).unwrap(), "[62:03.40]Whole line here");

        let untimed_spans = document(
            r#"<p begin="10s"><span>Whole</span> <span>line</span><span ttm:role="x-bg">(echo)</span></p>"#,
        );
        assert_eq!(
            to_lrc(&untimed_spans).unwrap(),
            "[00:10.00]Whole line\n[bg: (echo)]"
        );

        let untimed = document("<p>First</p><p>Second</p>");
        assert_eq!(to_lrc(&untimed).unwrap(), "First\nSecond");
    }

    #[test]
    fn a_line_sung_only_in_the_background_keeps_its_place() {
        let xml = document(concat!(
            r#"<p begin="5.5s" end="7s">"#,
            r#"<span ttm:role="x-bg"><span begin="5.5s" end="7s">(hey)</span></span>"#,
            r#"</p>"#,
        ));
        let text = to_lrc(&xml).unwrap();
        assert_eq!(text, "[00:05.50]\n[bg: <00:05.50>(hey)<00:07.00>]");
        assert_eq!(parse_lrc(&text).lines[0].text, "(hey)");
    }

    #[test]
    fn garbage_is_none_and_an_empty_body_is_empty() {
        assert_eq!(to_lrc("not xml"), None);
        assert_eq!(to_lrc("<tt><head/></tt>"), None);
        assert_eq!(
            to_lrc("<html><body><p>Checking your browser</p></body></html>"),
            None
        );
        assert_eq!(to_lrc(&document("")).as_deref(), Some(""));
    }

    #[test]
    fn clock_values() {
        assert_eq!(clock("0:21.718"), Some(21_718));
        assert_eq!(clock("3:07.5"), Some(187_500));
        assert_eq!(clock("1:00:00.000"), Some(3_600_000));
        assert_eq!(clock("12.25"), Some(12_250));
        assert_eq!(clock("12.25s"), Some(12_250));
        assert_eq!(clock("1500ms"), Some(1_500));
        assert_eq!(clock("x:10"), None);
        assert_eq!(clock("1:2:3:4"), None);
        assert_eq!(clock(""), None);
        assert_eq!(clock("99999999999999999:00"), None);
        assert_eq!(clock("18446744073709552"), None);
        assert_eq!(clock("18446744073709551.999"), None);
        assert_eq!(clock("5000000ms"), Some(5_000_000));
        assert_eq!(clock("12.5ms"), Some(12));
    }
}
