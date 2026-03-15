use chrono::{DateTime, Utc};
use std::path::PathBuf;

/// Unique identifier for an artist
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArtistId(pub i64);

impl From<i64> for ArtistId {
    fn from(id: i64) -> Self {
        ArtistId(id)
    }
}

/// Unique identifier for an album
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AlbumId(pub i64);

impl From<i64> for AlbumId {
    fn from(id: i64) -> Self {
        AlbumId(id)
    }
}

/// Unique identifier for a track
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TrackId(pub i64);

impl From<i64> for TrackId {
    fn from(id: i64) -> Self {
        TrackId(id)
    }
}

/// Represents an artist in the music library
#[derive(Debug, Clone)]
pub struct Artist {
    pub id: Option<ArtistId>,
    pub name: String,
    pub created_at: Option<DateTime<Utc>>,
}

impl Artist {
    pub fn new(name: String) -> Self {
        Self {
            id: None,
            name,
            created_at: None,
        }
    }
}

/// Represents an album in the music library
#[derive(Debug, Clone)]
pub struct Album {
    pub id: Option<AlbumId>,
    pub title: String,
    pub artist_id: Option<ArtistId>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

impl Album {
    pub fn new(title: String) -> Self {
        Self {
            id: None,
            title,
            artist_id: None,
            year: None,
            genre: None,
            created_at: None,
        }
    }
}

/// Represents a track (audio file) in the music library
#[derive(Debug, Clone)]
pub struct Track {
    pub id: Option<TrackId>,
    pub file_path: PathBuf,
    pub title: String,
    pub artist_id: Option<ArtistId>,
    pub album_id: Option<AlbumId>,
    pub track_number: Option<i32>,
    pub duration_ms: i64,
    pub genre: Option<String>,
    pub year: Option<i32>,
    pub sample_rate: Option<i32>,
    pub channels: Option<i32>,
    pub file_size: Option<i64>,
    pub added_at: Option<DateTime<Utc>>,
    pub last_modified: Option<DateTime<Utc>>,
}

impl Track {
    pub fn new(file_path: PathBuf, title: String, duration_ms: i64) -> Self {
        Self {
            id: None,
            file_path,
            title,
            artist_id: None,
            album_id: None,
            track_number: None,
            duration_ms,
            genre: None,
            year: None,
            sample_rate: None,
            channels: None,
            file_size: None,
            added_at: None,
            last_modified: None,
        }
    }
}

/// Metadata extracted from an audio file for indexing
#[derive(Debug, Clone)]
pub struct TrackMetadata {
    pub file_path: PathBuf,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<i32>,
    pub duration_ms: i64,
    pub genre: Option<String>,
    pub year: Option<i32>,
    pub sample_rate: Option<i32>,
    pub channels: Option<i32>,
    pub file_size: i64,
    pub last_modified: DateTime<Utc>,
}

impl TrackMetadata {
    pub fn into_track(self) -> Track {
        Track {
            id: None,
            file_path: self.file_path,
            title: self.title,
            artist_id: None,
            album_id: None,
            track_number: self.track_number,
            duration_ms: self.duration_ms,
            genre: self.genre,
            year: self.year,
            sample_rate: self.sample_rate,
            channels: self.channels,
            file_size: Some(self.file_size),
            added_at: None,
            last_modified: Some(self.last_modified),
        }
    }
}
