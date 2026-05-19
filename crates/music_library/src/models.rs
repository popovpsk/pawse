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

#[derive(Debug, Clone, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumSummary {
    pub id: i64,
    pub title: String,
    pub year: Option<i32>,
    pub cover_art_id: Option<i64>,
    pub artist_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumSearchEntry {
    pub album_id: i64,
    pub haystack: String,
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
