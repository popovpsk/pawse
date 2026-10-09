use audio_embedding::similarity::Query;
use music_library::Track;
use rand::SeedableRng;
use rand::rngs::StdRng;

use super::SimilarTracks;
use super::pool::{Pool, now_secs, seed_taken};
use super::rerank::{Familiarity, Rerank, Weights};

pub const ANY_ARTIST_POOL: usize = 50;
pub const OTHER_ARTISTS_POOL: usize = 250;
pub const LENGTH: usize = 30;

impl SimilarTracks {
    pub fn radio(
        &self,
        seed_id: i64,
        familiarity: Familiarity,
        rng_seed: u64,
    ) -> music_library::Result<Option<Vec<Track>>> {
        let repo = self.shared.repo.as_ref();
        let neighbors = &self.shared.neighbors;
        let Some(seed) = repo.track(seed_id)? else {
            return Ok(None);
        };
        let Some(vector) = neighbors.vector(repo, seed_id)? else {
            return Ok(None);
        };
        let own = repo.same_artist_track_ids(&[seed_id])?;
        let found = neighbors.search(
            repo,
            vec![
                Query::nearest(vector.clone(), ANY_ARTIST_POOL).excluding([seed_id]),
                Query::nearest(vector, OTHER_ARTISTS_POOL)
                    .excluding(own.into_iter().chain([seed_id])),
            ],
        )?;
        let taken = seed_taken(repo, &seed)?;
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
            &[
                (seed, vector(&[1.0, 0.0])),
                (twin, vector(&[1.0, 0.1])),
                (skit, vector(&[1.0, 0.05])),
                (unrelated, vector(&[-1.0, 0.0])),
            ],
        )
        .unwrap();
        let similar = similar(Arc::new(lib));

        assert_eq!(
            ids(&similar.radio(seed, Familiarity::Any, 1).unwrap().unwrap()),
            vec![twin]
        );
        assert!(
            similar
                .radio(unanalysed, Familiarity::Any, 1)
                .unwrap()
                .is_none()
        );
        assert!(similar.radio(12345, Familiarity::Any, 1).unwrap().is_none());
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
        lib.save_embeddings(EMBEDDING_VERSION, &rows).unwrap();
        let similar = similar(Arc::new(lib));

        let radio = ids(&similar.radio(seed, Familiarity::Any, 3).unwrap().unwrap());
        assert_eq!(radio.len(), LENGTH);
        assert!(others.iter().all(|id| radio.contains(id)));
        assert!(radio.iter().filter(|id| own.contains(id)).count() >= 10);
    }
}
