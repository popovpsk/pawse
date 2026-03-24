//! Domain models for the music library.
//!
//! These structs represent the core entities in the music library database.
//! They are designed to be mapped to/from SQLite rows.

use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::str::FromStr;

// ============================================================================
// TYPE ALIASES FOR IDS
// ============================================================================

/// Unique identifier for an artist
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArtistId(pub i64);

impl From<i64> for ArtistId {
    fn from(id: i64) -> Self {
        ArtistId(id)
    }
}

impl From<&i64> for ArtistId {
    fn from(id: &i64) -> Self {
        ArtistId(*id)
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

impl From<&i64> for AlbumId {
    fn from(id: &i64) -> Self {
        AlbumId(*id)
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

impl From<&i64> for TrackId {
    fn from(id: &i64) -> Self {
        TrackId(*id)
    }
}

// ============================================================================
// ENUMS
// ============================================================================

/// Type of artist (person, group, or unknown)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ArtistType {
    Person,
    Group,
    #[default]
    Unknown,
}

impl ArtistType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArtistType::Person => "person",
            ArtistType::Group => "group",
            ArtistType::Unknown => "unknown",
        }
    }
}

impl From<&str> for ArtistType {
    fn from(s: &str) -> Self {
        match s {
            "person" => ArtistType::Person,
            "group" => ArtistType::Group,
            _ => ArtistType::Unknown,
        }
    }
}

impl FromStr for ArtistType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "person" => ArtistType::Person,
            "group" => ArtistType::Group,
            _ => ArtistType::Unknown,
        })
    }
}

/// Type of album release
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AlbumType {
    Album,
    Single,
    Ep,
    Compilation,
    #[default]
    Unknown,
}

impl AlbumType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlbumType::Album => "album",
            AlbumType::Single => "single",
            AlbumType::Ep => "ep",
            AlbumType::Compilation => "compilation",
            AlbumType::Unknown => "unknown",
        }
    }
}

impl From<&str> for AlbumType {
    fn from(s: &str) -> Self {
        match s {
            "album" => AlbumType::Album,
            "single" => AlbumType::Single,
            "ep" => AlbumType::Ep,
            "compilation" => AlbumType::Compilation,
            _ => AlbumType::Unknown,
        }
    }
}

impl FromStr for AlbumType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "album" => AlbumType::Album,
            "single" => AlbumType::Single,
            "ep" => AlbumType::Ep,
            "compilation" => AlbumType::Compilation,
            _ => AlbumType::Unknown,
        })
    }
}

/// Role of an artist in a track or album relationship
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ArtistRole {
    #[default]
    Primary,
    Featured,
    Remixer,
    Producer,
    Engineer,
    Mixer,
    Mastering,
    Various, // For compilations
}

impl ArtistRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArtistRole::Primary => "primary",
            ArtistRole::Featured => "featured",
            ArtistRole::Remixer => "remixer",
            ArtistRole::Producer => "producer",
            ArtistRole::Engineer => "engineer",
            ArtistRole::Mixer => "mixer",
            ArtistRole::Mastering => "mastering",
            ArtistRole::Various => "various",
        }
    }
}

impl From<&str> for ArtistRole {
    fn from(s: &str) -> Self {
        match s {
            "primary" => ArtistRole::Primary,
            "featured" => ArtistRole::Featured,
            "remixer" => ArtistRole::Remixer,
            "producer" => ArtistRole::Producer,
            "engineer" => ArtistRole::Engineer,
            "mixer" => ArtistRole::Mixer,
            "mastering" => ArtistRole::Mastering,
            "various" => ArtistRole::Various,
            _ => ArtistRole::Primary,
        }
    }
}

impl FromStr for ArtistRole {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "primary" => ArtistRole::Primary,
            "featured" => ArtistRole::Featured,
            "remixer" => ArtistRole::Remixer,
            "producer" => ArtistRole::Producer,
            "engineer" => ArtistRole::Engineer,
            "mixer" => ArtistRole::Mixer,
            "mastering" => ArtistRole::Mastering,
            "various" => ArtistRole::Various,
            _ => ArtistRole::Primary,
        })
    }
}

// ============================================================================
// CORE ENTITIES
// ============================================================================

/// Represents an artist (person or group) in the music library.
#[derive(Debug, Clone)]
pub struct Artist {
    pub id: Option<ArtistId>,
    pub name: String,
    pub disambiguation: Option<String>,
    pub artist_type: ArtistType,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl Artist {
    pub fn new(name: String) -> Self {
        Self {
            id: None,
            name,
            disambiguation: None,
            artist_type: ArtistType::default(),
            created_at: None,
            updated_at: None,
        }
    }

    pub fn with_disambiguation(mut self, disambiguation: impl Into<String>) -> Self {
        self.disambiguation = Some(disambiguation.into());
        self
    }

    pub fn with_type(mut self, artist_type: ArtistType) -> Self {
        self.artist_type = artist_type;
        self
    }
}

/// Represents an album in the music library.
#[derive(Debug, Clone)]
pub struct Album {
    pub id: Option<AlbumId>,
    pub title: String,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub cover_art_path: Option<PathBuf>,
    pub total_discs: Option<i32>,
    pub album_type: AlbumType,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl Album {
    pub fn new(title: String) -> Self {
        Self {
            id: None,
            title,
            year: None,
            genre: None,
            cover_art_path: None,
            total_discs: None,
            album_type: AlbumType::default(),
            created_at: None,
            updated_at: None,
        }
    }

    pub fn with_year(mut self, year: i32) -> Self {
        self.year = Some(year);
        self
    }

    pub fn with_genre(mut self, genre: impl Into<String>) -> Self {
        self.genre = Some(genre.into());
        self
    }

    pub fn with_type(mut self, album_type: AlbumType) -> Self {
        self.album_type = album_type;
        self
    }
}

/// Represents a track (audio file) in the music library.
#[derive(Debug, Clone)]
pub struct Track {
    pub id: Option<TrackId>,
    pub file_path: PathBuf,
    pub title: String,
    pub duration_ms: i64,

    // File metadata
    pub file_size: Option<i64>,
    pub file_modified_at: Option<DateTime<Utc>>,

    // Track positioning
    pub track_number: Option<i32>,
    pub total_tracks: Option<i32>,
    pub disc_number: Option<i32>,

    // Technical audio info
    pub sample_rate: Option<i32>,
    pub bit_depth: Option<i32>,
    pub channels: Option<i32>,
    pub codec: Option<String>,

    // Playback stats
    pub play_count: i64,
    pub last_played_at: Option<DateTime<Utc>>,
    pub rating: Option<i32>,

    // Library state
    pub is_available: bool,
    pub is_favorite: bool,

    // Timestamps
    pub added_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,

    // Foreign keys
    pub album_id: Option<AlbumId>,
}

impl Track {
    pub fn new(file_path: PathBuf, title: String, duration_ms: i64) -> Self {
        Self {
            id: None,
            file_path,
            title,
            duration_ms,
            file_size: None,
            file_modified_at: None,
            track_number: None,
            total_tracks: None,
            disc_number: None,
            sample_rate: None,
            bit_depth: None,
            channels: None,
            codec: None,
            play_count: 0,
            last_played_at: None,
            rating: None,
            is_available: true,
            is_favorite: false,
            added_at: None,
            updated_at: None,
            album_id: None,
        }
    }

    pub fn with_album(mut self, album_id: AlbumId) -> Self {
        self.album_id = Some(album_id);
        self
    }

    pub fn with_track_number(mut self, track_number: i32) -> Self {
        self.track_number = Some(track_number);
        self
    }

    pub fn with_disc_number(mut self, disc_number: i32) -> Self {
        self.disc_number = Some(disc_number);
        self
    }

    pub fn with_sample_rate(mut self, sample_rate: i32) -> Self {
        self.sample_rate = Some(sample_rate);
        self
    }

    pub fn with_channels(mut self, channels: i32) -> Self {
        self.channels = Some(channels);
        self
    }
}

// ============================================================================
// JUNCTION ENTITIES (MANY-TO-MANY)
// ============================================================================

/// Relationship between a track and an artist.
#[derive(Debug, Clone)]
pub struct TrackArtist {
    pub track_id: TrackId,
    pub artist_id: ArtistId,
    pub role: ArtistRole,
    pub display_order: i32,
}

impl TrackArtist {
    pub fn new(track_id: TrackId, artist_id: ArtistId) -> Self {
        Self {
            track_id,
            artist_id,
            role: ArtistRole::Primary,
            display_order: 1,
        }
    }

    pub fn with_role(mut self, role: ArtistRole) -> Self {
        self.role = role;
        self
    }

    pub fn with_display_order(mut self, order: i32) -> Self {
        self.display_order = order;
        self
    }
}

/// Relationship between an album and an artist.
#[derive(Debug, Clone)]
pub struct AlbumArtist {
    pub album_id: AlbumId,
    pub artist_id: ArtistId,
    pub role: ArtistRole,
    pub display_order: i32,
}

impl AlbumArtist {
    pub fn new(album_id: AlbumId, artist_id: ArtistId) -> Self {
        Self {
            album_id,
            artist_id,
            role: ArtistRole::Primary,
            display_order: 1,
        }
    }

    pub fn with_role(mut self, role: ArtistRole) -> Self {
        self.role = role;
        self
    }

    pub fn with_display_order(mut self, order: i32) -> Self {
        self.display_order = order;
        self
    }
}
