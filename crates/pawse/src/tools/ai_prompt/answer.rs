use std::collections::{HashMap, HashSet};

use music_library::TrackListing;

use super::template::UNKNOWN_ARTIST;

const DASHES: [&str; 5] = [" — ", " – ", " - ", "—", "–"];
const CUTS: [&str; 6] = [" — ", " – ", " - ", ": ", "—", "–"];
const FENCES: [&str; 2] = ["```", "~~~"];
const MISSING_LINE_MAX_CHARS: usize = 100;

#[derive(Debug, Default)]
pub struct TrackIndex {
    exact: HashMap<String, i64>,
    loose: HashMap<String, i64>,
}

impl TrackIndex {
    pub fn new(listings: &[TrackListing]) -> Self {
        let mut index = Self::default();
        for listing in listings {
            let artists = std::iter::once(listing.artist.as_str())
                .chain(listing.album_artist.as_deref())
                .map(|artist| {
                    if artist.trim().is_empty() {
                        UNKNOWN_ARTIST
                    } else {
                        artist
                    }
                });
            for artist in artists {
                index
                    .exact
                    .entry(exact_key(artist, &listing.title))
                    .or_insert(listing.track_id);
                if let Some(key) = loose_key(artist, &listing.title) {
                    index.loose.entry(key).or_insert(listing.track_id);
                }
            }
        }
        index
    }

    fn find(&self, artist: &str, title: &str) -> Option<i64> {
        self.exact.get(&exact_key(artist, title)).copied()
    }

    fn find_loose(&self, artist: &str, title: &str) -> Option<i64> {
        self.loose.get(&loose_key(artist, title)?).copied()
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedAnswer {
    pub track_ids: Vec<i64>,
    pub missing: Vec<String>,
}

pub fn parse_answer(text: &str, index: &TrackIndex) -> ParsedAnswer {
    if let Some(lines) = fenced_lines(text) {
        let parsed = parse_lines(&lines, index, true);
        if !parsed.track_ids.is_empty() {
            return parsed;
        }
    }
    let lines: Vec<&str> = text.lines().collect();
    parse_lines(&lines, index, false)
}

fn parse_lines(lines: &[&str], index: &TrackIndex, from_block: bool) -> ParsedAnswer {
    let mut parsed = ParsedAnswer::default();
    let mut seen = HashSet::new();
    let mut missed = HashSet::new();
    for raw in lines {
        let line = clean_line(raw);
        if !line.chars().any(char::is_alphanumeric) {
            continue;
        }
        let starless = line.replace('*', "");
        let found = match_line(&line, index).or_else(|| match_line(&starless, index));
        match found {
            Some(id) => {
                if seen.insert(id) {
                    parsed.track_ids.push(id);
                }
            }
            None if (from_block || looks_like_track(&starless))
                && missed.insert(starless.clone()) =>
            {
                parsed.missing.push(starless);
            }
            None => {}
        }
    }
    parsed
}

fn fenced_lines(text: &str) -> Option<Vec<&str>> {
    let mut lines = Vec::new();
    let mut inside = false;
    let mut found = false;
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(fence) = FENCES.iter().find(|fence| trimmed.starts_with(**fence)) else {
            if inside {
                lines.push(line);
            }
            continue;
        };
        found = true;
        let inner = &trimmed[fence.len()..];
        match inner.strip_suffix(fence) {
            Some(inline) if !inline.trim().is_empty() => lines.push(inline),
            _ => inside = !inside,
        }
    }
    found.then_some(lines)
}

fn looks_like_track(line: &str) -> bool {
    line.chars().count() <= MISSING_LINE_MAX_CHARS
        && !line.ends_with(':')
        && DASHES.iter().any(|sep| line.contains(sep))
}

fn clean_line(raw: &str) -> String {
    let spaced = raw.replace(['\u{a0}', '\u{2007}', '\u{202f}', '\u{2009}'], " ");
    let mut line = spaced.trim();
    loop {
        let before = line;
        line = line.trim_start_matches('#').trim_start();
        if let Some(rest) = strip_bullet(line) {
            line = rest;
        }
        if let Some(rest) = strip_number(line) {
            line = rest;
        }
        if line == before {
            break;
        }
    }
    let line = line.replace('`', "");
    line.trim().trim_matches('*').trim().to_string()
}

fn strip_bullet(line: &str) -> Option<&str> {
    ["- ", "* ", "• ", "– ", "— ", "+ "]
        .iter()
        .find_map(|bullet| line.strip_prefix(bullet))
        .map(str::trim_start)
}

fn strip_number(line: &str) -> Option<&str> {
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    let rest = line[digits..]
        .strip_prefix('.')
        .or_else(|| line[digits..].strip_prefix(')'))?;
    if rest.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(rest.trim_start())
}

fn match_line(line: &str, index: &TrackIndex) -> Option<i64> {
    let cuts = cuts(line);
    let mut pairs = Vec::new();
    for (i, &(start, end)) in cuts.iter().enumerate() {
        let stops = cuts[i + 1..]
            .iter()
            .map(|&(next, _)| next)
            .chain(std::iter::once(line.len()))
            .rev();
        pairs.extend(stops.map(|stop| (&line[..start], &line[end..stop])));
    }
    for lookup in [TrackIndex::find, TrackIndex::find_loose] {
        for &(left, right) in &pairs {
            if let Some(id) = lookup(index, left, right).or_else(|| lookup(index, right, left)) {
                return Some(id);
            }
        }
    }
    None
}

fn cuts(line: &str) -> Vec<(usize, usize)> {
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    for sep in CUTS {
        for (start, _) in line.match_indices(sep) {
            let end = start + sep.len();
            if !cuts.iter().any(|&(s, e)| start < e && s < end) {
                cuts.push((start, end));
            }
        }
    }
    cuts.sort_unstable();
    cuts
}

fn fold(text: &str) -> String {
    let folded: String = text
        .chars()
        .map(|c| match c {
            '’' | '‘' | '´' | '`' => '\'',
            '“' | '”' | '„' | '«' | '»' => '"',
            _ => c,
        })
        .collect();
    let folded = folded.replace('…', "...");
    let trimmed = folded.trim().trim_matches('"').trim();
    music_library::normalize_tag(trimmed)
}

fn exact_key(artist: &str, title: &str) -> String {
    format!("{}\u{1}{}", fold(artist), fold(title))
}

fn strip_brackets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn alnum_words(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn loose_key(artist: &str, title: &str) -> Option<String> {
    let artist = alnum_words(artist);
    let title = alnum_words(&strip_brackets(title));
    (!artist.is_empty() && !title.is_empty()).then(|| format!("{artist}\u{1}{title}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn listing(id: i64, artist: &str, title: &str) -> TrackListing {
        TrackListing {
            track_id: id,
            title: title.into(),
            artist: artist.into(),
            album_id: None,
            album: None,
            album_artist: None,
            year: None,
            liked: false,
            plays: 0,
            last_played: None,
        }
    }

    fn index() -> TrackIndex {
        let mut guest = listing(5, "Thom Yorke", "Hearing Damage");
        guest.album_artist = Some("Various Artists".into());
        TrackIndex::new(&[
            listing(1, "Radiohead", "Airbag"),
            listing(2, "Massive Attack", "Teardrop"),
            listing(3, "Portishead", "Glory Box - 2011 Remaster"),
            listing(4, "Sigur Rós", "Hoppípolla"),
            guest,
            listing(6, "Guns N' Roses", "Don't Cry (Original)"),
            listing(7, "Portishead", "Glory Box"),
            listing(8, "", "Untitled"),
            listing(9, "Cee Lo Green", "F**k You"),
            listing(10, "Me", "Stand"),
        ])
    }

    #[rstest]
    #[case::plain("Radiohead — Airbag", 1)]
    #[case::en_dash("Radiohead – Airbag", 1)]
    #[case::hyphen("Radiohead - Airbag", 1)]
    #[case::no_spaces("Radiohead—Airbag", 1)]
    #[case::numbered("1. Radiohead — Airbag", 1)]
    #[case::paren_numbered("12) Radiohead — Airbag", 1)]
    #[case::bullet("- Radiohead — Airbag", 1)]
    #[case::bold("**Radiohead** — *Airbag*", 1)]
    #[case::bold_bullet("* **Radiohead — Airbag**", 1)]
    #[case::quoted_title("Radiohead — «Airbag»", 1)]
    #[case::smart_quotes("Radiohead — “Airbag”", 1)]
    #[case::case_and_spaces("  radiohead   —   AIRBAG ", 1)]
    #[case::reason("Radiohead — Airbag — a restless opener", 1)]
    #[case::colon_reason("Radiohead — Airbag: a restless opener", 1)]
    #[case::reversed("Airbag — Radiohead", 1)]
    #[case::number_without_space("1.Radiohead — Airbag", 1)]
    #[case::nbsp_hyphen("Radiohead\u{a0}-\u{a0}Airbag", 1)]
    #[case::unknown_artist("Unknown artist — Untitled", 8)]
    #[case::stars_in_title("Cee Lo Green — F**k You", 9)]
    #[case::stars_in_bold_title("**Cee Lo Green — F**k You**", 9)]
    #[case::title_with_dash("Portishead — Glory Box - 2011 Remaster", 3)]
    #[case::shorter_title("Portishead — Glory Box", 7)]
    #[case::diacritics("Sigur Rós — Hoppípolla", 4)]
    #[case::album_artist("Various Artists — Hearing Damage", 5)]
    #[case::track_artist("Thom Yorke — Hearing Damage", 5)]
    #[case::apostrophes("Guns N’ Roses — Don’t Cry (Original)", 6)]
    #[case::bracket_dropped("Guns N' Roses — Don't Cry", 6)]
    fn matches_one_line(#[case] line: &str, #[case] id: i64) {
        let parsed = parse_answer(line, &index());
        assert_eq!(parsed.track_ids, vec![id]);
        assert!(parsed.missing.is_empty());
    }

    #[test]
    fn prose_around_a_list_is_ignored() {
        let answer = "Here is a playlist for a rainy evening:\n\n\
            1. **Massive Attack — Teardrop** — slow and heavy start.\n\
            2. **Radiohead — Airbag** — lifts the tempo.\n\n\
            The idea: from dusk to night.";
        let parsed = parse_answer(answer, &index());
        assert_eq!(parsed.track_ids, vec![2, 1]);
        assert!(parsed.missing.is_empty());
    }

    #[test]
    fn code_block_is_the_only_source_when_present() {
        let answer = "Sure!\n\n```\nMassive Attack — Teardrop\nRadiohead — Airbag\nBurial — Archangel\n```\n\n\
            Portishead — Glory Box: closes the night.\nRadiohead — Airbag: an opener.";
        let parsed = parse_answer(answer, &index());
        assert_eq!(parsed.track_ids, vec![2, 1]);
        assert_eq!(parsed.missing, vec!["Burial — Archangel".to_string()]);
    }

    #[test]
    fn code_block_with_language_tag() {
        let answer = "```text\nRadiohead — Airbag\n```";
        assert_eq!(parse_answer(answer, &index()).track_ids, vec![1]);
    }

    #[test]
    fn duplicates_keep_the_first_position() {
        let answer = "Radiohead — Airbag\nMassive Attack — Teardrop\nradiohead - airbag";
        assert_eq!(parse_answer(answer, &index()).track_ids, vec![1, 2]);
    }

    #[test]
    fn free_text_reports_only_track_like_misses() {
        let answer = "A calm mix for you:\n\
            Burial — Archangel\n\
            Radiohead — Airbag\n\
            This playlist moves from trip-hop to post-rock — slowly, with a long fade at the end, just like you asked for in the wishes.";
        let parsed = parse_answer(answer, &index());
        assert_eq!(parsed.track_ids, vec![1]);
        assert_eq!(parsed.missing, vec!["Burial — Archangel".to_string()]);
    }

    #[test]
    fn prose_with_by_is_not_a_track() {
        let parsed = parse_answer("Stand by Me", &index());
        assert!(parsed.track_ids.is_empty());
    }

    #[test]
    fn inline_fence_line_is_read() {
        let answer = "```Radiohead — Airbag```\nMassive Attack — Teardrop";
        assert_eq!(parse_answer(answer, &index()).track_ids, vec![1]);
    }

    #[test]
    fn tilde_fence_is_a_block() {
        let answer = "Intro\n~~~\nRadiohead — Airbag\n~~~\nMassive Attack — Teardrop: why";
        assert_eq!(parse_answer(answer, &index()).track_ids, vec![1]);
    }

    #[test]
    fn empty_block_falls_back_to_the_whole_text() {
        let answer = "```\n```\nRadiohead — Airbag";
        assert_eq!(parse_answer(answer, &index()).track_ids, vec![1]);
    }

    #[test]
    fn misses_are_reported_once_and_separator_lines_skipped() {
        let answer = "```\nBurial — Archangel\n— —\nburial — archangel\nBurial — Archangel\nRadiohead — Airbag\n```";
        let parsed = parse_answer(answer, &index());
        assert_eq!(parsed.track_ids, vec![1]);
        assert_eq!(
            parsed.missing,
            vec![
                "Burial — Archangel".to_string(),
                "burial — archangel".to_string()
            ]
        );
    }

    #[test]
    fn empty_answer() {
        assert_eq!(parse_answer("", &index()), ParsedAnswer::default());
    }
}
