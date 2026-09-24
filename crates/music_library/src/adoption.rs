use std::collections::{HashMap, HashSet};

const FILE_TOLERANCE_MS: i64 = 2_000;
const TAG_TOLERANCE_MS: i64 = 5_000;

type Rank = (bool, i64, i64, usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    File,
    Tags,
    Title,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::File => "file",
            Tier::Tags => "tags",
            Tier::Title => "title",
        }
    }
}

pub const WHOLE_FILE: i64 = -1;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Descriptor {
    pub item_id: i64,
    pub title: String,
    pub artist: String,
    pub artist_aliases: Vec<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub files: Vec<(i64, i64)>,
    pub sources: Vec<i64>,
    pub live: bool,
}

pub fn normalize_tag(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn tags_key(artist: &str, title: &str, album: Option<&str>) -> Option<(String, String, String)> {
    let artist = normalize_tag(artist);
    if artist.is_empty() {
        return None;
    }
    Some((
        artist,
        normalize_tag(title),
        normalize_tag(album.unwrap_or("")),
    ))
}

fn title_key(artist: &str, title: &str) -> Option<(String, String)> {
    let artist = normalize_tag(artist);
    if artist.is_empty() {
        return None;
    }
    Some((artist, normalize_tag(title)))
}

fn duration_gap(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    Some((a? - b?).abs())
}

fn passes(tier: Tier, gap: Option<i64>) -> bool {
    match (tier, gap) {
        (Tier::File, Some(gap)) => gap <= FILE_TOLERANCE_MS,
        (Tier::Tags, Some(gap)) | (Tier::Title, Some(gap)) => gap <= TAG_TOLERANCE_MS,
        (Tier::File, None) | (Tier::Tags, None) => true,
        (Tier::Title, None) => false,
    }
}

fn shares_file(a: &Descriptor, b: &Descriptor) -> bool {
    a.files.iter().any(|file| b.files.contains(file))
}

fn shares_source(a: &Descriptor, b: &Descriptor) -> bool {
    a.sources.iter().any(|source| b.sources.contains(source))
}

pub fn match_tracks(
    arrivals: &[Descriptor],
    candidates: &[Descriptor],
) -> Vec<Option<(i64, Tier)>> {
    let mut by_file: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let mut by_tags: HashMap<(String, String, String), Vec<usize>> = HashMap::new();
    let mut by_title: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (ix, candidate) in candidates.iter().enumerate() {
        for file in &candidate.files {
            if file.0 > 0 {
                by_file.entry(*file).or_default().push(ix);
            }
        }
        if let Some(key) = tags_key(
            &candidate.artist,
            &candidate.title,
            candidate.album.as_deref(),
        ) {
            by_tags.entry(key).or_default().push(ix);
        }
        if let Some(key) = title_key(&candidate.artist, &candidate.title) {
            by_title.entry(key).or_default().push(ix);
        }
    }

    let mut assigned: Vec<Option<(i64, Tier)>> = vec![None; arrivals.len()];
    let mut taken: HashSet<usize> = HashSet::new();
    for tier in [Tier::File, Tier::Tags, Tier::Title] {
        let mut pairs: Vec<(Rank, usize, usize)> = Vec::new();
        for (arrival_ix, arrival) in arrivals.iter().enumerate() {
            if assigned[arrival_ix].is_some() {
                continue;
            }
            let artists = std::iter::once(&arrival.artist).chain(&arrival.artist_aliases);
            let mut found: Vec<usize> = match tier {
                Tier::File => arrival
                    .files
                    .iter()
                    .filter_map(|file| by_file.get(file))
                    .flatten()
                    .copied()
                    .collect(),
                Tier::Tags => artists
                    .filter_map(|artist| {
                        tags_key(artist, &arrival.title, arrival.album.as_deref())
                            .and_then(|key| by_tags.get(&key))
                    })
                    .flatten()
                    .copied()
                    .collect(),
                Tier::Title => artists
                    .filter_map(|artist| {
                        title_key(artist, &arrival.title).and_then(|key| by_title.get(&key))
                    })
                    .flatten()
                    .copied()
                    .collect(),
            };
            found.sort_unstable();
            found.dedup();
            for ix in found {
                let candidate = &candidates[ix];
                if taken.contains(&ix)
                    || candidate.item_id == arrival.item_id
                    || shares_source(arrival, candidate)
                    || (tier == Tier::Title && candidate.live)
                {
                    continue;
                }
                let gap = duration_gap(arrival.duration_ms, candidate.duration_ms);
                if !passes(tier, gap) {
                    continue;
                }
                let rank = (
                    !shares_file(arrival, candidate),
                    gap.unwrap_or(i64::MAX),
                    candidate.item_id,
                    arrival_ix,
                );
                pairs.push((rank, arrival_ix, ix));
            }
        }
        pairs.sort_unstable();
        for (_, arrival_ix, ix) in pairs {
            if assigned[arrival_ix].is_some() || taken.contains(&ix) {
                continue;
            }
            taken.insert(ix);
            assigned[arrival_ix] = Some((candidates[ix].item_id, tier));
        }
    }
    assigned
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(item_id: i64, artist: &str, title: &str, duration_ms: Option<i64>) -> Descriptor {
        Descriptor {
            item_id,
            title: title.into(),
            artist: artist.into(),
            album: Some("Album".into()),
            duration_ms,
            ..Default::default()
        }
    }

    fn arrival(artist: &str, title: &str, duration_ms: Option<i64>) -> Descriptor {
        Descriptor {
            sources: vec![1],
            ..track(0, artist, title, duration_ms)
        }
    }

    fn sized(mut descriptor: Descriptor, size: i64, offset: i64) -> Descriptor {
        descriptor.files.push((size, offset));
        descriptor
    }

    #[test]
    fn normalization_ignores_case_and_spacing() {
        assert_eq!(normalize_tag("  Boards   of Canada "), "boards of canada");
    }

    #[test]
    fn the_same_file_matches_without_any_tags() {
        let got = match_tracks(
            &[sized(arrival("", "01", Some(60_000)), 1234, 0)],
            &[sized(track(7, "", "track 01", Some(60_500)), 1234, 0)],
        );
        assert_eq!(got, vec![Some((7, Tier::File))]);
    }

    #[test]
    fn cue_tracks_of_one_image_are_told_apart_by_offset() {
        let got = match_tracks(
            &[
                sized(arrival("", "b", Some(30_000)), 900, 60_000),
                sized(arrival("", "a", Some(60_000)), 900, 0),
            ],
            &[
                sized(track(1, "", "x", Some(60_000)), 900, 0),
                sized(track(2, "", "y", Some(30_000)), 900, 60_000),
            ],
        );
        assert_eq!(got, vec![Some((2, Tier::File)), Some((1, Tier::File))]);
    }

    #[test]
    fn a_file_match_still_needs_a_close_duration() {
        let got = match_tracks(
            &[sized(arrival("", "01", Some(60_000)), 1234, 0)],
            &[sized(track(7, "", "01", Some(90_000)), 1234, 0)],
        );
        assert_eq!(got, vec![None]);
    }

    #[test]
    fn untagged_files_of_different_sizes_never_match() {
        let got = match_tracks(
            &[sized(arrival("", "01", Some(60_000)), 1, 0)],
            &[sized(track(7, "", "01", Some(60_000)), 2, 0)],
        );
        assert_eq!(got, vec![None]);
    }

    #[test]
    fn tags_prefer_the_same_file_then_the_closest_duration() {
        let got = match_tracks(
            &[sized(arrival("Artist", "Song", Some(200_000)), 50, 0)],
            &[
                track(3, "artist", "song", Some(200_000)),
                track(9, "ARTIST", "Song", Some(200_500)),
            ],
        );
        assert_eq!(got, vec![Some((3, Tier::Tags))]);
        let got = match_tracks(
            &[sized(arrival("Artist", "Song", Some(200_000)), 50, 0)],
            &[
                track(3, "artist", "song", Some(204_000)),
                sized(track(9, "ARTIST", "Song", Some(203_000)), 51, 0),
            ],
        );
        assert_eq!(got, vec![Some((9, Tier::Tags))]);
    }

    #[test]
    fn a_candidate_already_held_by_the_same_source_is_skipped() {
        let mut held = track(3, "Artist", "Song", Some(200_000));
        held.sources = vec![1];
        let mut elsewhere = track(4, "Artist", "Song", Some(200_000));
        elsewhere.sources = vec![2];
        assert_eq!(
            match_tracks(
                &[arrival("Artist", "Song", Some(200_000))],
                std::slice::from_ref(&held)
            ),
            vec![None]
        );
        assert_eq!(
            match_tracks(
                &[arrival("Artist", "Song", Some(200_000))],
                &[held, elsewhere]
            ),
            vec![Some((4, Tier::Tags))]
        );
    }

    #[test]
    fn a_candidate_is_claimed_only_once() {
        let got = match_tracks(
            &[
                arrival("Artist", "Song", Some(200_000)),
                arrival("Artist", "Song", Some(200_000)),
            ],
            &[track(3, "Artist", "Song", Some(200_000))],
        );
        assert_eq!(got, vec![Some((3, Tier::Tags)), None]);
    }

    #[test]
    fn a_stronger_tier_claims_its_candidate_before_a_weaker_one() {
        let mut retitled_album = arrival("Artist", "Song", Some(200_000));
        retitled_album.album = Some("Other".into());
        let got = match_tracks(
            &[retitled_album, arrival("Artist", "Song", Some(200_000))],
            &[track(3, "Artist", "Song", Some(200_000))],
        );
        assert_eq!(got, vec![None, Some((3, Tier::Tags))]);
    }

    #[test]
    fn an_artist_alias_matches_when_the_main_credit_does_not() {
        let mut split = arrival("Limp Bizkit", "Song", Some(200_000));
        split.artist_aliases = vec!["Limp Bizkit Feat. Method Man".into()];
        let got = match_tracks(
            &[split],
            &[track(
                5,
                "Limp Bizkit Feat. Method Man",
                "Song",
                Some(200_000),
            )],
        );
        assert_eq!(got, vec![Some((5, Tier::Tags))]);
    }

    #[test]
    fn a_live_item_is_never_claimed_on_title_alone() {
        let mut elsewhere = arrival("Artist", "Song", Some(200_000));
        elsewhere.album = Some("Live".into());
        let mut live = track(4, "Artist", "Song", Some(201_000));
        live.live = true;
        assert_eq!(
            match_tracks(
                std::slice::from_ref(&elsewhere),
                std::slice::from_ref(&live)
            ),
            vec![None]
        );
        live.album = Some("Live".into());
        assert_eq!(
            match_tracks(&[elsewhere], &[live]),
            vec![Some((4, Tier::Tags))]
        );
    }

    #[test]
    fn the_title_tier_needs_a_close_known_duration() {
        let mut elsewhere = arrival("Artist", "Song", Some(200_000));
        elsewhere.album = Some("Live".into());
        let mut unknown = elsewhere.clone();
        unknown.duration_ms = None;
        let far = track(3, "Artist", "Song", Some(260_000));
        assert_eq!(
            match_tracks(std::slice::from_ref(&elsewhere), &[far]),
            vec![None]
        );
        let near = track(4, "Artist", "Song", Some(203_000));
        assert_eq!(
            match_tracks(
                std::slice::from_ref(&elsewhere),
                std::slice::from_ref(&near)
            ),
            vec![Some((4, Tier::Title))]
        );
        assert_eq!(match_tracks(&[unknown], &[near]), vec![None]);
    }

    #[test]
    fn the_closest_arrival_wins_whatever_order_they_come_in() {
        let far = arrival("Artist", "Song", Some(204_000));
        let near = arrival("Artist", "Song", Some(200_000));
        let lost = track(3, "Artist", "Song", Some(200_000));
        assert_eq!(
            match_tracks(&[far, near], &[lost]),
            vec![None, Some((3, Tier::Tags))]
        );
    }

    #[test]
    fn an_item_never_matches_itself() {
        let mut me = track(4, "Artist", "Song", Some(200_000));
        me.sources = vec![2];
        let other = track(4, "Artist", "Song", Some(200_000));
        assert_eq!(match_tracks(&[me], &[other]), vec![None]);
    }
}
