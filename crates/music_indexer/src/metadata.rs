use crate::error::{IndexerError, Result};
use music_library::TrackMetadata;
use std::path::Path;
use symphonia::core::codecs::CODEC_TYPE_NULL;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTagKey};
use symphonia::core::probe::Hint;
use chrono::{DateTime, Utc};
use std::time::SystemTime;

/// Supported audio file extensions
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "m4a", "aac", "wma", "aiff", "ape", "opus",
];

/// Checks if a file extension is a supported audio format
pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| AUDIO_EXTENSIONS.iter().any(|&e| e.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

/// Helper function to extract a tag value by key from metadata revisions
fn find_tag_from_revisions(
    metadata: &symphonia::core::meta::Metadata,
    key: StandardTagKey,
) -> Option<String> {
    metadata
        .current()
        .and_then(|tags| {
            tags.tags()
                .iter()
                .find(|tag| tag.std_key == Some(key))
                .map(|tag| tag.value.to_string())
        })
}

/// Parses a track number string (e.g., "1", "1/10", "01")
fn parse_track_number(s: &str) -> Option<i32> {
    // Handle formats like "1/10" or just "1"
    s.split('/')
        .next()
        .and_then(|part| part.trim().parse::<i32>().ok())
}

/// Parses a year string (e.g., "2023", "2023-01-15")
fn parse_year(s: &str) -> Option<i32> {
    // Try to extract just the year from various formats
    s.chars()
        .take(4)
        .collect::<String>()
        .parse::<i32>()
        .ok()
}

/// Extracts metadata from an audio file using symphonia
pub fn extract_metadata(path: &Path) -> Result<TrackMetadata> {
    // Open the file
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    // Create a hint with the file extension
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    // Probe the file to determine format
    let mut probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| IndexerError::Symphonia(e.to_string()))?;

    // Find the first audio track
    let track = probed
        .format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .cloned()
        .ok_or_else(|| IndexerError::NoAudioTrack(path.to_string_lossy().to_string()))?;

    let codec_params = &track.codec_params;

    // Extract standard tags from metadata
    let title = find_tag_from_revisions(&probed.format.metadata(), StandardTagKey::TrackTitle)
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string()
        });

    let artist = find_tag_from_revisions(&probed.format.metadata(), StandardTagKey::Artist);
    let album = find_tag_from_revisions(&probed.format.metadata(), StandardTagKey::Album);
    let genre = find_tag_from_revisions(&probed.format.metadata(), StandardTagKey::Genre);

    // Extract track number
    let track_number =
        find_tag_from_revisions(&probed.format.metadata(), StandardTagKey::TrackNumber)
            .and_then(|s| parse_track_number(&s));

    // Extract year/date
    let year = find_tag_from_revisions(&probed.format.metadata(), StandardTagKey::Date)
        .or_else(|| {
            find_tag_from_revisions(&probed.format.metadata(), StandardTagKey::Composer)
        })
        .and_then(|s| parse_year(&s));

    // Calculate duration from codec params
    let duration_ms = codec_params
        .n_frames
        .zip(codec_params.sample_rate)
        .map(|(frames, sample_rate)| (frames as f64 / sample_rate as f64 * 1000.0) as i64)
        .unwrap_or(0);

    // Extract technical info
    let sample_rate = codec_params.sample_rate.map(|r| r as i32);
    let channels = codec_params.channels.map(|c| c.count() as i32);

    // Get file info
    let file_size = path.metadata().map(|m| m.len() as i64).unwrap_or(0);

    let last_modified = path
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|sys_time: SystemTime| {
            sys_time
                .duration_since(SystemTime::UNIX_EPOCH)
                .ok()
                .and_then(|dur| DateTime::from_timestamp(dur.as_secs() as i64, 0))
        })
        .unwrap_or_else(Utc::now);

    Ok(TrackMetadata {
        file_path: path.to_path_buf(),
        title,
        artist,
        album,
        track_number,
        duration_ms,
        genre,
        year,
        sample_rate,
        channels,
        file_size,
        last_modified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_audio_file() {
        assert!(is_audio_file(Path::new("/music/song.mp3")));
        assert!(is_audio_file(Path::new("/music/song.FLAC")));
        assert!(is_audio_file(Path::new("/music/song.wav")));
        assert!(is_audio_file(Path::new("/music/song.ogg")));
        assert!(is_audio_file(Path::new("/music/song.m4a")));
        assert!(!is_audio_file(Path::new("/music/song.txt")));
        assert!(!is_audio_file(Path::new("/music/song.pdf")));
    }

    #[test]
    fn test_parse_track_number() {
        assert_eq!(parse_track_number("1"), Some(1));
        assert_eq!(parse_track_number("01"), Some(1));
        assert_eq!(parse_track_number("1/10"), Some(1));
        assert_eq!(parse_track_number("10/12"), Some(10));
        assert_eq!(parse_track_number("invalid"), None);
    }

    #[test]
    fn test_parse_year() {
        assert_eq!(parse_year("2023"), Some(2023));
        assert_eq!(parse_year("2023-01-15"), Some(2023));
        assert_eq!(parse_year("1999"), Some(1999));
        assert_eq!(parse_year("invalid"), None);
    }
}
