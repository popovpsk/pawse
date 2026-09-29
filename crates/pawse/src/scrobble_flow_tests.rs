use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use music_library::{LibraryRepository, RemoteSong, SqliteLibrary};
use scrobble::rewrite::{self, Sample};
use scrobble::{
    CsvLog, Love, NowPlaying, PlayAccumulator, Rewriter, Scrobble, ScrobbleHandle, ScrobbleStore,
    ScrobbleTarget, SubmitError, TargetId,
};

use super::*;
use crate::scrobble_store::LibraryScrobbleStore;
use crate::server_scrobble::ServerTarget;
use crate::servers::{RemoteError, ServerClient, ServerKind};
use crate::settings_store::ScrobbleSettings;

const WAIT: Duration = Duration::from_secs(5);

struct Song {
    key: &'static str,
    title: &'static str,
    artist: &'static str,
    album: Option<&'static str>,
    secs: i64,
}

struct Library {
    repo: Arc<dyn LibraryRepository>,
    source_id: i64,
    path: PathBuf,
    dir: PathBuf,
}

impl Library {
    fn new(songs: &[Song]) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "pawse-scrobble-flow-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("library.db");
        let repo: Arc<dyn LibraryRepository> = Arc::new(SqliteLibrary::open_at(&path).unwrap());
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
        let listing: Vec<RemoteSong> = songs
            .iter()
            .map(|song| RemoteSong {
                key: song.key.into(),
                title: song.title.into(),
                artist: Some(song.artist.into()),
                album: song.album.map(Into::into),
                album_artist: song.album.map(|_| song.artist.into()),
                duration_ms: Some(song.secs * 1000),
                ..Default::default()
            })
            .collect();
        repo.apply_remote_listing(source_id, &listing, &[]).unwrap();
        let mut scan = repo.open_scan_session().unwrap();
        scan.clear().unwrap();
        scan.finish().unwrap();
        Self {
            repo,
            source_id,
            path,
            dir,
        }
    }

    fn reopen(&self) -> Arc<dyn LibraryRepository> {
        Arc::new(SqliteLibrary::open_at(&self.path).unwrap())
    }

    fn item(&self, key: &str) -> i64 {
        self.repo
            .items_for_remote_keys(self.source_id, &[key.to_string()])
            .unwrap()[0]
    }

    fn track(&self, key: &str) -> music_library::Track {
        self.repo.track(self.item(key)).unwrap().unwrap()
    }

    fn store(&self) -> Arc<LibraryScrobbleStore> {
        Arc::new(LibraryScrobbleStore::new(self.repo.clone()))
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn settings(json: &str) -> ScrobbleSettings {
    serde_json::from_str(json).unwrap()
}

fn rules(settings: &ScrobbleSettings) -> Rewriter {
    let (rewriter, errors) = Rewriter::compile(&settings.rewrite);
    assert!(errors.is_empty(), "{errors:?}");
    rewriter
}

fn no_rules() -> Rewriter {
    Rewriter::default()
}

#[derive(Clone, Default)]
struct Service {
    id: Option<TargetId>,
    sent: Arc<Mutex<Vec<String>>>,
    offline: Arc<AtomicBool>,
    attempts: Arc<AtomicUsize>,
}

impl Service {
    fn new(id: TargetId) -> Self {
        Self {
            id: Some(id),
            ..Self::default()
        }
    }

    fn offline(id: TargetId) -> Self {
        let service = Self::new(id);
        service.offline.store(true, Ordering::SeqCst);
        service
    }

    fn sent(&self) -> Vec<String> {
        self.sent.lock().unwrap().clone()
    }

    fn deliver(&self, line: String) -> Result<(), SubmitError> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        if self.offline.load(Ordering::SeqCst) {
            return Err(SubmitError::Transient("offline".into()));
        }
        self.sent.lock().unwrap().push(line);
        Ok(())
    }
}

impl ScrobbleTarget for Service {
    fn id(&self) -> TargetId {
        self.id.unwrap()
    }

    fn max_batch(&self) -> usize {
        50
    }

    fn now_playing(&self, now_playing: &NowPlaying) -> Result<(), SubmitError> {
        self.deliver(format!(
            "now playing: {} - {}",
            now_playing.artist, now_playing.title
        ))
    }

    fn submit(&self, items: &[Scrobble]) -> Result<(), SubmitError> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        if self.offline.load(Ordering::SeqCst) {
            return Err(SubmitError::Transient("offline".into()));
        }
        let mut sent = self.sent.lock().unwrap();
        for item in items {
            sent.push(format!(
                "scrobble: {} - {} [{}]",
                item.artist,
                item.title,
                item.album.as_deref().unwrap_or("")
            ));
        }
        Ok(())
    }

    fn love(&self, artist: &str, title: &str, love: bool, _at: u64) -> Result<(), SubmitError> {
        let verb = if love { "love" } else { "unlove" };
        self.deliver(format!("{verb}: {artist} - {title}"))
    }
}

#[derive(Default)]
struct Nas {
    calls: Mutex<Vec<String>>,
}

impl ServerClient for Nas {
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

    fn scrobble(&self, key: &str, _played_at: u64) -> Result<(), RemoteError> {
        self.calls.lock().unwrap().push(format!("scrobble {key}"));
        Ok(())
    }

    fn now_playing(&self, key: &str) -> Result<(), RemoteError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("now playing {key}"));
        Ok(())
    }

    fn set_favorite(&self, key: &str, favorite: bool) -> Result<(), RemoteError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("favorite {key} {favorite}"));
        Ok(())
    }
}

fn spawn(
    store: Arc<LibraryScrobbleStore>,
    targets: Vec<Box<dyn ScrobbleTarget>>,
    rewriter: Rewriter,
) -> ScrobbleHandle {
    let (status, _) = flume::unbounded();
    ScrobbleHandle::spawn(store, targets, rewriter, status)
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn listen(
    library: &Library,
    handle: &ScrobbleHandle,
    key: &str,
    played_secs: u64,
    started_at: u64,
) -> bool {
    let track = library.track(key);
    let current = Current {
        meta: captured_meta(&*library.repo, &track, true),
        duration: Duration::from_millis(track.duration_ms.unwrap_or(0) as u64),
        timestamp: started_at,
        started: true,
    };
    handle.now_playing(now_playing_of(&current), current.meta.track_id);
    let start = Instant::now();
    let mut accumulator = PlayAccumulator::new();
    accumulator.on_play(start);
    let end = start + Duration::from_secs(played_secs);
    accumulator.on_pause(end);
    match pending_play(&current, accumulator.played(end), true, Commit::Final) {
        Some(play) => {
            let qualified = play.qualified;
            deliver(handle, play);
            qualified
        }
        None => false,
    }
}

fn like(library: &Library, handle: &ScrobbleHandle, key: &str, loved: bool) {
    let track_id = library.item(key);
    let (artist, title) = love_meta(&*library.repo, track_id, true).unwrap();
    handle.love(Love {
        track_id: Some(track_id),
        artist,
        title,
        loved,
        at: 1_700_000_000,
    });
}

fn let_it_be() -> Song {
    Song {
        key: "song-1",
        title: "Let It Be (Remastered 2009)",
        artist: "The Beatles",
        album: Some("Let It Be - Single"),
        secs: 243,
    }
}

const PRESETS_AND_A_REGEX_RULE: &str = r#"{"rewrite":{
    "presets":["remastered","single_ep"],
    "rules":[{"field":"artist","regex":true,"pattern":"^The (.+)$","replacement":"$1, The"}]}}"#;

#[test]
fn a_listen_reaches_every_target_the_way_each_expects_it() {
    let library = Library::new(&[let_it_be()]);
    let csv = library.dir.join("scrobbles.csv");
    let lastfm = Service::new(TargetId::Lastfm);
    let listenbrainz = Service::offline(TargetId::ListenBrainz);
    let nas = Arc::new(Nas::default());
    let server = ServerTarget::new(
        library.source_id,
        ServerKind::Subsonic,
        nas.clone(),
        library.repo.clone(),
        true,
        true,
    )
    .unwrap();
    let store = library.store();
    let handle = spawn(
        store.clone(),
        vec![
            Box::new(lastfm.clone()),
            Box::new(listenbrainz.clone()),
            Box::new(CsvLog::new(csv.clone(), "test".into())),
            Box::new(server),
        ],
        rules(&settings(PRESETS_AND_A_REGEX_RULE)),
    );

    assert!(listen(&library, &handle, "song-1", 243, 1_700_000_000));

    wait_until("last.fm, the csv log and the server", || {
        lastfm.sent().len() == 2
            && std::fs::read_to_string(&csv).is_ok_and(|s| s.contains(",scrobble"))
            && nas.calls.lock().unwrap().len() == 2
    });
    let mut sent = lastfm.sent();
    sent.sort();
    assert_eq!(
        sent,
        vec![
            "now playing: Beatles, The - Let It Be",
            "scrobble: Beatles, The - Let It Be [Let It Be]",
        ]
    );
    let log = std::fs::read_to_string(&csv).unwrap();
    assert!(
        log.contains("\"Beatles, The\",Let It Be,Let It Be,"),
        "csv log: {log}"
    );
    let mut calls = nas.calls.lock().unwrap().clone();
    calls.sort();
    assert_eq!(
        calls,
        vec!["now playing song-1", "scrobble song-1"],
        "the server matches by id, the text is irrelevant to it"
    );
    wait_until("listenbrainz to be tried", || {
        listenbrainz.attempts.load(Ordering::SeqCst) >= 2
    });
    let queued = store.pending_scrobbles(TargetId::ListenBrainz, 10).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(
        (queued[0].2.artist.as_str(), queued[0].2.title.as_str()),
        ("The Beatles", "Let It Be (Remastered 2009)"),
        "the history keeps the file's text; rules only shape what is sent"
    );
    assert_eq!(queued[0].2.album.as_deref(), Some("Let It Be - Single"));
}

#[test]
fn an_offline_backlog_goes_out_with_the_rules_after_a_restart() {
    let library = Library::new(&[let_it_be()]);
    let rules_json = settings(PRESETS_AND_A_REGEX_RULE);
    let down = Service::offline(TargetId::Lastfm);
    let handle = spawn(
        library.store(),
        vec![Box::new(down.clone())],
        rules(&rules_json),
    );
    assert!(listen(&library, &handle, "song-1", 243, 1_700_000_000));
    wait_until("the first failed attempt", || {
        down.attempts.load(Ordering::SeqCst) >= 2
    });
    drop(handle);

    let up = Service::new(TargetId::Lastfm);
    let store = Arc::new(LibraryScrobbleStore::new(library.reopen()));
    let _handle = spawn(store, vec![Box::new(up.clone())], rules(&rules_json));

    wait_until("the backlog after the restart", || {
        up.sent().iter().any(|s| s.starts_with("scrobble"))
    });
    assert_eq!(
        up.sent(),
        vec!["scrobble: Beatles, The - Let It Be [Let It Be]"]
    );
}

#[test]
fn rules_switched_on_while_offline_apply_to_the_queue() {
    let library = Library::new(&[let_it_be()]);
    let down = Service::offline(TargetId::Lastfm);
    let handle = spawn(library.store(), vec![Box::new(down.clone())], no_rules());
    assert!(listen(&library, &handle, "song-1", 243, 1_700_000_000));
    wait_until("the first failed attempt", || {
        down.attempts.load(Ordering::SeqCst) >= 2
    });

    handle.set_rewriter(rules(&settings(PRESETS_AND_A_REGEX_RULE)));
    let up = Service::new(TargetId::Lastfm);
    handle.configure(vec![Box::new(up.clone())]);

    wait_until("the queued play", || {
        up.sent().iter().any(|s| s.starts_with("scrobble"))
    });
    assert_eq!(
        up.sent(),
        vec!["scrobble: Beatles, The - Let It Be [Let It Be]"]
    );
}

#[test]
fn a_like_goes_out_rewritten_and_imports_back_onto_the_same_track() {
    let library = Library::new(&[let_it_be()]);
    let rules_json = settings(PRESETS_AND_A_REGEX_RULE);
    let lastfm = Service::new(TargetId::Lastfm);
    let handle = spawn(
        library.store(),
        vec![Box::new(lastfm.clone())],
        rules(&rules_json),
    );

    like(&library, &handle, "song-1", true);
    like(&library, &handle, "song-1", false);

    wait_until("the like and the unlike", || lastfm.sent().len() == 2);
    assert_eq!(
        lastfm.sent(),
        vec![
            "love: Beatles, The - Let It Be",
            "unlove: Beatles, The - Let It Be",
        ],
        "an unlike must reach the same name the like went out under"
    );

    let tracks = library.repo.all_tracks().unwrap();
    let ids: Vec<i64> = tracks.iter().map(|t| t.id).collect();
    let artists = library.repo.track_artists_map(&ids).unwrap();
    let index = crate::scrobble_import::build_index(&tracks, &artists, &rules(&rules_json));
    let matched = index
        .get(&crate::scrobble_import::key("Beatles, The", "Let It Be"))
        .expect("the service's loved track must find its way back");
    assert_eq!(matched, &vec![(library.item("song-1"), false)]);
}

#[test]
fn a_skip_is_history_and_is_never_queued() {
    let library = Library::new(&[Song {
        key: "song-2",
        title: "Pneuma",
        artist: "Tool",
        album: None,
        secs: 713,
    }]);
    let lastfm = Service::new(TargetId::Lastfm);
    let store = library.store();
    let handle = spawn(store.clone(), vec![Box::new(lastfm.clone())], no_rules());

    assert!(!listen(&library, &handle, "song-2", 25, 1_700_000_000));
    handle.flush();

    wait_until("now playing", || !lastfm.sent().is_empty());
    assert_eq!(lastfm.sent(), vec!["now playing: Tool - Pneuma"]);
    assert_eq!(store.pending_count(&[TargetId::Lastfm]).unwrap(), 0);
}

#[test]
fn the_settings_preview_counts_exactly_what_delivery_changes() {
    let library = Library::new(&[
        let_it_be(),
        Song {
            key: "song-2",
            title: "Pneuma",
            artist: "Tool",
            album: Some("Fear Inoculum (Deluxe Edition)"),
            secs: 713,
        },
        Song {
            key: "song-3",
            title: "HUMBLE. (Explicit)",
            artist: "Kendrick Lamar",
            album: Some("DAMN."),
            secs: 177,
        },
        Song {
            key: "song-4",
            title: "Track",
            artist: "Unknown Artist",
            album: None,
            secs: 200,
        },
    ]);
    let rules_json = settings(
        r#"{"rewrite":{"presets":["remastered","explicit","edition"],
        "rules":[{"field":"artist","pattern":"Unknown Artist"}]}}"#,
    );
    let rewriter = rules(&rules_json);
    let samples = crate::scrobble_rules_settings::library_samples(&*library.repo, true);
    assert_eq!(samples.len(), 4);

    let preview = rewrite::preview(&rewriter, &samples, 10);

    let delivered = |sample: &Sample| {
        let scrobble = Scrobble {
            artist: sample.artist.clone(),
            title: sample.title.clone(),
            album: sample.album.clone(),
            album_artist: sample.album_artist.clone(),
            track_number: None,
            duration_secs: None,
            timestamp: 0,
        };
        (rewriter.apply(&scrobble), scrobble)
    };
    let changed = samples
        .iter()
        .filter(|sample| {
            let (sent, original) = delivered(sample);
            sent != original
        })
        .count();
    assert_eq!(preview.tracks, changed);
    assert_eq!(preview.tracks, 3, "{preview:?}");
    for (before, after) in &preview.examples {
        let shown_as_sent = samples.iter().any(|sample| {
            let (sent, original) = delivered(sample);
            (original.title == *before && sent.title == *after)
                || (original.album.as_deref() == Some(before.as_str())
                    && sent.album.as_deref() == Some(after.as_str()))
        });
        assert!(
            shown_as_sent,
            "preview shows {before} → {after}, delivery disagrees"
        );
    }
    let unknown = samples
        .iter()
        .find(|sample| sample.artist == "Unknown Artist")
        .unwrap();
    let (sent, _) = delivered(unknown);
    assert_eq!(
        sent.artist, "Unknown Artist",
        "an artist a rule empties is sent as is, and the preview does not count it"
    );
}
