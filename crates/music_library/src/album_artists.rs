use std::collections::HashSet;

pub struct AlbumTrackArtists {
    pub explicit: Vec<i64>,
    pub artists: Vec<(i64, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedAlbumArtists {
    pub artist_ids: Vec<i64>,
    pub known: bool,
}

const JOIN_CHARS: [char; 7] = ['/', '&', ',', ';', '+', '(', '['];
const JOIN_WORDS: [&str; 8] = [
    "feat",
    "ft",
    "featuring",
    "with",
    "vs",
    "versus",
    "and",
    "x",
];

fn is_join_marker(rest: &str) -> bool {
    let trimmed = rest.trim_start();
    let spaced = trimmed.len() < rest.len();
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if JOIN_CHARS.contains(&first) {
        return true;
    }
    if first == '-' {
        return spaced;
    }
    let word_end = trimmed
        .find(|c: char| !c.is_alphanumeric())
        .unwrap_or(trimmed.len());
    let word = trimmed[..word_end].to_lowercase();
    word_end < trimmed.len() && JOIN_WORDS.contains(&word.as_str())
}

fn covers(primary: &str, other: &str) -> bool {
    let (primary, other) = (primary.to_lowercase(), other.to_lowercase());
    other == primary || (other.starts_with(&primary) && is_join_marker(&other[primary.len()..]))
}

fn ids(artists: &[(i64, String)]) -> Vec<i64> {
    artists.iter().map(|(id, _)| *id).collect()
}

fn same_credits(artists: &[(i64, String)], ids: &[i64]) -> bool {
    artists.len() == ids.len() && artists.iter().zip(ids).all(|((id, _), want)| id == want)
}

pub fn derive_album_artists(tracks: &[AlbumTrackArtists]) -> DerivedAlbumArtists {
    if let Some(tagged) = tracks.iter().find(|t| !t.explicit.is_empty()) {
        return DerivedAlbumArtists {
            artist_ids: tagged.explicit.clone(),
            known: true,
        };
    }
    let voting: Vec<&AlbumTrackArtists> = tracks.iter().filter(|t| !t.artists.is_empty()).collect();
    let Some(first) = voting.first() else {
        return DerivedAlbumArtists {
            artist_ids: Vec::new(),
            known: false,
        };
    };
    let first_ids = ids(&first.artists);
    if voting.iter().all(|t| same_credits(&t.artists, &first_ids)) {
        return DerivedAlbumArtists {
            artist_ids: first_ids,
            known: true,
        };
    }
    let primaries: Vec<&(i64, String)> = voting.iter().map(|t| &t.artists[0]).collect();
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    for (id, name) in &primaries {
        if !seen.insert(*id) {
            continue;
        }
        if primaries.iter().all(|(_, other)| covers(name, other)) {
            candidates.push(*id);
        }
    }
    if let [only] = candidates[..] {
        return DerivedAlbumArtists {
            artist_ids: vec![only],
            known: true,
        };
    }
    DerivedAlbumArtists {
        artist_ids: first_ids,
        known: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(artists: &[(i64, &str)]) -> AlbumTrackArtists {
        AlbumTrackArtists {
            explicit: Vec::new(),
            artists: artists.iter().map(|(id, n)| (*id, n.to_string())).collect(),
        }
    }

    fn tagged(explicit: &[i64], artists: &[(i64, &str)]) -> AlbumTrackArtists {
        AlbumTrackArtists {
            explicit: explicit.to_vec(),
            ..track(artists)
        }
    }

    fn derived(ids: &[i64], known: bool) -> DerivedAlbumArtists {
        DerivedAlbumArtists {
            artist_ids: ids.to_vec(),
            known,
        }
    }

    #[test]
    fn the_first_track_carrying_a_tag_names_the_album() {
        let tracks = [
            track(&[(1, "Opener")]),
            tagged(&[9], &[(2, "Guest")]),
            tagged(&[8], &[(2, "Guest")]),
        ];
        assert_eq!(derive_album_artists(&tracks), derived(&[9], true));
    }

    #[test]
    fn a_shared_primary_with_guest_suffixes_is_the_album_artist() {
        let tracks = [
            track(&[(1, "Limp Bizkit")]),
            track(&[(2, "Limp Bizkit Feat. Method Man")]),
            track(&[]),
            track(&[(3, "Limp Bizkit Feat. Xzibit")]),
            track(&[(1, "Limp Bizkit")]),
        ];
        assert_eq!(derive_album_artists(&tracks), derived(&[1], true));
    }

    #[test]
    fn a_slash_in_the_name_does_not_split_it() {
        let tracks = [track(&[(1, "AC/DC")]), track(&[(2, "AC/DC & Friends")])];
        assert_eq!(derive_album_artists(&tracks), derived(&[1], true));
        let no_standalone = [
            track(&[(2, "AC/DC & Friends")]),
            track(&[(3, "AC/DC feat. Someone")]),
        ];
        assert_eq!(derive_album_artists(&no_standalone), derived(&[2], false));
    }

    #[test]
    fn a_word_prefix_is_not_a_shared_artist() {
        let tracks = [track(&[(1, "Sonic Youth")]), track(&[(2, "Sonic Boom")])];
        assert_eq!(derive_album_artists(&tracks), derived(&[1], false));
        let tracks = [track(&[(1, "Limp")]), track(&[(2, "Limp Bizkit")])];
        assert_eq!(derive_album_artists(&tracks), derived(&[1], false));
    }

    #[test]
    fn two_artists_sharing_a_first_word_stay_apart() {
        for pair in [
            [(1, "Queen"), (2, "Queen Latifah")],
            [(1, "Pink"), (2, "Pink Floyd")],
            [(1, "The Who"), (2, "The Whitest Boy Alive")],
        ] {
            let tracks = [track(&pair[..1]), track(&pair[1..])];
            assert_eq!(
                derive_album_artists(&tracks),
                derived(&[1], false),
                "{pair:?}"
            );
        }
    }

    #[test]
    fn a_hyphen_inside_a_name_is_not_a_guest_marker() {
        for pair in [
            [(1, "Jay"), (2, "Jay-Z")],
            [(1, "Wu"), (2, "Wu-Tang Clan")],
            [(1, "T"), (2, "T-Pain")],
        ] {
            let tracks = [track(&pair[..1]), track(&pair[1..])];
            assert_eq!(
                derive_album_artists(&tracks),
                derived(&[1], false),
                "{pair:?}"
            );
        }
        let spaced = [track(&[(1, "Band")]), track(&[(2, "Band - Guest")])];
        assert_eq!(derive_album_artists(&spaced), derived(&[1], true));
    }

    #[test]
    fn a_guest_marker_is_matched_whatever_the_casing() {
        let tracks = [
            track(&[(1, "band")]),
            track(&[(2, "Band FEAT. Guest")]),
            track(&[(3, "BAND with Another")]),
        ];
        assert_eq!(derive_album_artists(&tracks), derived(&[1], true));
    }

    #[test]
    fn two_standalone_artists_make_a_compilation() {
        let tracks = [
            track(&[(1, "Two Feathers")]),
            track(&[(2, "Mikael Stanne")]),
            track(&[(3, "Two Feathers/Mikael Stanne")]),
        ];
        assert_eq!(derive_album_artists(&tracks), derived(&[1], false));
    }

    #[test]
    fn identical_multi_value_credits_are_kept_whole() {
        let tracks = [
            track(&[(1, "Simon"), (2, "Garfunkel")]),
            track(&[(1, "Simon"), (2, "Garfunkel")]),
        ];
        assert_eq!(derive_album_artists(&tracks), derived(&[1, 2], true));
        let with_guest = [track(&[(1, "Band")]), track(&[(1, "Band"), (3, "Guest")])];
        assert_eq!(derive_album_artists(&with_guest), derived(&[1], true));
    }

    #[test]
    fn no_artists_at_all_yields_nothing() {
        assert_eq!(
            derive_album_artists(&[track(&[]), track(&[])]),
            derived(&[], false)
        );
        assert_eq!(derive_album_artists(&[]), derived(&[], false));
    }
}
