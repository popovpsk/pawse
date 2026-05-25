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
