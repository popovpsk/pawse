use std::sync::Arc;

use music_library::LibraryRepository;
use scrobble::{Love, NowPlaying, Scrobble, ScrobbleTarget, SubmitError, TargetId};

use crate::servers::{RemoteError, ServerClient, ServerKind};

pub struct ServerTarget {
    source_id: i64,
    client: Arc<dyn ServerClient>,
    repo: Arc<dyn LibraryRepository>,
    plays: bool,
    likes: bool,
}

impl ServerTarget {
    pub fn new(
        source_id: i64,
        kind: ServerKind,
        client: Arc<dyn ServerClient>,
        repo: Arc<dyn LibraryRepository>,
        plays: bool,
        likes: bool,
    ) -> Option<Self> {
        let plays = plays && kind.reports_plays();
        let likes = likes && kind.sends_favorites();
        (plays || likes).then_some(Self {
            source_id,
            client,
            repo,
            plays,
            likes,
        })
    }

    fn key(&self, track_id: Option<i64>) -> Result<String, SubmitError> {
        let Some(track_id) = track_id else {
            return Err(SubmitError::Unsupported);
        };
        match self.repo.remote_key_for_item(self.source_id, track_id) {
            Ok(Some(key)) => Ok(key),
            Ok(None) => Err(SubmitError::Unsupported),
            Err(e) => Err(SubmitError::Transient(e.to_string())),
        }
    }
}

fn submit_error(error: RemoteError) -> SubmitError {
    match error {
        RemoteError::Auth => SubmitError::Auth("wrong username or password".into()),
        RemoteError::Unreachable(message) => SubmitError::Transient(message),
        RemoteError::NotFound(_) => SubmitError::Unsupported,
        RemoteError::Other(message) => SubmitError::Permanent(message),
    }
}

impl ScrobbleTarget for ServerTarget {
    fn id(&self) -> TargetId {
        TargetId::Server(self.source_id)
    }

    fn max_batch(&self) -> usize {
        1
    }

    fn now_playing(&self, _now_playing: &NowPlaying) -> Result<(), SubmitError> {
        Err(SubmitError::Unsupported)
    }

    fn submit(&self, _items: &[Scrobble]) -> Result<(), SubmitError> {
        Err(SubmitError::Unsupported)
    }

    fn love(&self, _artist: &str, _title: &str, _love: bool, _at: u64) -> Result<(), SubmitError> {
        Err(SubmitError::Unsupported)
    }

    fn accepts_loves(&self) -> bool {
        self.likes
    }

    fn accepts_scrobbles(&self) -> bool {
        self.plays
    }

    fn now_playing_track(
        &self,
        _now_playing: &NowPlaying,
        track_id: Option<i64>,
    ) -> Result<(), SubmitError> {
        if !self.plays {
            return Err(SubmitError::Unsupported);
        }
        let key = self.key(track_id)?;
        self.client.now_playing(&key).map_err(submit_error)
    }

    fn submit_tracks(
        &self,
        items: &[Scrobble],
        track_ids: &[Option<i64>],
    ) -> Result<(), SubmitError> {
        let (Some(item), Some(track_id)) = (items.first(), track_ids.first()) else {
            return Ok(());
        };
        if !self.plays {
            return Err(SubmitError::Unsupported);
        }
        let key = self.key(*track_id)?;
        self.client
            .scrobble(&key, item.timestamp)
            .map_err(submit_error)
    }

    fn love_track(&self, love: &Love) -> Result<(), SubmitError> {
        if !self.likes {
            return Err(SubmitError::Unsupported);
        }
        let key = self.key(love.track_id)?;
        self.client
            .set_favorite(&key, love.loved)
            .map_err(submit_error)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    use music_library::{RemoteSong, SqliteLibrary};

    use super::*;

    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<String>>,
        fail: Option<RemoteError>,
    }

    impl ServerClient for Recorder {
        fn ping(&self) -> Result<(), RemoteError> {
            Ok(())
        }

        fn songs(&self) -> Result<Vec<RemoteSong>, RemoteError> {
            Ok(Vec::new())
        }

        fn favorite_keys(&self) -> Result<Vec<String>, RemoteError> {
            Ok(Vec::new())
        }

        fn cover_art(&self, _key: &str) -> Result<Vec<u8>, RemoteError> {
            Ok(Vec::new())
        }

        fn fetch_range(
            &self,
            _key: &str,
            _start: u64,
            _end: Option<u64>,
        ) -> Result<server_http::RangeBody, RemoteError> {
            Err(RemoteError::Other("no media".into()))
        }

        fn scrobble(&self, key: &str, played_at: u64) -> Result<(), RemoteError> {
            self.record(format!("scrobble {key} {played_at}"))
        }

        fn now_playing(&self, key: &str) -> Result<(), RemoteError> {
            self.record(format!("now_playing {key}"))
        }

        fn set_favorite(&self, key: &str, favorite: bool) -> Result<(), RemoteError> {
            self.record(format!("favorite {key} {favorite}"))
        }
    }

    impl Recorder {
        fn record(&self, call: String) -> Result<(), RemoteError> {
            self.calls.lock().unwrap().push(call);
            match &self.fail {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }
    }

    fn library() -> (Arc<dyn LibraryRepository>, std::path::PathBuf) {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join("pawse-server-scrobble");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!(
            "test-{}-{}.db",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_file(&path);
        (Arc::new(SqliteLibrary::open_at(&path).unwrap()), path)
    }

    fn server_with_song(repo: &Arc<dyn LibraryRepository>, key: &str) -> (i64, i64) {
        repo.reconcile_remote_sources(
            "subsonic",
            &[music_library::RemoteSource {
                uri: "me@http://nas".into(),
                name: "nas".into(),
            }],
        )
        .unwrap();
        let source_id = repo
            .sources()
            .unwrap()
            .into_iter()
            .find(|source| source.kind == "subsonic")
            .unwrap()
            .id;
        repo.apply_remote_listing(
            source_id,
            &[RemoteSong {
                key: key.into(),
                title: "Pneuma".into(),
                artist: Some("Tool".into()),
                duration_ms: Some(713_000),
                ..Default::default()
            }],
            &[],
        )
        .unwrap();
        let item = repo
            .items_for_remote_keys(source_id, &[key.into()])
            .unwrap()[0];
        (source_id, item)
    }

    fn target(
        repo: &Arc<dyn LibraryRepository>,
        source_id: i64,
        client: Arc<Recorder>,
    ) -> ServerTarget {
        ServerTarget::new(
            source_id,
            ServerKind::Subsonic,
            client,
            repo.clone(),
            true,
            true,
        )
        .unwrap()
    }

    fn scrobble_at(timestamp: u64) -> Scrobble {
        Scrobble {
            artist: "Tool".into(),
            title: "Pneuma".into(),
            album: None,
            album_artist: None,
            track_number: None,
            duration_secs: Some(713),
            timestamp,
        }
    }

    fn love(track_id: Option<i64>, loved: bool) -> Love {
        Love {
            track_id,
            artist: "Tool".into(),
            title: "Pneuma".into(),
            loved,
            at: 1_700_000_000,
        }
    }

    #[test]
    fn plays_likes_and_now_playing_go_to_the_servers_song() {
        let (repo, path) = library();
        let (source_id, item) = server_with_song(&repo, "song-9");
        let client = Arc::new(Recorder::default());
        let target = target(&repo, source_id, client.clone());

        target
            .submit_tracks(&[scrobble_at(1_700_000_000)], &[Some(item)])
            .unwrap();
        target.love_track(&love(Some(item), true)).unwrap();
        target.love_track(&love(Some(item), false)).unwrap();
        let now_playing = NowPlaying {
            artist: "Tool".into(),
            title: "Pneuma".into(),
            album: None,
            album_artist: None,
            track_number: None,
            duration_secs: None,
        };
        target.now_playing_track(&now_playing, Some(item)).unwrap();

        assert_eq!(
            *client.calls.lock().unwrap(),
            vec![
                "scrobble song-9 1700000000",
                "favorite song-9 true",
                "favorite song-9 false",
                "now_playing song-9",
            ]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_track_the_server_does_not_have_is_unsupported_not_an_error() {
        let (repo, path) = library();
        let (source_id, item) = server_with_song(&repo, "song-9");
        let client = Arc::new(Recorder::default());
        let target = target(&repo, source_id, client.clone());

        assert!(matches!(
            target.submit_tracks(&[scrobble_at(1)], &[Some(item + 1000)]),
            Err(SubmitError::Unsupported)
        ));
        assert!(matches!(
            target.love_track(&love(None, true)),
            Err(SubmitError::Unsupported)
        ));
        assert!(client.calls.lock().unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn server_failures_keep_the_auth_and_offline_split() {
        let (repo, path) = library();
        let (source_id, item) = server_with_song(&repo, "song-9");
        for (error, expected) in [
            (RemoteError::Auth, "auth"),
            (RemoteError::Unreachable("down".into()), "transient"),
            (RemoteError::Other("bad request".into()), "permanent"),
            (RemoteError::NotFound("gone".into()), "unsupported"),
        ] {
            let client = Arc::new(Recorder {
                fail: Some(error),
                ..Default::default()
            });
            let result = target(&repo, source_id, client).love_track(&love(Some(item), true));
            let got = match result {
                Err(SubmitError::Auth(_)) => "auth",
                Err(SubmitError::Transient(_)) => "transient",
                Err(SubmitError::Permanent(_)) => "permanent",
                Err(SubmitError::Unsupported) => "unsupported",
                other => panic!("unexpected {other:?}"),
            };
            assert_eq!(got, expected);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_switched_off_side_takes_nothing() {
        let (repo, path) = library();
        let client: Arc<dyn ServerClient> = Arc::new(Recorder::default());
        let likes_only = ServerTarget::new(
            1,
            ServerKind::Subsonic,
            client.clone(),
            repo.clone(),
            false,
            true,
        )
        .unwrap();
        assert!(!likes_only.accepts_scrobbles());
        assert!(likes_only.accepts_loves());

        let jellyfin = ServerTarget::new(
            1,
            ServerKind::Jellyfin,
            client.clone(),
            repo.clone(),
            true,
            true,
        )
        .unwrap();
        assert!(
            !jellyfin.accepts_scrobbles(),
            "Jellyfin plays are not reported"
        );

        assert!(ServerTarget::new(1, ServerKind::Dlna, client, repo.clone(), true, true).is_none());
        let _ = std::fs::remove_file(&path);
    }
}
