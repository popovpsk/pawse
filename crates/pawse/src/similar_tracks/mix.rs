use std::ops::RangeInclusive;

use audio_embedding::similarity::Query;
use music_library::Track;
use rand::SeedableRng;
use rand::rngs::StdRng;

use super::SimilarTracks;
use super::pool::{Pool, now_secs};
use super::rerank::{Familiarity, Rerank, Taken, Weights};

pub const GAP: RangeInclusive<usize> = 1..=3;
pub const MAX_MIXED: usize = 400;
pub const POOL_PER_TRACK: usize = 2;
pub const MIN_POOL: usize = 100;

impl SimilarTracks {
    pub fn mix(
        &self,
        basis: &[i64],
        queue: &[i64],
        count: usize,
        familiarity: Familiarity,
        rng_seed: u64,
    ) -> music_library::Result<Option<Vec<Track>>> {
        let repo = self.shared.repo.as_ref();
        let neighbors = &self.shared.neighbors;
        let Some(mean) = neighbors.mean_of(repo, basis)? else {
            return Ok(None);
        };
        let count = count.min(MAX_MIXED);
        let own = repo.same_artist_track_ids(basis)?;
        let found = neighbors.search(
            repo,
            vec![
                Query::nearest(mean, (count * POOL_PER_TRACK).max(MIN_POOL))
                    .excluding(own.into_iter().chain(queue.iter().copied())),
            ],
        )?;
        let rerank = Rerank {
            count,
            familiarity,
            now: now_secs(),
            weights: Weights::default(),
        };
        Ok(Some(Pool::load(repo, found)?.pick(
            &rerank,
            Taken::default(),
            &mut StdRng::seed_from_u64(rng_seed),
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use audio_embedding::EMBEDDING_VERSION;
    use music_library::LibraryRepository;

    use super::*;
    use crate::similar_tracks::neighbors::tests::{add_track, library, vector};
    use crate::similar_tracks::radio::tests::similar;

    #[test]
    fn the_mix_comes_from_other_artists_than_the_queue() {
        let (_dir, lib) = library();
        let album: Vec<i64> = (0..3)
            .map(|n| add_track(&lib, &format!("a{n}"), "A", 200_000))
            .collect();
        let other_album = add_track(&lib, "a-other", "A", 200_000);
        let guest = add_track(&lib, "b", "B", 200_000);
        let close = add_track(&lib, "c", "C", 200_000);
        let far = add_track(&lib, "d", "D", 200_000);
        let unanalysed = add_track(&lib, "e", "E", 200_000);
        let mut rows: Vec<(i64, Vec<f32>)> =
            album.iter().map(|&id| (id, vector(&[1.0, 0.0]))).collect();
        rows.extend([
            (other_album, vector(&[1.0, 0.01])),
            (guest, vector(&[0.9, 0.2])),
            (close, vector(&[0.95, 0.1])),
            (far, vector(&[-1.0, 0.0])),
        ]);
        lib.save_embeddings(EMBEDDING_VERSION, &rows).unwrap();
        let similar = similar(Arc::new(lib));
        let queue = [album.as_slice(), &[guest]].concat();

        let mixed: Vec<i64> = similar
            .mix(&album, &queue, 5, Familiarity::Any, 1)
            .unwrap()
            .unwrap()
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(mixed, vec![close]);
        assert!(
            similar
                .mix(&[unanalysed], &[unanalysed], 5, Familiarity::Any, 1)
                .unwrap()
                .is_none()
        );
    }
}
