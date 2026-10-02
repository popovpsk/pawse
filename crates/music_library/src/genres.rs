pub fn normalize_genres<'a>(raw: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in raw {
        for piece in value.split([',', ';', '/']) {
            let cleaned: String = piece.split_whitespace().collect::<Vec<_>>().join(" ");
            if cleaned.is_empty() || is_junk_genre(&cleaned) {
                continue;
            }
            let key = cleaned.to_lowercase();
            if !out.iter().any(|g| g.to_lowercase() == key) {
                out.push(cleaned);
            }
        }
    }
    out
}

fn is_junk_genre(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "album"
            | "unknown"
            | "unknown genre"
            | "other"
            | "various"
            | "various artists"
            | "genre"
            | "none"
            | "no genre"
            | "misc"
    ) || lower.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_genres_splits_dedups_and_filters() {
        let got = normalize_genres(
            [
                "Rock, Alternative",
                "alternative",
                " Indie  Rock ",
                "Album",
                "255",
                "Drum & Bass",
                "Progressive Rock/Metal",
            ]
            .into_iter(),
        );
        assert_eq!(
            got.iter().map(String::as_str).collect::<Vec<_>>(),
            vec![
                "Rock",
                "Alternative",
                "Indie Rock",
                "Drum & Bass",
                "Progressive Rock",
                "Metal",
            ]
        );
    }
}
