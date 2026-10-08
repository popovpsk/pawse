use std::collections::{HashMap, HashSet};

use music_library::{Track, normalize_tag};

use super::SimilarTracks;

pub const POOL: usize = 200;
pub const LENGTH: usize = 30;
pub const MIN_DURATION_MS: i64 = 60_000;
pub const MAX_PER_ARTIST: usize = 2;

impl SimilarTracks {
    pub fn radio(&self, seed_id: i64) -> music_library::Result<Vec<Track>> {
        let repo = self.shared.repo.as_ref();
        let Some(seed) = repo.track(seed_id)? else {
            return Ok(Vec::new());
        };
        let nearest = self.shared.neighbors.nearest(repo, seed_id, POOL)?;
        let mut tracks = HashMap::with_capacity(nearest.len());
        for (id, _) in &nearest {
            if let Some(track) = repo.track(*id)? {
                tracks.insert(*id, track);
            }
        }
        let mut ids: Vec<i64> = nearest.iter().map(|(id, _)| *id).collect();
        ids.push(seed_id);
        let artists = repo.track_artists_map(&ids)?;
        Ok(pick(seed, &nearest, tracks, &artists))
    }
}

pub fn pick(
    seed: Track,
    nearest: &[(i64, f32)],
    mut tracks: HashMap<i64, Track>,
    artists: &HashMap<i64, Vec<String>>,
) -> Vec<Track> {
    let artist_of = |id: i64| {
        artists
            .get(&id)
            .and_then(|names| names.first())
            .map(|name| normalize_tag(name))
            .unwrap_or_default()
    };
    let mut per_artist: HashMap<String, usize> = HashMap::new();
    let mut heard: HashSet<(String, String)> = HashSet::new();
    let seed_artist = artist_of(seed.id);
    if !seed_artist.is_empty() {
        heard.insert((seed_artist.clone(), normalize_tag(&seed.title)));
        per_artist.insert(seed_artist, 1);
    }
    let mut queue = vec![seed];
    for (id, _) in nearest {
        if queue.len() > LENGTH {
            break;
        }
        let Some(track) = tracks.remove(id) else {
            continue;
        };
        if !track.available || track.duration_ms.is_some_and(|ms| ms < MIN_DURATION_MS) {
            continue;
        }
        let artist = artist_of(*id);
        if !artist.is_empty() {
            let count = per_artist.entry(artist.clone()).or_default();
            if *count >= MAX_PER_ARTIST || !heard.insert((artist, normalize_tag(&track.title))) {
                continue;
            }
            *count += 1;
        }
        queue.push(track);
    }
    queue
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use audio_embedding::EMBEDDING_VERSION;
    use music_library::LibraryRepository;

    use super::*;
    use crate::similar_tracks::neighbors::tests::{add_track, library, vector};
    use crate::similar_tracks::{Neighbors, Shared};

    fn track(id: i64, title: &str, duration_ms: Option<i64>, available: bool) -> Track {
        Track {
            id,
            path: format!("/m/{id}.flac"),
            title: title.into(),
            album_id: None,
            track_number: None,
            disc_number: 1,
            duration_ms,
            year: None,
            cover_art_id: None,
            start_offset_ms: 0,
            liked: false,
            bitrate: None,
            is_cue: false,
            available,
        }
    }

    fn ids(queue: &[Track]) -> Vec<i64> {
        queue.iter().map(|t| t.id).collect()
    }

    fn run(seed: Track, candidates: Vec<(Track, &str)>, seed_artist: &str) -> Vec<Track> {
        let nearest: Vec<(i64, f32)> = candidates
            .iter()
            .enumerate()
            .map(|(rank, (t, _))| (t.id, 1.0 - rank as f32 / 1000.0))
            .collect();
        let mut artists: HashMap<i64, Vec<String>> = candidates
            .iter()
            .filter(|(_, a)| !a.is_empty())
            .map(|(t, a)| (t.id, vec![a.to_string()]))
            .collect();
        artists.insert(seed.id, vec![seed_artist.to_string()]);
        let tracks = candidates.into_iter().map(|(t, _)| (t.id, t)).collect();
        pick(seed, &nearest, tracks, &artists)
    }

    #[test]
    fn the_queue_starts_with_the_seed_and_keeps_similarity_order() {
        let queue = run(
            track(1, "seed", Some(200_000), true),
            vec![
                (track(2, "b", Some(200_000), true), "B"),
                (track(3, "c", Some(200_000), true), "C"),
            ],
            "A",
        );
        assert_eq!(ids(&queue), vec![1, 2, 3]);
    }

    #[test]
    fn skits_unavailable_tracks_and_missing_rows_are_left_out() {
        let queue = run(
            track(1, "seed", Some(200_000), true),
            vec![
                (track(2, "intro", Some(59_999), true), "B"),
                (track(3, "gone", Some(200_000), false), "C"),
                (track(4, "unknown length", None, true), "D"),
                (track(5, "song", Some(60_000), true), "E"),
            ],
            "A",
        );
        assert_eq!(ids(&queue), vec![1, 4, 5]);
    }

    #[test]
    fn an_artist_gets_two_tracks_counting_the_seed() {
        let queue = run(
            track(1, "seed", Some(200_000), true),
            vec![
                (track(2, "a2", Some(200_000), true), "a"),
                (track(3, "a3", Some(200_000), true), "A "),
                (track(4, "b1", Some(200_000), true), "B"),
                (track(5, "b2", Some(200_000), true), "B"),
                (track(6, "b3", Some(200_000), true), "b"),
            ],
            "A",
        );
        assert_eq!(ids(&queue), vec![1, 2, 4, 5]);
    }

    #[test]
    fn the_same_song_on_another_release_is_played_once() {
        let queue = run(
            track(1, "Song", Some(200_000), true),
            vec![
                (track(2, "song ", Some(200_000), true), "A"),
                (track(3, "Other", Some(200_000), true), "B"),
                (track(4, "Other", Some(210_000), true), "B"),
            ],
            "A",
        );
        assert_eq!(ids(&queue), vec![1, 3]);
    }

    #[test]
    fn untagged_tracks_have_no_artist_cap() {
        let queue = run(
            track(1, "seed", Some(200_000), true),
            vec![
                (track(2, "01", Some(200_000), true), ""),
                (track(3, "01", Some(200_000), true), ""),
                (track(4, "02", Some(200_000), true), ""),
            ],
            "",
        );
        assert_eq!(ids(&queue), vec![1, 2, 3, 4]);
    }

    #[test]
    fn the_queue_stops_at_thirty_after_the_seed() {
        let candidates = (2..100)
            .map(|id| (track(id, &format!("t{id}"), Some(200_000), true), ""))
            .collect();
        let queue = run(track(1, "seed", Some(200_000), true), candidates, "");
        assert_eq!(queue.len(), LENGTH + 1);
        assert_eq!(queue.last().unwrap().id, LENGTH as i64 + 1);
    }

    #[test]
    fn radio_reads_the_library_end_to_end() {
        let (_dir, lib) = library();
        let seed = add_track(&lib, "seed", "A", 200_000);
        let twin = add_track(&lib, "twin", "B", 200_000);
        let skit = add_track(&lib, "skit", "C", 20_000);
        let unrelated = add_track(&lib, "unrelated", "D", 200_000);
        let unanalysed = add_track(&lib, "unanalysed", "E", 200_000);
        lib.save_embeddings(
            EMBEDDING_VERSION,
            &[
                (seed, vector(&[1.0, 0.0])),
                (twin, vector(&[1.0, 0.1])),
                (skit, vector(&[1.0, 0.05])),
                (unrelated, vector(&[-1.0, 0.0])),
            ],
        )
        .unwrap();
        let repo: Arc<dyn LibraryRepository> = Arc::new(lib);
        let similar = SimilarTracks {
            shared: Arc::new(Shared {
                repo,
                neighbors: Neighbors::default(),
                stop: AtomicBool::new(false),
            }),
        };

        assert_eq!(
            ids(&similar.radio(seed).unwrap()),
            vec![seed, twin, unrelated]
        );
        assert!(similar.radio(unanalysed).unwrap().len() == 1);
        assert!(similar.radio(12345).unwrap().is_empty());
    }
}
