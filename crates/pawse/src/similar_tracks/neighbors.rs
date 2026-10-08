use std::sync::{Arc, Mutex};

use audio_embedding::similarity::{MeanAccumulator, SCAN_CHUNK, TopN};
use audio_embedding::{DIM, EMBEDDING_VERSION};
use music_library::LibraryRepository;

#[derive(Default)]
pub struct Neighbors {
    mean: Mutex<Option<Arc<[f32]>>>,
}

impl Neighbors {
    pub fn invalidate(&self) {
        *self.mean.lock().unwrap() = None;
    }

    pub fn nearest(
        &self,
        repo: &dyn LibraryRepository,
        seed_id: i64,
        n: usize,
    ) -> music_library::Result<Vec<(i64, f32)>> {
        let Some(seed) = repo.embedding(seed_id, EMBEDDING_VERSION)? else {
            return Ok(Vec::new());
        };
        let mean = self.mean(repo)?;
        let mut top = TopN::new(seed_id, &seed, &mean, n);
        repo.scan_embeddings(EMBEDDING_VERSION, SCAN_CHUNK, &mut |ids, vectors| {
            top.feed(ids, vectors)
        })?;
        Ok(top.finish())
    }

    fn mean(&self, repo: &dyn LibraryRepository) -> music_library::Result<Arc<[f32]>> {
        let mut cached = self.mean.lock().unwrap();
        if let Some(mean) = cached.as_ref() {
            return Ok(mean.clone());
        }
        let mut sum = MeanAccumulator::new(DIM);
        repo.scan_embeddings(EMBEDDING_VERSION, SCAN_CHUNK, &mut |_, vectors| {
            sum.feed(vectors)
        })?;
        let mean: Arc<[f32]> = sum.finish().into();
        *cached = Some(mean.clone());
        Ok(mean)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use music_library::{NewTrack, SqliteLibrary};

    pub(crate) fn library() -> (tempfile::TempDir, SqliteLibrary) {
        let dir = tempfile::tempdir().unwrap();
        let lib = SqliteLibrary::open_at(dir.path().join("library.db")).unwrap();
        (dir, lib)
    }

    pub(crate) fn add_track(
        lib: &SqliteLibrary,
        title: &str,
        artist: &str,
        duration_ms: u64,
    ) -> i64 {
        let artist_id = lib.upsert_artist(artist).unwrap();
        lib.upsert_track(
            &NewTrack {
                path: format!("/music/{artist}/{title}.flac"),
                title: Some(title.into()),
                artist_names: vec![artist.into()],
                duration_ms: Some(duration_ms),
                ..Default::default()
            },
            None,
            &[(artist_id, 0)],
        )
        .unwrap()
    }

    pub(crate) fn vector(direction: &[f32]) -> Vec<f32> {
        let mut v = vec![1.0f32; DIM];
        for (slot, d) in v.iter_mut().zip(direction) {
            *slot += d;
        }
        v
    }

    #[test]
    fn the_nearest_tracks_point_the_same_way_after_centering() {
        let (_dir, lib) = library();
        let seed = add_track(&lib, "seed", "A", 200_000);
        let close = add_track(&lib, "close", "B", 200_000);
        let far = add_track(&lib, "far", "C", 200_000);
        let other = add_track(&lib, "other", "D", 200_000);
        lib.save_embeddings(
            EMBEDDING_VERSION,
            &[
                (seed, vector(&[1.0, 0.0])),
                (close, vector(&[0.9, 0.1])),
                (far, vector(&[-1.0, 0.0])),
                (other, vector(&[0.0, 1.0])),
            ],
        )
        .unwrap();
        let neighbors = Neighbors::default();

        let found: Vec<i64> = neighbors
            .nearest(&lib, seed, 10)
            .unwrap()
            .into_iter()
            .map(|(id, _)| id)
            .collect();

        assert_eq!(found, vec![close, other, far]);
        assert!(neighbors.nearest(&lib, 999, 10).unwrap().is_empty());
    }

    #[test]
    fn the_mean_is_cached_until_invalidated() {
        let (_dir, lib) = library();
        let a = add_track(&lib, "a", "A", 200_000);
        let b = add_track(&lib, "b", "B", 200_000);
        lib.save_embeddings(EMBEDDING_VERSION, &[(a, vector(&[2.0]))])
            .unwrap();
        let neighbors = Neighbors::default();
        assert_eq!(neighbors.mean(&lib).unwrap()[0], 3.0);

        lib.save_embeddings(EMBEDDING_VERSION, &[(b, vector(&[0.0]))])
            .unwrap();
        assert_eq!(neighbors.mean(&lib).unwrap()[0], 3.0);

        neighbors.invalidate();
        assert_eq!(neighbors.mean(&lib).unwrap()[0], 2.0);
    }
}
