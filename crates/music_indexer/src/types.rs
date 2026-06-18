use std::path::PathBuf;

/// A track's cover before it reaches the DB writer. `Bytes` is raw image data
/// the pipeline must hash + thumbnail; `Cached` is a hash already resolved by an
/// earlier track sharing the same directory's external cover, so the pipeline
/// references it without re-reading, re-hashing, or re-thumbnailing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverArt {
    Bytes {
        data: Vec<u8>,
        source_path: PathBuf,
        embedded: bool,
    },
    Cached(String),
}

impl CoverArt {
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            CoverArt::Bytes { data, .. } => Some(data),
            CoverArt::Cached(_) => None,
        }
    }
}

/// Raw output of the parsing business logic for one logical track. Carries the
/// embedded/external cover as [`CoverArt`] — the pipeline turns these into
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
    pub genres: Vec<String>,
    pub duration_ms: Option<u64>,
    pub cover_art: Option<CoverArt>,
    pub start_offset_ms: Option<u64>,
    pub bitrate: Option<u32>,
}

impl ScannedTrack {
    pub fn into_prepared(self, cover_hash: Option<String>) -> PreparedTrack {
        PreparedTrack {
            path: self.path,
            title: self.title,
            artist_names: self.artist_names,
            album_artist_names: self.album_artist_names,
            album_title: self.album_title,
            track_number: self.track_number,
            disc_number: self.disc_number,
            year: self.year,
            genres: self.genres,
            duration_ms: self.duration_ms,
            cover_hash,
            start_offset_ms: self.start_offset_ms,
            bitrate: self.bitrate,
        }
    }
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
    pub genres: Vec<String>,
    pub duration_ms: Option<u64>,
    pub cover_hash: Option<String>,
    pub start_offset_ms: Option<u64>,
    pub bitrate: Option<u32>,
}

/// The filesystem state to index, captured cheaply by [`collect_sources`](crate::collect_sources).
/// `fingerprint` is a hash over every relevant file's `(path, mtime, size)`; an
/// unchanged fingerprint means a rescan can be skipped entirely. `audio_files`
/// is the raw walk result — audio referenced by a `.cue` is still present and is
/// dropped only inside [`run`](crate::run), so the fast path stays stat-only.
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
        source_path: String,
        embedded: bool,
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
