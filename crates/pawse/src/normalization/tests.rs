use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use music_library::{LibraryRepository, RemoteSong, SqliteLibrary};

use super::*;
use crate::remote_media::{CacheStore, RemoteMedia};

struct Rig {
    dir: PathBuf,
    path: PathBuf,
    source_id: i64,
    item_id: i64,
}

impl Rig {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "pawse-loudness-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("library.db");
        let repo = SqliteLibrary::open_at(&path).unwrap();
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
        let mut rig = Self {
            dir,
            path,
            source_id,
            item_id: 0,
        };
        rig.serve(None);
        rig.item_id = repo
            .items_for_remote_keys(source_id, &["song".to_string()])
            .unwrap()[0];
        rig
    }

    fn serve(&self, gain_db: Option<f32>) {
        let song = RemoteSong {
            key: "song".into(),
            title: "Song".into(),
            gain_db,
            ..Default::default()
        };
        SqliteLibrary::open_at(&self.path)
            .unwrap()
            .apply_remote_listing(self.source_id, &[song], &[])
            .unwrap();
    }

    fn server_locator(&self) -> String {
        remote::locator(self.source_id, "song", "flac")
    }

    fn normalization(&self, on: bool) -> Normalization {
        let repo: Arc<dyn LibraryRepository> =
            Arc::new(SqliteLibrary::open_at(&self.path).unwrap());
        Normalization::new(
            Arc::new(Switch::new(on)),
            repo,
            RemoteMedia::with_cache(CacheStore::new(self.dir.join("media"))),
            Arc::new(|job| {
                std::thread::spawn(job);
            }),
        )
    }

    fn target(&self, fixture_name: &str) -> TrackRequest {
        TrackRequest {
            id: self.item_id,
            path: fixture(fixture_name).to_string_lossy().into_owned(),
            start_offset_ms: 0,
            duration: None,
            is_cue: false,
        }
    }

    fn stored(&self) -> Vec<StoredLoudness> {
        SqliteLibrary::open_at(&self.path)
            .unwrap()
            .loudness_values(VERSION)
            .unwrap()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../fixtures")
        .join(name)
}

const UNHURRIED: Duration = Duration::from_secs(60);

fn wait_for(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_click_estimate_is_used_at_once_and_saved() {
    let rig = Rig::new();
    let loudness = rig.normalization(true);
    let target = rig.target("long_44_stereo.flac");

    let gain = loudness
        .estimate_within(&target, &fixture("long_44_stereo.flac"), 0, UNHURRIED)
        .unwrap();

    assert_eq!(loudness.known(rig.item_id, &target.path), Some(gain));
    wait_for("the quick value in the db", || rig.stored().len() == 1);
    assert!(!rig.stored()[0].full);
    loudness.shutdown();
}

#[test]
fn a_tagged_file_is_left_to_its_tags() {
    let rig = Rig::new();
    let loudness = rig.normalization(true);
    let target = rig.target("replaygain.flac");

    assert_eq!(
        loudness.estimate_now(&target, &fixture("replaygain.flac"), 0),
        None
    );
    assert_eq!(loudness.known(rig.item_id, &target.path), None);
    assert!(!loudness.wants_estimate(&target));
    loudness.measure(target.clone());
    assert!(loudness.inner.lock_queue().jobs.is_empty());
    loudness.shutdown();
}

#[test]
fn a_full_measurement_replaces_the_quick_one_and_survives_a_restart() {
    let rig = Rig::new();
    let loudness = rig.normalization(true);
    let target = rig.target("long_44_stereo.flac");
    loudness.estimate_within(&target, &fixture("long_44_stereo.flac"), 0, UNHURRIED);

    loudness.measure(target.clone());
    wait_for("the full value", || loudness.inner.has_full(rig.item_id));
    wait_for("the full value in the db", || {
        rig.stored().first().is_some_and(|row| row.full)
    });
    loudness.shutdown();

    let restarted = rig.normalization(true);
    assert!(restarted.inner.has_full(rig.item_id));
    restarted.shutdown();
}

#[test]
fn a_click_estimate_queued_at_quit_is_still_saved() {
    let rig = Rig::new();
    let loudness = rig.normalization(true);
    let target = rig.target("long_44_stereo.flac");
    loudness.estimate_within(&target, &fixture("long_44_stereo.flac"), 0, UNHURRIED);
    loudness.shutdown();
    assert_eq!(rig.stored().len(), 1);
}

#[test]
fn slow_formats_skip_the_click_estimate() {
    let rig = Rig::new();
    let loudness = rig.normalization(true);
    let mut target = rig.target("long_44_stereo.flac");
    assert!(loudness.wants_click_estimate(&target));
    target.path = "/music/image.ape".into();
    assert!(!loudness.wants_click_estimate(&target));
    assert!(loudness.wants_estimate(&target));
    loudness.shutdown();
}

#[test]
fn nothing_is_measured_while_switched_off() {
    let rig = Rig::new();
    let loudness = rig.normalization(false);
    let target = rig.target("long_44_stereo.flac");

    assert_eq!(
        loudness.estimate_now(&target, &fixture("long_44_stereo.flac"), 0),
        None
    );
    loudness.measure(target.clone());
    loudness.prepare(target);
    assert!(loudness.inner.lock_queue().jobs.is_empty());
    loudness.shutdown();
}

const SERVER_GAIN: ReplayGain = ReplayGain {
    gain_db: -6.5,
    peak: None,
};

#[test]
fn a_server_gain_counts_only_for_tracks_played_from_that_server() {
    let rig = Rig::new();
    rig.serve(Some(SERVER_GAIN.gain_db));
    let loudness = rig.normalization(true);
    let local = rig.target("long_44_stereo.flac");
    let measured = loudness
        .estimate_within(&local, &fixture("long_44_stereo.flac"), 0, UNHURRIED)
        .unwrap();

    assert_eq!(
        loudness.known(rig.item_id, &rig.server_locator()),
        Some(SERVER_GAIN)
    );
    assert_eq!(loudness.known(rig.item_id, &local.path), Some(measured));
    let elsewhere = remote::locator(rig.source_id + 1, "song", "flac");
    assert_eq!(loudness.known(rig.item_id, &elsewhere), Some(measured));
    loudness.shutdown();
}

#[test]
fn a_track_with_a_server_gain_is_not_measured() {
    let rig = Rig::new();
    rig.serve(Some(SERVER_GAIN.gain_db));
    let loudness = rig.normalization(true);
    let target = TrackRequest {
        path: rig.server_locator(),
        ..rig.target("long_44_stereo.flac")
    };

    assert!(!loudness.wants_click_estimate(&target));
    loudness.prepare(target.clone());
    loudness.measure(target);
    assert!(loudness.inner.lock_queue().jobs.is_empty());
    loudness.shutdown();
}

#[test]
fn server_gains_follow_the_next_sync() {
    let rig = Rig::new();
    let loudness = rig.normalization(true);
    let locator = rig.server_locator();
    assert_eq!(loudness.known(rig.item_id, &locator), None);

    rig.serve(Some(SERVER_GAIN.gain_db));
    loudness.reload_server_gains();
    wait_for("the server gain", || {
        loudness.known(rig.item_id, &locator) == Some(SERVER_GAIN)
    });

    rig.serve(None);
    loudness.reload_server_gains();
    wait_for("the server gain to go", || {
        loudness.known(rig.item_id, &locator).is_none()
    });
    loudness.shutdown();
}

#[test]
fn an_older_reload_never_overwrites_a_newer_one() {
    let rig = Rig::new();
    let loudness = rig.normalization(true);
    rig.serve(Some(SERVER_GAIN.gain_db));
    loudness.inner.load_server_gains(2);
    rig.serve(None);
    loudness.inner.load_server_gains(1);
    assert_eq!(
        loudness.known(rig.item_id, &rig.server_locator()),
        Some(SERVER_GAIN)
    );
    loudness.shutdown();
}

#[test]
fn a_cue_segment_never_takes_the_file_tags() {
    assert!(takes_file_tags(false, 0));
    assert!(!takes_file_tags(true, 0));
    assert!(!takes_file_tags(false, 1_000));
}
