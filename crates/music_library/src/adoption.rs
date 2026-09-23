use std::collections::{HashMap, HashSet};

const PATH_TOLERANCE_MS: i64 = 2_000;
const TAG_TOLERANCE_MS: i64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Path,
    Tags,
    Title,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Path => "path",
            Tier::Tags => "tags",
            Tier::Title => "title",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival {
    pub rel_path: Option<String>,
    pub start_offset_ms: i64,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orphan {
    pub item_id: i64,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub locations: Vec<(String, i64)>,
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
        (Tier::Path, Some(gap)) => gap <= PATH_TOLERANCE_MS,
        (Tier::Tags, Some(gap)) | (Tier::Title, Some(gap)) => gap <= TAG_TOLERANCE_MS,
        (Tier::Path, None) | (Tier::Tags, None) => true,
        (Tier::Title, None) => false,
    }
}

pub fn match_arrivals(arrivals: &[Arrival], orphans: &[Orphan]) -> Vec<Option<(i64, Tier)>> {
    let mut by_path: HashMap<(&str, i64), Vec<usize>> = HashMap::new();
    let mut by_tags: HashMap<(String, String, String), Vec<usize>> = HashMap::new();
    let mut by_title: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (ix, orphan) in orphans.iter().enumerate() {
        for (rel_path, offset) in &orphan.locations {
            by_path
                .entry((rel_path.as_str(), *offset))
                .or_default()
                .push(ix);
        }
        if let Some(key) = tags_key(&orphan.artist, &orphan.title, orphan.album.as_deref()) {
            by_tags.entry(key).or_default().push(ix);
        }
        if let Some(key) = title_key(&orphan.artist, &orphan.title) {
            by_title.entry(key).or_default().push(ix);
        }
    }

    let mut assigned: Vec<Option<(i64, Tier)>> = vec![None; arrivals.len()];
    let mut taken: HashSet<usize> = HashSet::new();
    for tier in [Tier::Path, Tier::Tags, Tier::Title] {
        for (arrival_ix, arrival) in arrivals.iter().enumerate() {
            if assigned[arrival_ix].is_some() {
                continue;
            }
            let candidates = match tier {
                Tier::Path => arrival
                    .rel_path
                    .as_deref()
                    .and_then(|rel| by_path.get(&(rel, arrival.start_offset_ms))),
                Tier::Tags => tags_key(&arrival.artist, &arrival.title, arrival.album.as_deref())
                    .and_then(|key| by_tags.get(&key)),
                Tier::Title => {
                    title_key(&arrival.artist, &arrival.title).and_then(|key| by_title.get(&key))
                }
            };
            let Some(candidates) = candidates else {
                continue;
            };
            let best = candidates
                .iter()
                .copied()
                .filter(|ix| !taken.contains(ix))
                .map(|ix| {
                    let gap = duration_gap(arrival.duration_ms, orphans[ix].duration_ms);
                    (ix, gap)
                })
                .filter(|(_, gap)| passes(tier, *gap))
                .min_by_key(|(ix, gap)| (gap.unwrap_or(i64::MAX), orphans[*ix].item_id));
            if let Some((ix, _)) = best {
                taken.insert(ix);
                assigned[arrival_ix] = Some((orphans[ix].item_id, tier));
            }
        }
    }
    assigned
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrival(rel_path: &str, artist: &str, title: &str, duration_ms: Option<i64>) -> Arrival {
        Arrival {
            rel_path: Some(rel_path.into()),
            start_offset_ms: 0,
            title: title.into(),
            artist: artist.into(),
            album: Some("Album".into()),
            duration_ms,
        }
    }

    fn orphan(
        item_id: i64,
        rel_path: &str,
        artist: &str,
        title: &str,
        duration_ms: Option<i64>,
    ) -> Orphan {
        Orphan {
            item_id,
            title: title.into(),
            artist: artist.into(),
            album: Some("Album".into()),
            duration_ms,
            locations: vec![(rel_path.into(), 0)],
        }
    }

    #[test]
    fn normalization_ignores_case_and_spacing() {
        assert_eq!(normalize_tag("  Boards   of Canada "), "boards of canada");
    }

    #[test]
    fn the_same_relative_path_wins_for_untagged_files() {
        let got = match_arrivals(
            &[arrival("a/01.flac", "", "01", Some(60_000))],
            &[orphan(7, "a/01.flac", "", "01", Some(60_500))],
        );
        assert_eq!(got, vec![Some((7, Tier::Path))]);
    }

    #[test]
    fn untagged_files_never_match_by_title_alone() {
        let got = match_arrivals(
            &[arrival("b/01.flac", "", "01", Some(60_000))],
            &[orphan(7, "a/01.flac", "", "01", Some(60_000))],
        );
        assert_eq!(got, vec![None]);
    }

    #[test]
    fn tags_match_across_a_move_and_pick_the_closest_duration() {
        let got = match_arrivals(
            &[arrival("new/song.flac", "Artist", "Song", Some(200_000))],
            &[
                orphan(3, "old/song.flac", "artist", "song", Some(204_000)),
                orphan(9, "older/song.flac", "ARTIST", "Song", Some(200_500)),
            ],
        );
        assert_eq!(got, vec![Some((9, Tier::Tags))]);
    }

    #[test]
    fn an_orphan_is_adopted_only_once() {
        let got = match_arrivals(
            &[
                arrival("x/song.flac", "Artist", "Song", Some(200_000)),
                arrival("y/song.flac", "Artist", "Song", Some(200_000)),
            ],
            &[orphan(3, "old/song.flac", "Artist", "Song", Some(200_000))],
        );
        assert_eq!(got, vec![Some((3, Tier::Tags)), None]);
    }

    #[test]
    fn a_stronger_tier_claims_its_orphan_before_a_weaker_one() {
        let mut retitled_album = arrival("z/song.flac", "Artist", "Song", Some(200_000));
        retitled_album.album = Some("Other".into());
        let got = match_arrivals(
            &[
                retitled_album,
                arrival("w/song.flac", "Artist", "Song", Some(200_000)),
            ],
            &[orphan(3, "old/song.flac", "Artist", "Song", Some(200_000))],
        );
        assert_eq!(got, vec![None, Some((3, Tier::Tags))]);
    }

    #[test]
    fn the_title_tier_needs_a_close_known_duration() {
        let mut elsewhere = arrival("z/song.flac", "Artist", "Song", Some(200_000));
        elsewhere.album = Some("Live".into());
        let mut unknown = elsewhere.clone();
        unknown.duration_ms = None;
        let far = orphan(3, "old/song.flac", "Artist", "Song", Some(260_000));
        assert_eq!(
            match_arrivals(std::slice::from_ref(&elsewhere), &[far]),
            vec![None]
        );
        let near = orphan(4, "old/song.flac", "Artist", "Song", Some(203_000));
        assert_eq!(
            match_arrivals(
                std::slice::from_ref(&elsewhere),
                std::slice::from_ref(&near)
            ),
            vec![Some((4, Tier::Title))]
        );
        assert_eq!(match_arrivals(&[unknown], &[near]), vec![None]);
    }
}
