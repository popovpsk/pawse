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
}
