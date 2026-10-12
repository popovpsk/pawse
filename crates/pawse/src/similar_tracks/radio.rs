use audio_embedding::similarity::Query;
use music_library::Track;
use rand::SeedableRng;
use rand::rngs::StdRng;

use super::SimilarTracks;
use super::pool::{Pool, now_secs, seeds_taken};
use super::rerank::{Familiarity, Rerank, Weights};

pub const ANY_ARTIST_POOL: usize = 100;
pub const OTHER_ARTISTS_POOL: usize = 400;
pub const LENGTH: usize = 100;

impl SimilarTracks {
    pub fn radio(
        &self,
        seeds: &[i64],
        skip: &[i64],
        familiarity: Familiarity,
        rng_seed: u64,
    ) -> music_library::Result<Option<Vec<Track>>> {
        let repo = self.shared.repo.as_ref();
        let neighbors = &self.shared.neighbors;
        let mut tracks = Vec::with_capacity(seeds.len());
        for &id in seeds {
            if let Some(track) = repo.track(id)? {
                tracks.push(track);
            }
        }
        let ids: Vec<i64> = tracks.iter().map(|track| track.id).collect();
        let vector = match ids.as_slice() {
            [] => None,
            [one] => neighbors.vector(repo, *one)?,
            many => neighbors.mean_of(repo, many)?,
        };
        let Some(vector) = vector else {
            return Ok(None);
        };
        let own = repo.same_artist_track_ids(&ids)?;
        let excluded = || skip.iter().chain(&ids).copied();
        let found = neighbors.search(
            repo,
            vec![
                Query::nearest(vector.clone(), ANY_ARTIST_POOL).excluding(excluded()),
                Query::nearest(vector, OTHER_ARTISTS_POOL)
                    .excluding(own.into_iter().chain(excluded())),
            ],
        )?;
        let taken = seeds_taken(repo, &tracks)?;
        let rerank = Rerank {
            count: LENGTH,
            familiarity,
            now: now_secs(),
            weights: Weights::default(),
        };
        Ok(Some(Pool::load(repo, found)?.pick(
            &rerank,
            taken,
            &mut StdRng::seed_from_u64(rng_seed),
        )))
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use audio_embedding::EMBEDDING_VERSION;
    use music_library::LibraryRepository;

    use super::*;
    use crate::similar_tracks::neighbors::tests::{add_track, library, vector};
    use crate::similar_tracks::{Neighbors, Shared};

    pub(crate) fn similar(repo: Arc<dyn LibraryRepository>) -> SimilarTracks {
        SimilarTracks {
            shared: Arc::new(Shared {
                repo,
                neighbors: Neighbors::default(),
                stop: AtomicBool::new(false),
                progress: Default::default(),
                saved: flume::bounded(1).0,
            }),
        }
    }

    fn ids(tracks: &[Track]) -> Vec<i64> {
        tracks.iter().map(|t| t.id).collect()
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
            &crate::similar_tracks::analyses(&[
                (seed, vector(&[1.0, 0.0])),
                (twin, vector(&[1.0, 0.1])),
                (skit, vector(&[1.0, 0.05])),
                (unrelated, vector(&[-1.0, 0.0])),
            ]),
        )
        .unwrap();
        let similar = similar(Arc::new(lib));

        assert_eq!(
            ids(&similar
                .radio(&[seed], &[], Familiarity::Any, 1)
                .unwrap()
                .unwrap()),
            vec![twin]
        );
        assert!(
            similar
                .radio(&[unanalysed], &[], Familiarity::Any, 1)
                .unwrap()
                .is_none()
        );
        assert!(
            similar
                .radio(&[12345], &[], Familiarity::Any, 1)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_radio_skips_the_tracks_it_is_told_to() {
        let (_dir, lib) = library();
        let seed = add_track(&lib, "seed", "A", 200_000);
        let same_artist = add_track(&lib, "same", "A", 200_000);
        let heard = add_track(&lib, "heard", "B", 200_000);
        let fresh = add_track(&lib, "fresh", "C", 200_000);
        let unrelated = add_track(&lib, "unrelated", "D", 200_000);
        lib.save_embeddings(
            EMBEDDING_VERSION,
            &crate::similar_tracks::analyses(&[
                (seed, vector(&[1.0, 0.0])),
                (same_artist, vector(&[1.0, 0.02])),
                (heard, vector(&[1.0, 0.05])),
                (fresh, vector(&[1.0, 0.1])),
                (unrelated, vector(&[-1.0, 0.0])),
            ]),
        )
        .unwrap();
        let similar = similar(Arc::new(lib));

        let all = ids(&similar
            .radio(&[seed], &[], Familiarity::Any, 1)
            .unwrap()
            .unwrap());
        assert!(
            [same_artist, heard, fresh]
                .iter()
                .all(|id| all.contains(id))
        );
        assert_eq!(
            ids(&similar
                .radio(&[seed], &[same_artist, heard, seed], Familiarity::Any, 1)
                .unwrap()
                .unwrap()),
            vec![fresh]
        );
    }

    #[test]
    fn a_radio_from_several_tracks_starts_from_their_mean() {
        let (_dir, lib) = library();
        let left = add_track(&lib, "left", "A", 200_000);
        let right = add_track(&lib, "right", "B", 200_000);
        let silent = add_track(&lib, "silent", "C", 200_000);
        let middle = add_track(&lib, "middle", "D", 200_000);
        let near_left = add_track(&lib, "near left", "E", 200_000);
        let near_right = add_track(&lib, "near right", "G", 200_000);
        let far = add_track(&lib, "far", "F", 200_000);
        lib.save_embeddings(
            EMBEDDING_VERSION,
            &crate::similar_tracks::analyses(&[
                (left, vector(&[1.0, 0.0, 0.0])),
                (right, vector(&[0.0, 1.0, 0.0])),
                (middle, vector(&[0.7, 0.7, 0.0])),
                (near_left, vector(&[1.0, -0.4, 0.0])),
                (near_right, vector(&[-0.4, 1.0, 0.0])),
                (far, vector(&[-1.0, -1.0, 0.0])),
            ]),
        )
        .unwrap();
        let similar = similar(Arc::new(lib));

        let from_one = ids(&similar
            .radio(&[left], &[], Familiarity::Any, 1)
            .unwrap()
            .unwrap());
        assert_eq!(from_one.first(), Some(&near_left));
        let from_last = ids(&similar
            .radio(&[right], &[], Familiarity::Any, 1)
            .unwrap()
            .unwrap());
        assert_eq!(from_last.first(), Some(&near_right));
        let from_tail = ids(&similar
            .radio(&[left, silent, right], &[], Familiarity::Any, 1)
            .unwrap()
            .unwrap());
        assert_eq!(from_tail.first(), Some(&middle));
        assert!(!from_tail.contains(&left) && !from_tail.contains(&right));
        assert!(
            similar
                .radio(&[silent, 12345], &[], Familiarity::Any, 1)
                .unwrap()
                .is_none()
        );
        assert!(
            similar
                .radio(&[], &[], Familiarity::Any, 1)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn the_status_counts_analysed_catalog_tracks() {
        let (_dir, lib) = library();
        let a = add_track(&lib, "a", "A", 200_000);
        add_track(&lib, "b", "B", 200_000);
        lib.save_embeddings(
            EMBEDDING_VERSION,
            &crate::similar_tracks::analyses(&[(a, vector(&[1.0]))]),
        )
        .unwrap();
        let status = similar(Arc::new(lib)).status().unwrap();
        assert_eq!((status.analysed, status.total), (1, 2));
        assert_eq!(status.phase, crate::similar_tracks::Phase::Idle);
        assert_eq!((status.failed, status.unreachable), (0, 0));
    }

    #[test]
    fn a_big_discography_still_leaves_room_for_other_artists() {
        let (_dir, lib) = library();
        let seed = add_track(&lib, "seed", "A", 200_000);
        let mut rows = vec![(seed, vector(&[1.0, 0.0]))];
        let mut own = Vec::new();
        for n in 0..(ANY_ARTIST_POOL + OTHER_ARTISTS_POOL) {
            let id = add_track(&lib, &format!("a{n}"), "A", 200_000);
            rows.push((id, vector(&[1.0, 0.01 * (n % 10) as f32])));
            own.push(id);
        }
        let mut others = Vec::new();
        for n in 0..10 {
            let id = add_track(&lib, &format!("o{n}"), &format!("O{n}"), 200_000);
            rows.push((id, vector(&[0.8, 0.3 + 0.01 * n as f32])));
            others.push(id);
        }
        for n in 0..300 {
            let id = add_track(&lib, &format!("z{n}"), "Z", 200_000);
            rows.push((id, vector(&[-1.0, 0.0])));
        }
        lib.save_embeddings(EMBEDDING_VERSION, &crate::similar_tracks::analyses(&rows))
            .unwrap();
        let similar = similar(Arc::new(lib));

        let radio = ids(&similar
            .radio(&[seed], &[], Familiarity::Any, 3)
            .unwrap()
            .unwrap());
        assert_eq!(radio.len(), LENGTH);
        assert!(others.iter().all(|id| radio.contains(id)));
        assert!(radio.iter().filter(|id| own.contains(id)).count() >= 10);
    }
}
