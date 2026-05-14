pub mod metadata;
pub mod scanner;
pub mod types;

pub use scanner::DirectoryScanner;
pub use types::{ScanEvent, ScannedTrack};

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use std::sync::atomic::{AtomicU64, Ordering};

    use super::metadata::read_metadata;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

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

    #[test]
    fn test_read_metadata_flac() {
        let path = fixture_path("02 - Selfless.flac");
        if !path.exists() {
            eprintln!("Fixture not found, skipping test");
            return;
        }
        let track = read_metadata(&path).expect("should read flac metadata");
        assert!(
            track.duration_ms.is_some(),
            "duration should be present"
        );
    }

    #[test]
    fn test_read_metadata_wav() {
        let path = fixture_path("sine_440_16_44_mono.wav");
        if !path.exists() {
            eprintln!("Fixture not found, skipping test");
            return;
        }
        let track = read_metadata(&path).expect("should read wav metadata");
        assert!(track.duration_ms.is_some());
    }

    #[test]
    fn test_read_metadata_multidisc_cd1() {
        let path = fixture_path(
            "Various Artists - Cyberpunk 2077 - Original Score (2020) [CD FLAC]/CD1/1.01. Marcin Przybylowicz - V.flac",
        );
        if !path.exists() {
            eprintln!("Multi-disc fixture not found, skipping test");
            return;
        }
        let track = read_metadata(&path).expect("should read flac metadata");
        assert_eq!(track.disc_number, Some(1));
        assert!(!track.album_artist_names.is_empty(), "album artist should be present");
        assert_eq!(track.album_title.as_deref(), Some("Cyberpunk 2077 - Original Score"));
        assert_eq!(track.title.as_deref(), Some("V"));
        assert!(track.cover_art.is_some(), "cover art should be found in parent dir for multi-disc albums");
    }

    #[test]
    fn test_read_metadata_multidisc_cd2() {
        let path = fixture_path(
            "Various Artists - Cyberpunk 2077 - Original Score (2020) [CD FLAC]/CD2/2.01. P.T. Adamczyk - The Voice In My Head.flac",
        );
        if !path.exists() {
            eprintln!("Multi-disc fixture not found, skipping test");
            return;
        }
        let track = read_metadata(&path).expect("should read flac metadata");
        assert_eq!(track.disc_number, Some(2));
        assert!(!track.album_artist_names.is_empty(), "album artist should be present");
        assert_eq!(track.album_title.as_deref(), Some("Cyberpunk 2077 - Original Score"));
        assert!(track.cover_art.is_some(), "cover art should be found in parent dir for multi-disc albums");
    }

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
            track.cover_art.as_deref(),
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
            track.cover_art.as_deref(),
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
            track.cover_art.as_deref(),
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
            track.cover_art.as_deref(),
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
            track.cover_art.as_deref(),
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
}
