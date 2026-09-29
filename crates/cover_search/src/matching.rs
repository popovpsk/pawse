use crate::candidate::Candidate;

const STORE_SUFFIXES: &[&str] = &[" - single", " - ep"];
const LEADING_ARTICLE: &str = "the";
const EDITION_WORDS: &[&str] = &[
    "deluxe",
    "edition",
    "remaster",
    "expanded",
    "anniversary",
    "bonus",
    "explicit",
    "clean",
    "mono",
    "stereo",
    "reissue",
];
const ARTIST_SEPARATORS: &[&str] = &[
    " & ",
    ", ",
    "; ",
    " / ",
    " and ",
    " x ",
    " feat. ",
    " feat ",
    " ft. ",
    " featuring ",
    " with ",
];

pub fn normalize(text: &str) -> String {
    let lower = text.to_lowercase();
    let unbracketed = drop_brackets(&lower);
    let mut trimmed = unbracketed.trim();
    for suffix in STORE_SUFFIXES {
        if let Some(stripped) = trimmed.strip_suffix(suffix) {
            trimmed = stripped.trim_end();
        }
    }
    let words: Vec<String> = trimmed
        .replace('&', " and ")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect();
    let words = match words.split_first() {
        Some((first, rest)) if first == LEADING_ARTICLE && !rest.is_empty() => rest,
        _ => &words[..],
    };
    let key = words.concat();
    if key.is_empty() {
        lower.trim().to_string()
    } else {
        key
    }
}

fn drop_brackets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut inner = String::new();
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '(' | '[' | '{' => {
                depth += 1;
                if depth > 1 {
                    inner.push(c);
                }
            }
            ')' | ']' | '}' if depth > 0 => {
                depth -= 1;
                if depth > 0 {
                    inner.push(c);
                } else {
                    if !EDITION_WORDS.iter().any(|word| inner.contains(word)) {
                        out.push(' ');
                        out.push_str(&inner);
                        out.push(' ');
                    }
                    inner.clear();
                }
            }
            _ if depth == 0 => out.push(c),
            _ => inner.push(c),
        }
    }
    out.push_str(&inner);
    out
}

fn artist_keys(artist: &str) -> Vec<String> {
    let lower = artist.to_lowercase();
    let mut parts = vec![lower.clone()];
    for separator in ARTIST_SEPARATORS {
        parts = parts
            .iter()
            .flat_map(|part| part.split(separator))
            .map(str::to_string)
            .collect();
    }
    let mut keys = vec![normalize(artist)];
    keys.extend(
        parts
            .iter()
            .map(|part| normalize(part))
            .filter(|key| !key.is_empty()),
    );
    keys
}

pub fn is_exact(artist: &str, album: &str, candidate: &Candidate) -> bool {
    let ours = normalize(artist);
    normalize(album) == normalize(&candidate.album)
        && artist_keys(&candidate.artist).contains(&ours)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::case_and_punctuation("OK Computer", "ok computer!")]
    #[case::edition_in_brackets("Dummy", "Dummy (Deluxe Edition)")]
    #[case::remaster_in_square_brackets("Blue Train", "Blue Train [Remastered]")]
    #[case::anniversary_in_brackets("Nevermind", "Nevermind (30th Anniversary Super Deluxe)")]
    #[case::brackets_kept_when_they_carry_the_name(
        "(What's the Story) Morning Glory?",
        "What's the Story Morning Glory"
    )]
    #[case::itunes_single_suffix("Teardrop", "Teardrop - Single")]
    #[case::itunes_ep_suffix("Frail", "Frail - EP")]
    #[case::ampersand("Simon & Garfunkel", "Simon and Garfunkel")]
    #[case::leading_article("The Beatles", "Beatles")]
    #[case::typographic_apostrophe("Don’t Stop", "Dont Stop")]
    #[case::cyrillic("Кино", "КИНО")]
    fn same_after_normalizing(#[case] a: &str, #[case] b: &str) {
        assert_eq!(normalize(a), normalize(b));
    }

    #[rstest]
    #[case::different_album("Dummy", "Third")]
    #[case::volume_in_brackets("Greatest Hits (Vol. 1)", "Greatest Hits (Vol. 2)")]
    #[case::disc_in_brackets("Live (Disc 1)", "Live (Disc 2)")]
    #[case::named_part_in_brackets("Untitled (Black Album)", "Untitled (White Album)")]
    #[case::taylors_version("Fearless (Taylor's Version)", "Fearless")]
    #[case::the_is_not_the_whole_name("The", "")]
    #[case::number_matters("Vol. 1", "Vol. 2")]
    fn different_after_normalizing(#[case] a: &str, #[case] b: &str) {
        assert_ne!(normalize(a), normalize(b));
    }

    #[test]
    fn all_punctuation_title_still_has_a_key() {
        assert_eq!(normalize("!!!"), "!!!");
        assert_eq!(normalize("(…)"), "(…)");
    }

    #[test]
    fn exact_needs_both_artist_and_album() {
        let c = Candidate::itunes("Portishead".into(), "Dummy".into(), "u".into());
        assert!(is_exact("portishead", "Dummy (Remastered)", &c));
        assert!(!is_exact("Portishead", "Third", &c));
        assert!(!is_exact("Beth Gibbons", "Dummy", &c));
    }

    #[test]
    fn first_album_artist_matches_a_joint_credit() {
        let c = Candidate::itunes(
            "JAY-Z & Kanye West".into(),
            "Watch the Throne".into(),
            "u".into(),
        );
        assert!(is_exact("JAY-Z", "Watch the Throne", &c));
        assert!(is_exact("Kanye West", "Watch the Throne", &c));
        assert!(!is_exact("Kanye", "Watch the Throne", &c));
    }

    #[test]
    fn a_duo_name_still_matches_as_a_whole() {
        let c = Candidate::itunes("Simon & Garfunkel".into(), "Bookends".into(), "u".into());
        assert!(is_exact("Simon and Garfunkel", "Bookends", &c));
    }
}
