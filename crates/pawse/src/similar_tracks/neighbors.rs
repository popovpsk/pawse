use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use audio_embedding::similarity::{MeanAccumulator, Query, SCAN_CHUNK, TopN};
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

    pub fn vector(
        &self,
        repo: &dyn LibraryRepository,
        id: i64,
    ) -> music_library::Result<Option<Vec<f32>>> {
        Ok(repo
            .embedding(id, EMBEDDING_VERSION)?
            .filter(|vector| vector.len() == DIM))
    }

    pub fn mean_of(
        &self,
        repo: &dyn LibraryRepository,
        ids: &[i64],
    ) -> music_library::Result<Option<Vec<f32>>> {
        if ids.is_empty() {
            return Ok(None);
        }
        let wanted: HashSet<i64> = ids.iter().copied().collect();
        let mut sum = MeanAccumulator::new(DIM);
        repo.scan_embeddings(EMBEDDING_VERSION, SCAN_CHUNK, &mut |ids, vectors| {
            for (id, row) in ids.iter().zip(vectors.as_chunks::<DIM>().0) {
                if wanted.contains(id) {
                    sum.feed(row);
                }
            }
        })?;
        Ok((sum.count() > 0).then(|| sum.finish().into_vec()))
    }

    pub fn search(
        &self,
        repo: &dyn LibraryRepository,
        queries: Vec<Query>,
    ) -> music_library::Result<Vec<Vec<(i64, f32)>>> {
        if queries.is_empty() {
            return Ok(Vec::new());
        }
        let mean = self.mean(repo)?;
        let mut tops: Vec<TopN> = queries
            .into_iter()
            .map(|query| TopN::new(query, &mean))
            .collect();
        repo.scan_embeddings(EMBEDDING_VERSION, SCAN_CHUNK, &mut |ids, vectors| {
            for top in &mut tops {
                top.feed(ids, vectors);
            }
        })?;
        Ok(tops.into_iter().map(TopN::finish).collect())
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

    fn ids(found: &[(i64, f32)]) -> Vec<i64> {
        found.iter().map(|(id, _)| *id).collect()
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
        let seed_vector = neighbors.vector(&lib, seed).unwrap().unwrap();

        let found = neighbors
            .search(
                &lib,
                vec![
                    Query::nearest(seed_vector.clone(), 10).excluding([seed]),
                    Query::farthest(seed_vector, 1),
                    Query::nearest(vector(&[1.0, 0.0]), 10).excluding([seed, close]),
                ],
            )
            .unwrap();

        assert_eq!(ids(&found[0]), vec![close, other, far]);
        assert_eq!(ids(&found[1]), vec![far]);
        assert_eq!(ids(&found[2]), vec![other, far]);
        assert!(neighbors.vector(&lib, 999).unwrap().is_none());
        assert!(neighbors.search(&lib, Vec::new()).unwrap().is_empty());
    }

    #[test]
    fn the_mean_of_tracks_skips_those_without_a_vector() {
        let (_dir, lib) = library();
        let a = add_track(&lib, "a", "A", 200_000);
        let b = add_track(&lib, "b", "B", 200_000);
        let bare = add_track(&lib, "bare", "C", 200_000);
        lib.save_embeddings(
            EMBEDDING_VERSION,
            &[(a, vector(&[2.0, 0.0])), (b, vector(&[0.0, 2.0]))],
        )
        .unwrap();
        let neighbors = Neighbors::default();

        let mean = neighbors.mean_of(&lib, &[a, b, bare]).unwrap().unwrap();
        assert_eq!(&mean[..3], &[2.0, 2.0, 1.0]);
        assert!(neighbors.mean_of(&lib, &[bare]).unwrap().is_none());
        assert!(neighbors.mean_of(&lib, &[]).unwrap().is_none());
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
