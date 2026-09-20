use std::sync::Arc;

use music_library::LibraryRepository;
use music_library::models::{DeliveryOutcome, NewLove, NewPlay, PendingLove, PendingPlay};
use scrobble::{Love, Outcome, Play, Scrobble, ScrobbleStore, StoreError, StoreResult, TargetId};

pub struct LibraryScrobbleStore {
    repo: Arc<dyn LibraryRepository>,
}

impl LibraryScrobbleStore {
    pub fn new(repo: Arc<dyn LibraryRepository>) -> Self {
        Self { repo }
    }
}

fn backend<T>(result: music_library::Result<T>) -> StoreResult<T> {
    result.map_err(|e| StoreError::Backend(e.to_string()))
}

fn keys(targets: &[TargetId]) -> Vec<&'static str> {
    targets.iter().map(|target| target.key()).collect()
}

fn to_new_play(play: &Play) -> NewPlay {
    NewPlay {
        track_id: play.track_id,
        artist: play.scrobble.artist.clone(),
        title: play.scrobble.title.clone(),
        album: play.scrobble.album.clone(),
        album_artist: play.scrobble.album_artist.clone(),
        track_number: play.scrobble.track_number,
        duration_secs: play.scrobble.duration_secs,
        played_secs: Some(play.played_secs),
        started_at: play.scrobble.timestamp,
        qualified: play.qualified,
    }
}

fn to_new_love(love: &Love) -> NewLove {
    NewLove {
        track_id: love.track_id,
        artist: love.artist.clone(),
        title: love.title.clone(),
        loved: love.loved,
        at: love.at,
    }
}

fn to_scrobble(pending: PendingPlay) -> (i64, Scrobble) {
    (
        pending.id,
        Scrobble {
            artist: pending.artist,
            title: pending.title,
            album: pending.album,
            album_artist: pending.album_artist,
            track_number: pending.track_number,
            duration_secs: pending.duration_secs,
            timestamp: pending.started_at,
        },
    )
}

fn to_love(pending: PendingLove) -> (i64, Love) {
    (
        pending.id,
        Love {
            track_id: None,
            artist: pending.artist,
            title: pending.title,
            loved: pending.loved,
            at: pending.at,
        },
    )
}

fn to_outcome(outcome: &Outcome) -> DeliveryOutcome {
    match outcome {
        Outcome::Sent => DeliveryOutcome::Sent,
        Outcome::Dropped(message) => DeliveryOutcome::Dropped(message.clone()),
        Outcome::Deferred(message) => DeliveryOutcome::Deferred(message.clone()),
    }
}

impl ScrobbleStore for LibraryScrobbleStore {
    fn record_play(&self, play: &Play, targets: &[TargetId]) -> StoreResult<i64> {
        backend(self.repo.record_play(&to_new_play(play), &keys(targets)))
    }

    fn record_love(&self, love: &Love, targets: &[TargetId]) -> StoreResult<i64> {
        backend(self.repo.record_love(&to_new_love(love), &keys(targets)))
    }

    fn pending_scrobbles(&self, target: TargetId, max: usize) -> StoreResult<Vec<(i64, Scrobble)>> {
        let pending = backend(self.repo.pending_plays(target.key(), max))?;
        Ok(pending.into_iter().map(to_scrobble).collect())
    }

    fn pending_loves(&self, target: TargetId, max: usize) -> StoreResult<Vec<(i64, Love)>> {
        let pending = backend(self.repo.pending_loves(target.key(), max))?;
        Ok(pending.into_iter().map(to_love).collect())
    }

    fn settle_scrobbles(
        &self,
        ids: &[i64],
        target: TargetId,
        outcome: &Outcome,
    ) -> StoreResult<()> {
        backend(
            self.repo
                .settle_plays(ids, target.key(), &to_outcome(outcome)),
        )
    }

    fn settle_loves(&self, ids: &[i64], target: TargetId, outcome: &Outcome) -> StoreResult<()> {
        backend(
            self.repo
                .settle_loves(ids, target.key(), &to_outcome(outcome)),
        )
    }

    fn pending_count(&self, targets: &[TargetId]) -> StoreResult<usize> {
        backend(self.repo.pending_scrobble_count(&keys(targets)))
    }

    fn trim(&self, cap: usize) -> StoreResult<()> {
        backend(self.repo.trim_pending_deliveries(cap)).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use music_library::SqliteLibrary;

    use super::*;

    fn store() -> (LibraryScrobbleStore, std::path::PathBuf) {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join("pawse-scrobble-store");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!(
            "test-{}-{}.db",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_file(&path);
        let repo: Arc<dyn LibraryRepository> = Arc::new(SqliteLibrary::open_at(&path).unwrap());
        (LibraryScrobbleStore::new(repo), path)
    }

    fn a_play(qualified: bool) -> Play {
        Play {
            track_id: None,
            scrobble: Scrobble {
                artist: "Tool".to_string(),
                title: "Pneuma".to_string(),
                album: Some("Fear Inoculum".to_string()),
                album_artist: Some("Tool".to_string()),
                track_number: Some(2),
                duration_secs: Some(713),
                timestamp: 1_700_000_000,
            },
            played_secs: 400,
            qualified,
        }
    }

    #[test]
    fn a_play_round_trips_through_the_database() {
        let (store, path) = store();
        store
            .record_play(&a_play(true), &[TargetId::Lastfm, TargetId::ListenBrainz])
            .unwrap();

        let pending = store.pending_scrobbles(TargetId::Lastfm, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].1.artist, "Tool");
        assert_eq!(pending[0].1.track_number, Some(2));
        assert_eq!(pending[0].1.duration_secs, Some(713));
        assert_eq!(pending[0].1.timestamp, 1_700_000_000);

        store
            .settle_scrobbles(&[pending[0].0], TargetId::Lastfm, &Outcome::Sent)
            .unwrap();

        assert!(
            store
                .pending_scrobbles(TargetId::Lastfm, 10)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store.pending_count(&[TargetId::ListenBrainz]).unwrap(),
            1,
            "settling one target must not settle the other"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unqualified_play_is_stored_without_being_queued() {
        let (store, path) = store();
        let id = store
            .record_play(&a_play(false), &[TargetId::Lastfm])
            .unwrap();

        assert!(id > 0, "the play must be written to the history");
        assert_eq!(store.pending_count(&[TargetId::Lastfm]).unwrap(), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn recording_the_same_play_twice_merges_instead_of_duplicating() {
        let (store, path) = store();
        let mut early = a_play(true);
        early.played_secs = 200;
        let first = store.record_play(&early, &[TargetId::Lastfm]).unwrap();

        let mut total = a_play(true);
        total.played_secs = 300;
        let second = store.record_play(&total, &[TargetId::Lastfm]).unwrap();

        assert_eq!(
            first, second,
            "the same listen must land on the same row, not a second one"
        );
        assert_eq!(store.pending_count(&[TargetId::Lastfm]).unwrap(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_love_keeps_the_time_it_happened() {
        let (store, path) = store();
        let love = Love {
            track_id: None,
            artist: "Tool".to_string(),
            title: "Pneuma".to_string(),
            loved: true,
            at: 1_700_000_500,
        };
        store.record_love(&love, &[TargetId::CsvLog]).unwrap();

        let pending = store.pending_loves(TargetId::CsvLog, 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].1.at, 1_700_000_500);
        assert!(pending[0].1.loved);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn trimming_bounds_the_queue_without_touching_the_history() {
        let (store, path) = store();
        for n in 0..5 {
            let mut play = a_play(true);
            play.scrobble.timestamp = 1_700_000_000 + n;
            store.record_play(&play, &[TargetId::Lastfm]).unwrap();
        }

        store.trim(2).unwrap();

        assert_eq!(store.pending_count(&[TargetId::Lastfm]).unwrap(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_cap_counts_plays_not_delivery_rows() {
        let (store, path) = store();
        let targets = [TargetId::Lastfm, TargetId::ListenBrainz, TargetId::CsvLog];
        for n in 0..5 {
            let mut play = a_play(true);
            play.scrobble.timestamp = 1_700_000_000 + n;
            store.record_play(&play, &targets).unwrap();
        }

        store.trim(4).unwrap();

        assert_eq!(
            store.pending_count(&targets).unwrap(),
            4,
            "four plays must survive a cap of four, regardless of how many services owe them"
        );
        let _ = std::fs::remove_file(&path);
    }
}
