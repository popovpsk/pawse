pub mod cue;
pub mod metadata;
pub mod pipeline;
pub mod scanner;
pub mod types;

pub use pipeline::{collect_sources, run};
pub use scanner::DirectoryScanner;
pub use types::{CoverArt, PreparedTrack, ScanEvent, ScannedTrack, SourceSet};

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use std::sync::atomic::{AtomicU64, Ordering};

    use super::metadata::read_metadata;
    use super::scanner::DirectoryScanner;
    use super::types::{CoverArt, ScanEvent, ScannedTrack};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn cover_bytes(track: &ScannedTrack) -> Option<&[u8]> {
        track.cover_art.as_ref().and_then(CoverArt::bytes)
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("pawse_test_{}_{}", std::process::id(), id));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn fixture_path(name: &str) -> PathBuf {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        PathBuf::from(manifest)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures")
            .join(name)
    }

    fn copy_fixture_wav(target: &Path) {
        let src = fixture_path("sine_440_16_44_mono.wav");
        std::fs::copy(&src, target).expect("failed to copy fixture WAV");
    }

    fn copy_fixture(name: &str, target: &Path) {
        let src = fixture_path(name);
        std::fs::copy(&src, target).expect("failed to copy fixture");
    }

    fn read_tagged_fixture(name: &str) -> super::ScannedTrack {
        let tmp = TempDir::new();
        let target = tmp.path().join(name);
        copy_fixture(name, &target);
        read_metadata(&target).expect("should read fixture metadata")
    }

    fn collect_scan_events(dir: &Path) -> Vec<ScanEvent> {
        let (tx, rx) = flume::unbounded();
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            DirectoryScanner::scan(&dir, tx);
        });
        rx.into_iter().collect()
    }

    fn run_scan_events(dir: &Path, known: HashSet<String>) -> Vec<ScanEvent> {
        let sources = crate::collect_sources(&[dir.to_path_buf()]);
        let (tx, rx) = flume::unbounded();
        std::thread::spawn(move || {
            crate::run(sources, known, tx);
        });
        rx.into_iter().collect()
    }

    fn count_covers(events: &[ScanEvent]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, ScanEvent::Cover { .. }))
            .count()
    }

    fn track_cover_hashes(events: &[ScanEvent]) -> Vec<Option<String>> {
        events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t.cover_hash.clone()),
                _ => None,
            })
            .collect()
    }

    // ── read_metadata: basic tag parsing ──────────────────────────────

    #[test]
    fn test_read_metadata_tagged_flac_basic() {
        let track = read_tagged_fixture("tagged_basic.flac");
        assert_eq!(track.title.as_deref(), Some("Test Track"));
        assert_eq!(track.artist_names, vec!["Test Artist".to_string()]);
        assert_eq!(track.album_title.as_deref(), Some("Test Album"));
        assert_eq!(
            track.album_artist_names,
            vec!["Test Album Artist".to_string()]
        );
        assert_eq!(track.track_number, Some(3));
        assert_eq!(track.disc_number, Some(1));
        assert_eq!(track.year, Some(2024));
        assert_eq!(track.duration_ms, Some(500));
        assert!(track.cover_art.is_none(), "no cover in temp dir");
        assert_eq!(track.start_offset_ms, None);
    }

    #[test]
    fn test_read_metadata_tagged_track_disc_slash() {
        let track = read_tagged_fixture("tagged_track_disc_slash.flac");
        assert_eq!(track.track_number, Some(5));
        assert_eq!(track.disc_number, Some(2));
    }

    #[test]
    fn test_read_metadata_year_from_full_date() {
        let track = read_tagged_fixture("tagged_track_disc_slash.flac");
        assert_eq!(
            track.year,
            Some(2023),
            "YEAR=2023-06-15 should yield the leading 4-digit year"
        );
    }

    #[test]
    fn test_normalize_genres_splits_dedups_and_filters() {
        let got = crate::metadata::normalize_genres(
            [
                "Rock, Alternative",
                "alternative",
                " Indie  Rock ",
                "Album",
                "255",
                "Drum & Bass",
                "Progressive Rock/Metal",
            ]
            .into_iter(),
        );
        assert_eq!(
            got.iter().map(String::as_str).collect::<Vec<_>>(),
            vec![
                "Rock",
                "Alternative",
                "Indie Rock",
                "Drum & Bass",
                "Progressive Rock",
                "Metal",
            ]
        );
    }

    #[test]
    fn test_read_metadata_tagged_flac_with_embedded_cover() {
        let track = read_tagged_fixture("tagged_with_cover.flac");
        assert_eq!(track.title.as_deref(), Some("Cover Track"));
        assert_eq!(track.year, Some(2022));
        assert_eq!(track.track_number, Some(1));
        assert_eq!(track.disc_number, Some(1));
        assert!(
            track.cover_art.is_some(),
            "embedded cover art should be present"
        );
    }

    #[test]
    fn test_read_metadata_embedded_cover_priority_over_external() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        let audio_path = dir.join("with_cover.flac");
        copy_fixture("tagged_with_cover.flac", &audio_path);

        std::fs::write(dir.join("front.jpg"), b"external_cover").unwrap();

        let track = read_metadata(&audio_path).expect("should read metadata");
        assert_ne!(
            cover_bytes(&track),
            Some(b"external_cover" as &[u8]),
            "embedded cover art should take priority over external"
        );
        assert!(track.cover_art.is_some());
    }

    #[test]
    fn test_read_metadata_tagged_mp3() {
        let track = read_tagged_fixture("tagged_mp3.mp3");
        assert_eq!(track.title.as_deref(), Some("MP3 Track"));
        assert_eq!(track.artist_names, vec!["MP3 Artist".to_string()]);
        assert_eq!(track.album_title.as_deref(), Some("MP3 Album"));
        assert_eq!(track.track_number, Some(7));
        assert!(track.duration_ms.unwrap() > 0);
    }

    #[test]
    fn test_read_metadata_tagged_ogg() {
        let track = read_tagged_fixture("tagged_ogg.ogg");
        assert_eq!(track.title.as_deref(), Some("OGG Track"));
        assert_eq!(track.artist_names, vec!["OGG Artist".to_string()]);
        assert_eq!(track.album_title.as_deref(), Some("OGG Album"));
        assert_eq!(track.track_number, Some(9));
        assert!(track.duration_ms.unwrap() > 0);
    }

    #[test]
    fn test_read_metadata_tagless_flac() {
        let track = read_tagged_fixture("tagless.flac");
        assert_eq!(track.title, None);
        assert!(track.artist_names.is_empty());
        assert!(track.album_artist_names.is_empty());
        assert_eq!(track.album_title, None);
        assert_eq!(track.track_number, None);
        assert_eq!(track.disc_number, None);
        assert_eq!(track.year, None);
        assert_eq!(track.duration_ms, Some(500));
        assert_eq!(track.start_offset_ms, None);
    }

    #[test]
    fn test_read_metadata_tagless_wav() {
        let tmp = TempDir::new();
        let audio_path = tmp.path().join("test.wav");
        copy_fixture_wav(&audio_path);

        let track = read_metadata(&audio_path).expect("should read wav metadata");
        assert_eq!(track.title, None);
        assert!(track.artist_names.is_empty());
        assert!(track.album_artist_names.is_empty());
        assert_eq!(track.album_title, None);
        assert_eq!(track.track_number, None);
        assert_eq!(track.disc_number, None);
        assert_eq!(track.year, None);
        assert!(track.duration_ms.is_some());
        assert_eq!(track.start_offset_ms, None);
    }

    // ── read_metadata: error cases ────────────────────────────────────

    #[test]
    fn test_read_metadata_nonexistent_file() {
        let result = read_metadata("/tmp/does_not_exist_12345.xyz");
        assert!(result.is_err(), "non-existent file should return Err");
    }

    #[test]
    fn test_read_metadata_corrupt_file() {
        let tmp = TempDir::new();
        let path = tmp.path().join("corrupt.wav");
        std::fs::write(&path, b"not valid audio data at all").unwrap();

        let result = read_metadata(&path);
        assert!(result.is_err(), "corrupt file should return Err");
    }

    #[test]
    fn test_read_metadata_duration() {
        let track = read_tagged_fixture("tagged_basic.flac");
        let duration = track.duration_ms.unwrap();
        assert!(
            (400..=600).contains(&duration),
            "expected ~500ms, got {duration}ms"
        );
    }

    // ── Cover art discovery ───────────────────────────────────────────

    #[test]
    fn test_cover_art_from_artwork_subdir() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        let artwork_dir = dir.join("Artwork");
        std::fs::create_dir(&artwork_dir).unwrap();

        std::fs::write(artwork_dir.join("Front.jpg"), b"artwork_front").unwrap();

        let audio_path = dir.join("test.wav");
        copy_fixture_wav(&audio_path);

        let track = read_metadata(&audio_path).expect("should read metadata");
        assert_eq!(
            cover_bytes(&track),
            Some(b"artwork_front" as &[u8]),
            "cover art in Artwork/ subdirectory of track's own dir should be found"
        );
    }

    #[test]
    fn test_cover_art_red_ops_naming() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        let artwork_dir = dir.join("Artwork");
        std::fs::create_dir(&artwork_dir).unwrap();

        std::fs::write(artwork_dir.join("FAKE-CAT-12345_02.jpg"), b"not_front").unwrap();
        std::fs::write(artwork_dir.join("FAKE-CAT-12345_01.jpg"), b"red_ops_front").unwrap();

        let audio_path = dir.join("test.wav");
        copy_fixture_wav(&audio_path);

        let track = read_metadata(&audio_path).expect("should read metadata");
        assert_eq!(
            cover_bytes(&track),
            Some(b"red_ops_front" as &[u8]),
            "RED/OPS _01 naming should be recognized as front cover"
        );
    }

    #[test]
    fn test_cover_art_negative_cd_word_boundary() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        std::fs::write(dir.join("WIGCD188J_front.jpg"), b"catalog").unwrap();
        std::fs::write(dir.join("cd.jpg"), b"disc_photo").unwrap();

        let audio_path = dir.join("test.wav");
        copy_fixture_wav(&audio_path);

        let track = read_metadata(&audio_path).expect("should read metadata");
        assert_eq!(
            cover_bytes(&track),
            Some(b"catalog" as &[u8]),
            "cd as a word boundary should NOT match inside catalog numbers like WIGCD188J"
        );
    }

    #[test]
    fn test_cover_art_prefers_direct_over_subdir() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        let artwork_dir = dir.join("Artwork");
        std::fs::create_dir(&artwork_dir).unwrap();

        std::fs::write(dir.join("front.jpg"), b"direct").unwrap();
        std::fs::write(artwork_dir.join("Front.jpg"), b"subdir").unwrap();

        let audio_path = dir.join("test.wav");
        copy_fixture_wav(&audio_path);

        let track = read_metadata(&audio_path).expect("should read metadata");
        assert_eq!(
            cover_bytes(&track),
            Some(b"direct" as &[u8]),
            "direct directory cover art should take priority over artwork subdirectory"
        );
    }

    #[test]
    fn test_cover_art_from_parent_subdir() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        let cd1 = dir.join("CD1");
        let artwork_dir = dir.join("Artwork");
        std::fs::create_dir(&cd1).unwrap();
        std::fs::create_dir(&artwork_dir).unwrap();

        std::fs::write(artwork_dir.join("front.jpg"), b"parent_artwork").unwrap();

        let audio_path = cd1.join("test.wav");
        copy_fixture_wav(&audio_path);

        let track = read_metadata(&audio_path).expect("should read metadata");
        assert_eq!(
            cover_bytes(&track),
            Some(b"parent_artwork" as &[u8]),
            "cover art in parent's Artwork/ subdir should be found for multi-disc tracks"
        );
    }

    #[test]
    fn test_cover_art_ignores_unknown_subdir() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        let random_dir = dir.join("random_folder");
        std::fs::create_dir(&random_dir).unwrap();
        std::fs::write(random_dir.join("front.jpg"), b"should_be_ignored").unwrap();

        let audio_path = dir.join("test.wav");
        copy_fixture_wav(&audio_path);

        let track = read_metadata(&audio_path).expect("should read metadata");
        assert_eq!(
            track.cover_art, None,
            "images in unknown subdirectories should be ignored"
        );
    }

    #[test]
    fn test_cover_art_no_cover_anywhere() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        let audio_path = dir.join("test.wav");
        copy_fixture_wav(&audio_path);

        let track = read_metadata(&audio_path).expect("should read metadata");
        assert_eq!(
            track.cover_art, None,
            "no cover art should be found in empty dir"
        );
    }

    #[test]
    fn test_cover_art_in_own_directory() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        std::fs::write(dir.join("cover.jpg"), b"own_cover").unwrap();

        let audio_path = dir.join("test.wav");
        copy_fixture_wav(&audio_path);

        let track = read_metadata(&audio_path).expect("should read metadata");
        assert_eq!(cover_bytes(&track), Some(b"own_cover" as &[u8]));
    }

    #[test]
    fn test_cover_art_negative_keywords_filtered() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        std::fs::write(dir.join("back.jpg"), b"back_cover").unwrap();
        std::fs::write(dir.join("rear_photo.jpg"), b"rear_cover").unwrap();
        std::fs::write(dir.join("inside.jpg"), b"inside_art").unwrap();
        std::fs::write(dir.join("booklet.jpg"), b"booklet_scan").unwrap();
        std::fs::write(dir.join("front.jpg"), b"real_front").unwrap();

        let audio_path = dir.join("test.wav");
        copy_fixture_wav(&audio_path);

        let track = read_metadata(&audio_path).expect("should read metadata");
        assert_eq!(
            cover_bytes(&track),
            Some(b"real_front" as &[u8]),
            "front cover should be selected despite negative-keyword files present"
        );
    }

    #[test]
    fn test_cover_art_only_negative_keywords_present() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        std::fs::write(dir.join("back_cover.jpg"), b"back_content").unwrap();
        std::fs::write(dir.join("cd_label.jpg"), b"cd_label_content").unwrap();
        std::fs::write(dir.join("poster_art.jpg"), b"poster_content").unwrap();

        let audio_path = dir.join("test.wav");
        copy_fixture_wav(&audio_path);

        let track = read_metadata(&audio_path).expect("should read metadata");
        assert_eq!(
            track.cover_art, None,
            "files with only negative keywords should not be selected (negative keyword only demotes prefix-matched files; non-prefix negative files fall through)"
        );
    }

    #[test]
    fn test_cover_art_fallback_by_size() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        std::fs::write(dir.join("image_a.jpg"), b"small").unwrap();
        std::fs::write(dir.join("image_b.jpg"), b"larger_content").unwrap();
        std::fs::write(dir.join("image_c.jpg"), b"ti").unwrap();

        let audio_path = dir.join("test.wav");
        copy_fixture_wav(&audio_path);

        let track = read_metadata(&audio_path).expect("should read metadata");
        assert_eq!(
            cover_bytes(&track),
            Some(b"larger_content" as &[u8]),
            "when no prefix-matched files, the largest file should be selected as cover art"
        );
    }

    #[test]
    fn test_cover_art_prefix_cover_highest_priority() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        std::fs::write(dir.join("folder.jpg"), b"folder").unwrap();
        std::fs::write(dir.join("cover.jpg"), b"cover_wins").unwrap();
        std::fs::write(dir.join("front.jpg"), b"front").unwrap();

        let audio_path = dir.join("test.wav");
        copy_fixture_wav(&audio_path);

        let track = read_metadata(&audio_path).expect("should read metadata");
        assert_eq!(
            cover_bytes(&track),
            Some(b"cover_wins" as &[u8]),
            "prefix 'cover' should have highest priority (index 0)"
        );
    }

    // ── DirectoryScanner::scan ────────────────────────────────────────

    #[test]
    fn test_scanner_empty_directory() {
        let tmp = TempDir::new();
        let events = collect_scan_events(tmp.path());

        assert_eq!(events.len(), 1, "empty dir should only emit Complete");
        assert!(matches!(events[0], ScanEvent::Complete));
    }

    #[test]
    fn test_scanner_single_audio_file() {
        let tmp = TempDir::new();
        let audio_path = tmp.path().join("test.wav");
        copy_fixture_wav(&audio_path);

        let events = collect_scan_events(tmp.path());

        let tracks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(tracks.len(), 1, "should find exactly one track");
        assert!(tracks[0].path.ends_with("test.wav"));
        assert!(
            events.iter().any(|e| matches!(e, ScanEvent::Complete)),
            "should emit Complete"
        );
    }

    #[test]
    fn test_scanner_skips_non_audio_files() {
        let tmp = TempDir::new();
        copy_fixture_wav(&tmp.path().join("audio.wav"));
        std::fs::write(tmp.path().join("readme.txt"), b"not audio").unwrap();
        std::fs::write(tmp.path().join("image.jpg"), b"not audio").unwrap();
        std::fs::write(tmp.path().join("notes.pdf"), b"not audio").unwrap();

        let events = collect_scan_events(tmp.path());

        let tracks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(tracks.len(), 1, "only audio files should be processed");
    }

    #[test]
    fn test_scanner_multiple_audio_files_progress() {
        let tmp = TempDir::new();
        for i in 0..12 {
            let name = format!("track_{i:02}.wav");
            copy_fixture_wav(&tmp.path().join(&name));
        }

        let events = collect_scan_events(tmp.path());

        let tracks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(_) => Some(()),
                _ => None,
            })
            .collect();
        assert_eq!(tracks.len(), 12, "all 12 tracks should be found");

        let progress: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Progress { scanned } => Some(*scanned),
                _ => None,
            })
            .collect();
        assert_eq!(progress, vec![10], "Progress should be emitted at 10");

        assert!(
            events.iter().any(|e| matches!(e, ScanEvent::Complete)),
            "should emit Complete"
        );
    }

    #[test]
    fn test_scanner_error_on_corrupt_file() {
        let tmp = TempDir::new();
        copy_fixture_wav(&tmp.path().join("good.wav"));
        std::fs::write(tmp.path().join("bad.wav"), b"corrupt audio data").unwrap();

        let events = collect_scan_events(tmp.path());

        let errors: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Error { path, error: _ } => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(errors.len(), 1, "should emit exactly one error");
        assert!(errors[0].ends_with("bad.wav"));

        let tracks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t.path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            tracks.len(),
            1,
            "good file should still be processed after error"
        );
        assert!(tracks[0].ends_with("good.wav"));

        assert!(
            events.iter().any(|e| matches!(e, ScanEvent::Complete)),
            "should emit Complete after errors"
        );
    }

    #[test]
    fn test_scanner_nested_directories() {
        let tmp = TempDir::new();
        let sub1 = tmp.path().join("artist1");
        let sub2 = tmp.path().join("artist2");
        std::fs::create_dir(&sub1).unwrap();
        std::fs::create_dir(&sub2).unwrap();

        copy_fixture_wav(&sub1.join("track_a.wav"));
        copy_fixture_wav(&sub2.join("track_b.wav"));
        copy_fixture_wav(&tmp.path().join("root_track.wav"));

        let events = collect_scan_events(tmp.path());

        let tracks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(&t.path),
                _ => None,
            })
            .collect();
        assert_eq!(tracks.len(), 3, "all files in nested dirs should be found");
    }

    #[test]
    fn test_scanner_receiver_dropped_stops_scan() {
        let tmp = TempDir::new();
        for i in 0..50 {
            copy_fixture_wav(&tmp.path().join(format!("track_{i:02}.wav")));
        }

        let (tx, rx) = flume::bounded(1);
        let dir = tmp.path().to_path_buf();
        std::thread::spawn(move || {
            DirectoryScanner::scan(&dir, tx);
        });

        let mut count = 0;
        for _event in rx.into_iter().take(5) {
            count += 1;
        }

        assert!(count < 50, "dropping receiver should stop scan early");
    }

    #[test]
    fn test_scanner_wav_and_flac_and_mp3_mixed() {
        let tmp = TempDir::new();
        copy_fixture_wav(&tmp.path().join("a.wav"));
        copy_fixture("tagless.flac", &tmp.path().join("b.flac"));
        copy_fixture("tagged_mp3.mp3", &tmp.path().join("c.mp3"));

        let events = collect_scan_events(tmp.path());

        let tracks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(_) => Some(()),
                _ => None,
            })
            .collect();
        assert_eq!(tracks.len(), 3, "all supported formats should be processed");
    }

    // ── CUE file processing ───────────────────────────────────────────

    #[test]
    fn test_scanner_cue_with_valid_audio() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        let audio_path = dir.join("audio.wav");
        copy_fixture_wav(&audio_path);

        let cue_content = "\
PERFORMER \"Album Performer\"
TITLE \"Album Title\"
REM DATE 2023
FILE \"audio.wav\" WAVE
  TRACK 01 AUDIO
    TITLE \"First Track\"
    PERFORMER \"Track Performer\"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE \"Second Track\"
    INDEX 01 00:00:15
";
        std::fs::write(dir.join("album.cue"), cue_content).unwrap();

        let events = collect_scan_events(dir);

        let tracks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(tracks.len(), 2, "CUE should produce 2 tracks");
        assert!(events.iter().any(|e| matches!(e, ScanEvent::Complete)));

        let t0 = &tracks[0];
        assert_eq!(t0.title.as_deref(), Some("First Track"));
        assert_eq!(t0.artist_names, vec!["Track Performer".to_string()]);
        assert_eq!(t0.album_artist_names, vec!["Album Performer".to_string()]);
        assert_eq!(t0.album_title.as_deref(), Some("Album Title"));
        assert_eq!(t0.track_number, Some(1));
        assert_eq!(t0.disc_number, Some(1));
        assert_eq!(t0.year, Some(2023));
        assert_eq!(t0.start_offset_ms, Some(0));
        assert!(t0.duration_ms.unwrap() > 0);

        let t1 = &tracks[1];
        assert_eq!(t1.title.as_deref(), Some("Second Track"));
        assert_eq!(t1.artist_names, vec!["Album Performer".to_string()]);
        assert_eq!(t1.track_number, Some(2));
        assert_eq!(t1.start_offset_ms, Some(200));
    }

    #[test]
    fn test_scanner_cue_skips_referenced_audio() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        let audio_path = dir.join("audio.wav");
        copy_fixture_wav(&audio_path);

        let cue_content = "\
FILE \"audio.wav\" WAVE
  TRACK 01 AUDIO
    TITLE \"Sole Track\"
    INDEX 01 00:00:00
";
        std::fs::write(dir.join("album.cue"), cue_content).unwrap();

        let events = collect_scan_events(dir);

        let tracks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            tracks.len(),
            1,
            "referenced audio should be skipped, only CUE track should appear"
        );
        assert!(tracks[0].path.ends_with("audio.wav"));
        assert_eq!(
            tracks[0].title.as_deref(),
            Some("Sole Track"),
            "track should come from CUE, not directory scan"
        );
    }

    #[test]
    fn test_scanner_cue_missing_audio_file() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        let cue_content = "\
FILE \"nonexistent.wav\" WAVE
  TRACK 01 AUDIO
    TITLE \"Ghost Track\"
    INDEX 01 00:00:00
";
        std::fs::write(dir.join("album.cue"), cue_content).unwrap();

        let events = collect_scan_events(dir);

        let errors: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Error { path, error: _ } => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(errors.len(), 1, "should emit error for missing audio file");
        assert!(errors[0].ends_with("album.cue"));
    }

    #[test]
    fn test_scanner_cue_malformed() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        std::fs::write(dir.join("broken.cue"), "not a valid cue sheet").unwrap();

        let events = collect_scan_events(dir);

        let errors: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Error { path, error: _ } => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(errors.len(), 1, "malformed CUE should produce error");
    }

    #[test]
    fn test_scanner_cue_multiple_cue_files() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        let audio1 = dir.join("audio1.wav");
        let audio2 = dir.join("audio2.wav");
        copy_fixture_wav(&audio1);
        copy_fixture_wav(&audio2);

        std::fs::write(
            dir.join("a.cue"),
            "FILE \"audio1.wav\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"A\"\n    INDEX 01 00:00:00\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("b.cue"),
            "FILE \"audio2.wav\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"B\"\n    INDEX 01 00:00:00\n",
        )
        .unwrap();

        let events = collect_scan_events(dir);

        let tracks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t.title.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(tracks.len(), 2, "both CUE files should produce tracks");
    }

    #[test]
    fn test_scanner_cue_with_subdirectory_audio() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        let sub = dir.join("subdir");
        std::fs::create_dir(&sub).unwrap();

        let audio_path = sub.join("audio.wav");
        copy_fixture_wav(&audio_path);

        let cue_content = "\
FILE \"subdir/audio.wav\" WAVE
  TRACK 01 AUDIO
    TITLE \"Subdir Track\"
    INDEX 01 00:00:00
";
        std::fs::write(dir.join("album.cue"), cue_content).unwrap();

        let events = collect_scan_events(dir);

        let tracks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            tracks.len(),
            1,
            "referenced audio in subdirectory should be skipped, only CUE track should appear"
        );
        assert_eq!(tracks[0].title.as_deref(), Some("Subdir Track"));
    }

    #[test]
    fn test_scanner_cue_last_track_duration() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        copy_fixture_wav(&dir.join("audio.wav"));

        let cue_content = "\
FILE \"audio.wav\" WAVE
  TRACK 01 AUDIO
    TITLE \"First\"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE \"Second\"
    INDEX 01 00:00:10
";
        std::fs::write(dir.join("album.cue"), cue_content).unwrap();

        let events = collect_scan_events(dir);

        let tracks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(tracks.len(), 2);

        let last = &tracks[1];
        assert!(
            last.duration_ms.unwrap() > 0,
            "last track duration should be computed from file duration minus offset"
        );
    }

    #[test]
    fn test_scanner_cue_with_embedded_cover() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        copy_fixture("tagged_with_cover.flac", &dir.join("audio.flac"));

        let cue_content = "\
FILE \"audio.flac\" WAVE
  TRACK 01 AUDIO
    TITLE \"Cover Track\"
    INDEX 01 00:00:00
";
        std::fs::write(dir.join("album.cue"), cue_content).unwrap();

        let events = collect_scan_events(dir);

        let tracks: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(tracks.len(), 1, "CUE track should be parsed");
        assert!(
            tracks[0].cover_hash.is_some(),
            "CUE track should inherit embedded cover art from audio file"
        );
    }

    #[test]
    fn test_scanner_cue_resolves_audio_by_stem_on_extension_mismatch() {
        let tmp = TempDir::new();
        let dir = tmp.path();

        // CUE references foo.wav (EAC convention) but only foo.flac exists on disk.
        copy_fixture("tagged_basic.flac", &dir.join("foo.flac"));

        let cue_content = "\
FILE \"foo.wav\" WAVE
  TRACK 01 AUDIO
    TITLE \"One\"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE \"Two\"
    INDEX 01 00:01:00
";
        std::fs::write(dir.join("album.cue"), cue_content).unwrap();

        let tracks: Vec<_> = collect_scan_events(dir)
            .into_iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t),
                _ => None,
            })
            .collect();

        assert_eq!(
            tracks.len(),
            2,
            "CUE must expand by resolving foo.flac despite the .wav reference"
        );
        assert_eq!(tracks[0].title.as_deref(), Some("One"));
    }

    // ── collect_sources: fast-path fingerprint ───────────────────────

    #[test]
    fn test_collect_sources_fingerprint_stable_then_changes() {
        let tmp = TempDir::new();
        copy_fixture_wav(&tmp.path().join("a.wav"));
        let folders = vec![tmp.path().to_path_buf()];

        let s1 = crate::collect_sources(&folders);
        let s2 = crate::collect_sources(&folders);
        assert_eq!(
            s1.fingerprint, s2.fingerprint,
            "fingerprint must be stable when nothing changed"
        );
        assert_eq!(s1.audio_files.len(), 1);

        copy_fixture_wav(&tmp.path().join("b.wav"));
        let s3 = crate::collect_sources(&folders);
        assert_ne!(
            s1.fingerprint, s3.fingerprint,
            "fingerprint must change when a file is added"
        );
        assert_eq!(s3.audio_files.len(), 2);
    }

    // collect_sources is stat-only: it returns the raw walk (cue-referenced
    // audio still present). The exclusion now happens in `run`, covered
    // end-to-end by test_scanner_cue_skips_referenced_audio.
    #[test]
    fn test_collect_sources_returns_raw_lists() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        copy_fixture_wav(&dir.join("audio.wav"));
        std::fs::write(
            dir.join("album.cue"),
            "FILE \"audio.wav\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"X\"\n    INDEX 01 00:00:00\n",
        )
        .unwrap();

        let sources = crate::collect_sources(&[dir.to_path_buf()]);
        assert_eq!(sources.cue_files.len(), 1);
        assert_eq!(
            sources.audio_files.len(),
            1,
            "collect_sources is stat-only; cue-referenced audio is dropped in run"
        );
    }

    #[test]
    fn test_scanner_cue_multi_disc_folder_layout() {
        let tmp = TempDir::new();
        let album_dir = tmp.path().join("2017 - Test Album [CAT-1]");
        let cd2 = album_dir.join("CD2");
        std::fs::create_dir_all(&cd2).unwrap();

        copy_fixture("tagged_basic.flac", &cd2.join("disc2.flac"));

        let cue_content = "\
PERFORMER \"Artist\"
TITLE \"Test Album CD2\"
FILE \"disc2.flac\" WAVE
  TRACK 01 AUDIO
    TITLE \"One\"
    INDEX 01 00:00:00
";
        std::fs::write(cd2.join("Artist - Test Album CD2.cue"), cue_content).unwrap();

        let tracks: Vec<_> = collect_scan_events(tmp.path())
            .into_iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t),
                _ => None,
            })
            .collect();

        assert_eq!(tracks.len(), 1);
        assert_eq!(
            tracks[0].disc_number,
            Some(2),
            "disc number must come from the CD2 folder"
        );
        assert_eq!(
            tracks[0].album_title.as_deref(),
            Some("Test Album"),
            "album title must come from the cleaned parent folder so discs merge"
        );
    }

    // ── pipeline: cover dedup / known hashes ──────────────────────────

    #[test]
    fn test_pipeline_dedupes_shared_external_cover() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        copy_fixture("cover_front.png", &dir.join("cover.png"));
        copy_fixture_wav(&dir.join("a.wav"));
        copy_fixture_wav(&dir.join("b.wav"));

        let events = collect_scan_events(dir);
        assert_eq!(
            count_covers(&events),
            1,
            "one external cover shared by two tracks must emit a single Cover"
        );

        let hashes = track_cover_hashes(&events);
        assert_eq!(hashes.len(), 2);
        assert!(hashes[0].is_some(), "tracks should carry the cover hash");
        assert_eq!(
            hashes[0], hashes[1],
            "both tracks must reference the same cover hash"
        );
    }

    #[test]
    fn test_pipeline_dedupes_shared_embedded_cover() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        copy_fixture("tagged_with_cover.flac", &dir.join("a.flac"));
        copy_fixture("tagged_with_cover.flac", &dir.join("b.flac"));

        let events = collect_scan_events(dir);
        assert_eq!(
            count_covers(&events),
            1,
            "identical embedded covers must be thumbnailed once"
        );
        let hashes = track_cover_hashes(&events);
        assert_eq!(hashes.len(), 2);
        assert_eq!(hashes[0], hashes[1]);
    }

    // Tracks in CD1/ and CD2/ have different parent dirs, so the per-dir cover
    // cache keys them separately and both take the Bytes path. Dedup of the
    // shared parent-level Artwork cover therefore rests entirely on the
    // cross-worker `claimed` set — this guards that interaction.
    #[test]
    fn test_pipeline_dedupes_parent_cover_across_disc_subdirs() {
        let tmp = TempDir::new();
        let album = tmp.path();
        let artwork = album.join("Artwork");
        let cd1 = album.join("CD1");
        let cd2 = album.join("CD2");
        std::fs::create_dir(&artwork).unwrap();
        std::fs::create_dir(&cd1).unwrap();
        std::fs::create_dir(&cd2).unwrap();

        copy_fixture("cover_front.png", &artwork.join("front.png"));
        copy_fixture_wav(&cd1.join("a.wav"));
        copy_fixture_wav(&cd2.join("b.wav"));

        let events = collect_scan_events(album);
        assert_eq!(
            count_covers(&events),
            1,
            "one parent-level cover shared by two disc subdirs must emit a single Cover"
        );
        let hashes = track_cover_hashes(&events);
        assert_eq!(hashes.len(), 2);
        assert!(hashes[0].is_some());
        assert_eq!(
            hashes[0], hashes[1],
            "both discs must reference the same cover hash"
        );
    }

    #[test]
    fn test_pipeline_known_hash_skips_cover_emission() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        copy_fixture("tagged_with_cover.flac", &dir.join("a.flac"));

        let first = run_scan_events(dir, HashSet::new());
        let hash = first
            .iter()
            .find_map(|e| match e {
                ScanEvent::Cover { hash, .. } => Some(hash.clone()),
                _ => None,
            })
            .expect("first scan should emit a cover");

        let known = HashSet::from([hash.clone()]);
        let second = run_scan_events(dir, known);

        assert_eq!(
            count_covers(&second),
            0,
            "a known cover hash must skip thumbnail generation and emission"
        );
        assert_eq!(
            track_cover_hashes(&second),
            vec![Some(hash)],
            "the track must still reference the known cover hash"
        );
    }

    // ── collect_sources: fingerprint sensitivity ─────────────────────

    #[test]
    fn test_fingerprint_tracks_image_changes() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        copy_fixture_wav(&dir.join("a.wav"));
        let folders = vec![dir.to_path_buf()];

        let base = crate::collect_sources(&folders).fingerprint;

        std::fs::write(dir.join("front.jpg"), b"small").unwrap();
        let added = crate::collect_sources(&folders).fingerprint;
        assert_ne!(
            base, added,
            "adding a cover image must change the fingerprint"
        );

        std::fs::write(dir.join("front.jpg"), b"a much larger cover payload").unwrap();
        let swapped = crate::collect_sources(&folders).fingerprint;
        assert_ne!(
            added, swapped,
            "swapping cover image content must change the fingerprint"
        );
    }

    #[test]
    fn test_fingerprint_ignores_non_media_files() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        copy_fixture_wav(&dir.join("a.wav"));
        let folders = vec![dir.to_path_buf()];

        let base = crate::collect_sources(&folders).fingerprint;
        std::fs::write(dir.join("notes.txt"), b"hello").unwrap();
        let after = crate::collect_sources(&folders).fingerprint;
        assert_eq!(
            base, after,
            "stray non-media files must not affect the fingerprint"
        );
    }

    // mtime is a distinct fingerprint input from path and size: re-tagging a
    // file that keeps the same size must still be detected. set_modified gives a
    // deterministic mtime change with no sleep.
    #[test]
    fn test_fingerprint_tracks_mtime_change() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        let path = dir.join("a.wav");
        copy_fixture_wav(&path);
        let folders = vec![dir.to_path_buf()];

        let base = crate::collect_sources(&folders).fingerprint;

        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000))
            .unwrap();
        drop(file);

        let touched = crate::collect_sources(&folders).fingerprint;
        assert_ne!(
            base, touched,
            "an mtime change alone (same path, same size) must flip the fingerprint"
        );
    }

    #[test]
    fn test_collect_sources_walks_multiple_folders_order_independent() {
        let a = TempDir::new();
        let b = TempDir::new();
        copy_fixture_wav(&a.path().join("a.wav"));
        copy_fixture_wav(&b.path().join("b.wav"));
        let ap = a.path().to_path_buf();
        let bp = b.path().to_path_buf();

        let forward = crate::collect_sources(&[ap.clone(), bp.clone()]);
        assert_eq!(
            forward.audio_files.len(),
            2,
            "audio from every passed folder must be collected"
        );

        let reverse = crate::collect_sources(&[bp, ap]);
        assert_eq!(
            forward.fingerprint, reverse.fingerprint,
            "fingerprint must not depend on the order folders are passed in (no spurious reindex on reorder)"
        );
    }

    #[test]
    fn test_collect_sources_missing_folder_does_not_panic() {
        let tmp = TempDir::new();
        let missing = tmp.path().join("was_deleted");

        let sources = crate::collect_sources(&[missing]);
        assert!(
            sources.audio_files.is_empty() && sources.cue_files.is_empty(),
            "a deleted/missing watched folder must yield nothing, not panic"
        );
    }

    // ── cue::resolve_audio_file ───────────────────────────────────────

    #[test]
    fn test_resolve_audio_file_exact_match() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        std::fs::write(dir.join("foo.flac"), b"x").unwrap();

        let resolved = crate::cue::resolve_audio_file(dir, "foo.flac");
        assert!(resolved.unwrap().ends_with("foo.flac"));
    }

    #[test]
    fn test_resolve_audio_file_stem_fallback_on_extension_mismatch() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        std::fs::write(dir.join("foo.flac"), b"x").unwrap();

        let resolved = crate::cue::resolve_audio_file(dir, "foo.wav");
        assert!(
            resolved.unwrap().ends_with("foo.flac"),
            "missing exact name should fall back to the same stem"
        );
    }

    #[test]
    fn test_resolve_audio_file_case_insensitive_stem_and_ext() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        std::fs::write(dir.join("FOO.FLAC"), b"x").unwrap();

        let resolved = crate::cue::resolve_audio_file(dir, "foo.wav");
        assert!(resolved.is_some(), "stem/ext matching must ignore case");
    }

    #[test]
    fn test_resolve_audio_file_unicode_case_insensitive_stem() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        std::fs::write(dir.join("CAFÉ.flac"), b"x").unwrap();

        let resolved = crate::cue::resolve_audio_file(dir, "Café.wav");
        assert!(
            resolved.is_some(),
            "stem matching must fold non-ASCII case (Café vs CAFÉ), not just ASCII"
        );
    }

    #[test]
    fn test_resolve_audio_file_no_match() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        std::fs::write(dir.join("other.flac"), b"x").unwrap();

        assert!(crate::cue::resolve_audio_file(dir, "foo.wav").is_none());
    }

    #[test]
    fn test_resolve_audio_file_multiple_candidates_deterministic() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        std::fs::write(dir.join("foo.flac"), b"x").unwrap();
        std::fs::write(dir.join("foo.ape"), b"x").unwrap();

        let resolved = crate::cue::resolve_audio_file(dir, "foo.wav");
        assert!(
            resolved.unwrap().ends_with("foo.ape"),
            "with multiple same-stem candidates the sorted-first one is chosen"
        );
    }

    // ── cue::process_cue_file (direct) ────────────────────────────────

    #[test]
    fn test_process_cue_file_offsets_and_durations() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        copy_fixture("tagged_basic.flac", &dir.join("audio.flac"));
        let cue = dir.join("album.cue");
        std::fs::write(
            &cue,
            "FILE \"audio.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"One\"\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"Two\"\n    INDEX 01 00:00:15\n",
        )
        .unwrap();

        let tracks = crate::cue::process_cue_file(&cue).expect("cue should parse");
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].title.as_deref(), Some("One"));
        assert_eq!(tracks[0].start_offset_ms, Some(0));
        assert_eq!(tracks[1].start_offset_ms, Some(200));
        assert_eq!(
            tracks[0].duration_ms,
            Some(200),
            "first track runs to the next index"
        );
        assert!(
            tracks[1].duration_ms.unwrap() > 0,
            "last track runs to the file's end"
        );
        assert_eq!(tracks[0].disc_number, Some(1));
    }

    // Regression: stem resolution must fold non-ASCII case. A cue referencing
    // `Café.wav` while only `CAFÉ.flac` exists (EAC extension mismatch + an
    // accented letter cased differently) must still resolve end-to-end: the cue
    // expands to its tracks (no Error), and the audio file is dropped from the
    // standalone set (no extra raw track). ASCII-only folding broke both.
    #[test]
    fn test_scanner_cue_resolves_audio_by_unicode_case_folded_stem() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        copy_fixture("tagged_basic.flac", &dir.join("CAFÉ.flac"));
        std::fs::write(
            dir.join("album.cue"),
            "FILE \"Café.wav\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"One\"\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"Two\"\n    INDEX 01 00:00:15\n",
        )
        .unwrap();

        let events = collect_scan_events(dir);

        let errors: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Error { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert!(
            errors.is_empty(),
            "cue must resolve the renamed audio, not error out: {errors:?}"
        );

        let titles: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t.title.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            titles,
            vec![Some("One".to_string()), Some("Two".to_string())],
            "exactly the two cue tracks — not a raw standalone track, not a double index"
        );
    }

    #[test]
    fn test_scan_emits_sidecar_lyrics() {
        use crate::types::LyricsSource;
        let tmp = TempDir::new();
        let dir = tmp.path();
        copy_fixture_wav(&dir.join("song.wav"));
        std::fs::write(dir.join("song.lrc"), "[00:01.00]first\n[00:02.50]second").unwrap();

        let tracks: Vec<_> = collect_scan_events(dir)
            .into_iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t),
                _ => None,
            })
            .collect();

        assert_eq!(tracks.len(), 1);
        let lyrics = tracks[0].lyrics.as_ref().expect("sidecar lyrics present");
        assert_eq!(lyrics.source, LyricsSource::Lrc);
        assert_eq!(lyrics.text, "[00:01.00]first\n[00:02.50]second");
    }

    #[test]
    fn test_scan_plain_sidecar_lyrics() {
        use crate::types::LyricsSource;
        let tmp = TempDir::new();
        let dir = tmp.path();
        copy_fixture_wav(&dir.join("song.wav"));
        std::fs::write(dir.join("song.lrc"), "just a plain\nlyric sheet").unwrap();

        let tracks: Vec<_> = collect_scan_events(dir)
            .into_iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t),
                _ => None,
            })
            .collect();

        let lyrics = tracks[0].lyrics.as_ref().expect("sidecar lyrics present");
        assert_eq!(lyrics.source, LyricsSource::Lrc);
        assert_eq!(lyrics.text, "just a plain\nlyric sheet");
    }

    #[test]
    fn test_scan_no_lyrics_without_sidecar_or_embedded() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        copy_fixture_wav(&dir.join("song.wav"));

        let tracks: Vec<_> = collect_scan_events(dir)
            .into_iter()
            .filter_map(|e| match e {
                ScanEvent::Track(t) => Some(t),
                _ => None,
            })
            .collect();

        assert!(tracks[0].lyrics.is_none());
    }

    #[test]
    fn test_fingerprint_tracks_lrc_changes() {
        let tmp = TempDir::new();
        let dir = tmp.path();
        copy_fixture_wav(&dir.join("song.wav"));
        let folders = vec![dir.to_path_buf()];

        let base = crate::collect_sources(&folders);
        assert_eq!(base.audio_files.len(), 1);

        std::fs::write(dir.join("song.lrc"), "[00:01.00]a").unwrap();
        let added = crate::collect_sources(&folders);
        assert_ne!(
            base.fingerprint, added.fingerprint,
            "adding a sidecar lrc must change the fingerprint"
        );
        assert_eq!(
            added.audio_files.len(),
            1,
            "a .lrc is not a track and must not enter audio_files"
        );
        assert!(
            added.cue_files.is_empty(),
            "a .lrc must not enter cue_files"
        );

        std::fs::write(dir.join("song.lrc"), "[00:01.00]a\n[00:02.00]b").unwrap();
        let edited = crate::collect_sources(&folders);
        assert_ne!(
            added.fingerprint, edited.fingerprint,
            "editing a sidecar lrc must change the fingerprint"
        );
    }
}
