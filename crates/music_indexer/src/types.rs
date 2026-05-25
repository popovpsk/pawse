use std::path::PathBuf;

/// Raw output of the parsing business logic for one logical track. Carries the
/// embedded/external cover art as raw bytes — the pipeline turns these into
/// deduped, pre-thumbnailed [`ScanEvent::Cover`] events plus a hash reference.
#[derive(Debug, Clone)]
pub struct ScannedTrack {
    pub path: PathBuf,
    pub title: Option<String>,
    pub artist_names: Vec<String>,
    pub album_artist_names: Vec<String>,
    pub album_title: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub duration_ms: Option<u64>,
    pub cover_art: Option<Vec<u8>>,
    pub start_offset_ms: Option<u64>,
}

/// A parsed track ready for the DB writer. Identical to [`ScannedTrack`] minus
/// the raw cover bytes, which are replaced by the cover's content hash
/// (`None` when the track has no cover).
#[derive(Debug, Clone)]
pub struct PreparedTrack {
    pub path: PathBuf,
    pub title: Option<String>,
    pub artist_names: Vec<String>,
    pub album_artist_names: Vec<String>,
    pub album_title: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub duration_ms: Option<u64>,
    pub cover_hash: Option<String>,
    pub start_offset_ms: Option<u64>,
}

/// The filesystem state to index, captured cheaply by [`collect_sources`](crate::collect_sources).
/// `fingerprint` is a hash over every relevant file's `(path, mtime, size)`; an
/// unchanged fingerprint means a rescan can be skipped entirely.
#[derive(Debug, Clone)]
pub struct SourceSet {
    pub cue_files: Vec<PathBuf>,
    pub audio_files: Vec<PathBuf>,
    pub fingerprint: String,
}

#[derive(Debug, Clone)]
pub enum ScanEvent {
    /// A pre-generated, deduped cover thumbnail, emitted once per unique hash.
    Cover {
        hash: String,
        small: Vec<u8>,
        large: Vec<u8>,
    },
    Track(PreparedTrack),
    Progress {
        scanned: usize,
    },
    Error {
        path: PathBuf,
        error: String,
    },
    Complete,
}
