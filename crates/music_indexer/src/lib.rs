pub mod metadata;
pub mod scanner;
pub mod types;

pub use scanner::DirectoryScanner;
pub use types::{ScanEvent, ScannedTrack};

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::metadata::read_metadata;

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
        // We don't assert specific tags since fixture may or may not have them
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
}
