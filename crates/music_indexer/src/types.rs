use std::path::PathBuf;

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

#[derive(Debug, Clone)]
pub enum ScanEvent {
    Track(ScannedTrack),
    Progress { scanned: usize },
    Error { path: PathBuf, error: String },
    Complete,
}
