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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistSummary {
    pub id: i64,
    pub name: String,
    pub sort_name: String,
    pub track_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NewTrack {
    pub path: String,
    pub title: Option<String>,
    pub album_title: Option<String>,
    pub artist_names: Vec<String>,
    pub album_artist_names: Vec<String>,
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
    pub lyrics: Option<ScanLyrics>,
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

/// A frozen reference to one track within one playlist by **content key**
/// (path + start_offset_ms), not by `track_id`. Used to preserve playlist
/// contents across a full rescan, where `tracks` rows get fresh ids.
///
/// Original positions are not stored — `playlist_track_refs` returns the
/// refs in `(playlist_id, position)` order, and `restore_playlist_track_refs`
/// re-densifies positions starting from 0. So `Vec` order is the contract;
/// stale gaps from removed tracks aren't carried over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistTrackRef {
    pub playlist_id: i64,
    pub path: String,
    pub start_offset_ms: i32,
}

/// A frozen lyrics row keyed by **content key** (path + start_offset_ms), used
/// to carry non-disk-derived lyrics (network fetches, plus their not-found
/// markers) across a full rescan. `clear()` cascades the `lyrics` table away
/// with `tracks`, and a rescan only re-reads `.lrc`/embedded lyrics from disk —
/// so without this, fetched lyrics would vanish on every rescan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricsRef {
    pub path: String,
    pub start_offset_ms: i32,
    pub source: String,
    pub text: String,
    pub not_found: bool,
    pub updated_at: i64,
}
