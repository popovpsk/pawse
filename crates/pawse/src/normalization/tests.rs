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
        let song = RemoteSong {
            key: "song".into(),
            title: "Song".into(),
            ..Default::default()
        };
        repo.apply_remote_listing(source_id, &[song], &[]).unwrap();
        let item_id = repo
            .items_for_remote_keys(source_id, &["song".to_string()])
            .unwrap()[0];
        Self { dir, path, item_id }
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

    assert_eq!(loudness.known(rig.item_id), Some(gain));
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
    assert_eq!(loudness.known(rig.item_id), None);
    assert!(!loudness.wants_estimate(rig.item_id));
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
    assert!(loudness.wants_estimate(target.id));
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

#[test]
fn a_cue_segment_never_takes_the_file_tags() {
    assert!(takes_file_tags(false, 0));
    assert!(!takes_file_tags(true, 0));
    assert!(!takes_file_tags(false, 1_000));
}
