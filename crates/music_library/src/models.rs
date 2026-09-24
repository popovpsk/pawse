#[derive(Debug, Clone)]
pub struct CoverArt {
    pub id: i64,
    pub small: Vec<u8>,
    pub large: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artist {
    pub id: i64,
    pub name: String,
    pub sort_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Album {
    pub id: i64,
    pub title: String,
    pub year: Option<i32>,
    pub cover_art_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub album_id: Option<i64>,
    pub track_number: Option<i32>,
    pub disc_number: i32,
    pub duration_ms: Option<i64>,
    pub year: Option<i32>,
    pub cover_art_id: Option<i64>,
    pub start_offset_ms: i32,
    #[serde(default)]
    pub liked: bool,
    #[serde(default)]
    pub bitrate: Option<u32>,
    #[serde(default)]
    pub is_cue: bool,
    #[serde(default = "available_by_default")]
    pub available: bool,
}

fn available_by_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumSummary {
    pub id: i64,
    pub title: String,
    pub year: Option<i32>,
    pub cover_art_id: Option<i64>,
    pub artist_name: String,
    pub artist_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumSearchEntry {
    pub album_id: i64,
    pub haystack: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtistGrouping {
    TrackArtist,
    #[default]
    AlbumArtist,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistSummary {
    pub id: i64,
    pub name: String,
    pub sort_name: String,
    pub track_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSummary {
    pub id: i64,
    pub kind: String,
    pub uri: String,
    pub enabled: bool,
    pub available: bool,
    pub track_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteSong {
    pub key: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub duration_ms: Option<i64>,
    pub size: Option<i64>,
    pub suffix: Option<String>,
    pub content_type: Option<String>,
    pub bitrate: Option<u32>,
    pub cover_key: Option<String>,
    pub cover_hash: Option<String>,
    pub artist_aliases: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RemoteSyncReport {
    pub total: usize,
    pub added: usize,
    pub adopted: usize,
    pub retired: usize,
    pub updated: usize,
    pub revived: usize,
    pub became_available: bool,
}

impl RemoteSyncReport {
    pub fn changed(&self) -> bool {
        self.added + self.adopted + self.retired + self.updated + self.revived > 0
            || self.became_available
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCover {
    pub hash: String,
    pub small: Vec<u8>,
    pub large: Vec<u8>,
    pub source_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSource {
    pub uri: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFolder {
    pub path: String,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NewTrack {
    pub path: String,
    pub title: Option<String>,
    pub album_title: Option<String>,
    pub artist_names: Vec<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub duration_ms: Option<u64>,
    pub cover_art_id: Option<i64>,
    pub start_offset_ms: Option<u64>,
    pub bitrate: Option<u32>,
}

/// The canonical `lyrics.source` tags and the one place that classifies a source
/// as disk-derived (re-read on rescan) vs. network (must survive a rescan).
pub mod lyrics_source {
    pub const LRC: &str = "lrc";
    pub const EMBEDDED: &str = "embedded";
    pub const LRCLIB: &str = "lrclib";

    pub fn is_disk_derived(source: &str) -> bool {
        matches!(source, LRC | EMBEDDED)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanLyrics {
    pub text: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLyrics {
    pub source: String,
    pub text: String,
    /// A remote lookup ran and found nothing: `text` is empty and the UI must
    /// not re-search.
    pub not_found: bool,
}

/// A track ready for batched scan insertion. Unlike [`NewTrack`], the cover is
/// referenced by content hash (resolved to a `cover_art_id` by the writer's
/// in-memory cache) rather than by id, so the parse workers never touch the DB.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanTrack {
    pub path: String,
    pub title: Option<String>,
    pub album_title: Option<String>,
    pub artist_names: Vec<String>,
    pub album_artist_names: Vec<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub duration_ms: Option<u64>,
    pub cover_hash: Option<String>,
    pub start_offset_ms: Option<u64>,
    pub bitrate: Option<u32>,
    pub is_cue: bool,
    pub lyrics: Option<ScanLyrics>,
    pub file_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackArtist {
    pub track_id: i64,
    pub artist_id: i64,
    pub role: String,
    pub credited_as: Option<String>,
    pub position: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistSummary {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
    pub track_count: i64,
}

pub mod delivery_state {
    pub const PENDING: i64 = 0;
    pub const SENT: i64 = 1;
    pub const DROPPED: i64 = 2;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPlay {
    pub track_id: Option<i64>,
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_number: Option<u32>,
    pub duration_secs: Option<u64>,
    pub played_secs: Option<u64>,
    pub started_at: u64,
    pub qualified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLove {
    pub track_id: Option<i64>,
    pub artist: String,
    pub title: String,
    pub loved: bool,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPlay {
    pub id: i64,
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_number: Option<u32>,
    pub duration_secs: Option<u64>,
    pub started_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingLove {
    pub id: i64,
    pub artist: String,
    pub title: String,
    pub loved: bool,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Sent,
    Dropped(String),
    Deferred(String),
}
