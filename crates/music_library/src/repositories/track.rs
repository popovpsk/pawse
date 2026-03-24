//! Track repository for CRUD operations.

use crate::models::{AlbumId, ArtistId, ArtistRole, Track, TrackArtist, TrackId};
use crate::repositories::Result;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};

/// Repository for managing tracks in the database.
pub struct TrackRepository<'conn> {
    conn: &'conn Connection,
}

impl<'conn> TrackRepository<'conn> {
    pub fn new(conn: &'conn Connection) -> Self {
        Self { conn }
    }

    /// Create a new track.
    pub fn create(&self, track: &Track) -> Result<TrackId> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO tracks (file_path, title, duration_ms, file_size, file_modified_at,
             track_number, total_tracks, disc_number, sample_rate, bit_depth, channels, codec,
             play_count, last_played_at, rating, is_available, is_favorite,
             added_at, updated_at, album_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        )?;

        let now = Utc::now().to_rfc3339();
        let file_path_str = track.file_path.to_string_lossy().to_string();
        let file_modified_str = track.file_modified_at.as_ref().map(|d| d.to_rfc3339());
        let last_played_str = track.last_played_at.as_ref().map(|d| d.to_rfc3339());
        let album_id = track.album_id.map(|id| id.0);

        stmt.execute(params![
            file_path_str,
            track.title,
            track.duration_ms,
            track.file_size,
            file_modified_str,
            track.track_number,
            track.total_tracks,
            track.disc_number,
            track.sample_rate,
            track.bit_depth,
            track.channels,
            track.codec,
            track.play_count,
            last_played_str,
            track.rating,
            track.is_available as i32,
            track.is_favorite as i32,
            now,
            now,
            album_id,
        ])?;

        let id = self.conn.last_insert_rowid();
        Ok(TrackId(id))
    }

    /// Get track by ID.
    pub fn get_by_id(&self, id: TrackId) -> Result<Option<Track>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, file_path, title, duration_ms, file_size, file_modified_at,
             track_number, total_tracks, disc_number, sample_rate, bit_depth, channels, codec,
             play_count, last_played_at, rating, is_available, is_favorite,
             added_at, updated_at, album_id
             FROM tracks WHERE id = ?1",
        )?;

        let track = stmt.query_row(params![id.0], |row| {
            Ok(Track {
                id: Some(TrackId(row.get(0)?)),
                file_path: row.get::<_, String>(1)?.into(),
                title: row.get(2)?,
                duration_ms: row.get(3)?,
                file_size: row.get(4)?,
                file_modified_at: row.get::<_, Option<String>>(5)?.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
                track_number: row.get(6)?,
                total_tracks: row.get(7)?,
                disc_number: row.get(8)?,
                sample_rate: row.get(9)?,
                bit_depth: row.get(10)?,
                channels: row.get(11)?,
                codec: row.get(12)?,
                play_count: row.get(13)?,
                last_played_at: row.get::<_, Option<String>>(14)?.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
                rating: row.get(15)?,
                is_available: row.get::<_, i32>(16)? != 0,
                is_favorite: row.get::<_, i32>(17)? != 0,
                added_at: row.get::<_, Option<String>>(18)?.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
                updated_at: row.get::<_, Option<String>>(19)?.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
                album_id: row.get::<_, Option<i64>>(20)?.map(AlbumId),
            })
        }).optional()?;

        Ok(track)
    }

    /// Get track by file path.
    pub fn get_by_file_path(&self, file_path: &std::path::Path) -> Result<Option<Track>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, file_path, title, duration_ms, file_size, file_modified_at,
             track_number, total_tracks, disc_number, sample_rate, bit_depth, channels, codec,
             play_count, last_played_at, rating, is_available, is_favorite,
             added_at, updated_at, album_id
             FROM tracks WHERE file_path = ?1",
        )?;

        let file_path_str = file_path.to_string_lossy().to_string();
        let track = stmt.query_row(params![file_path_str], |row| {
            Ok(Track {
                id: Some(TrackId(row.get(0)?)),
                file_path: row.get::<_, String>(1)?.into(),
                title: row.get(2)?,
                duration_ms: row.get(3)?,
                file_size: row.get(4)?,
                file_modified_at: row.get::<_, Option<String>>(5)?.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
                track_number: row.get(6)?,
                total_tracks: row.get(7)?,
                disc_number: row.get(8)?,
                sample_rate: row.get(9)?,
                bit_depth: row.get(10)?,
                channels: row.get(11)?,
                codec: row.get(12)?,
                play_count: row.get(13)?,
                last_played_at: row.get::<_, Option<String>>(14)?.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
                rating: row.get(15)?,
                is_available: row.get::<_, i32>(16)? != 0,
                is_favorite: row.get::<_, i32>(17)? != 0,
                added_at: row.get::<_, Option<String>>(18)?.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
                updated_at: row.get::<_, Option<String>>(19)?.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
                album_id: row.get::<_, Option<i64>>(20)?.map(AlbumId),
            })
        }).optional()?;

        Ok(track)
    }

    /// Update an existing track.
    pub fn update(&self, track: &Track) -> Result<()> {
        let Some(id) = track.id else {
            return Err(crate::repositories::RepositoryError::Validation("Track must have an ID".into()));
        };

        let mut stmt = self.conn.prepare_cached(
            "UPDATE tracks SET file_path = ?1, title = ?2, duration_ms = ?3, file_size = ?4,
             file_modified_at = ?5, track_number = ?6, total_tracks = ?7, disc_number = ?8,
             sample_rate = ?9, bit_depth = ?10, channels = ?11, codec = ?12,
             play_count = ?13, last_played_at = ?14, rating = ?15, is_available = ?16,
             is_favorite = ?17, updated_at = datetime('now'), album_id = ?18
             WHERE id = ?19",
        )?;

        let file_path_str = track.file_path.to_string_lossy().to_string();
        let file_modified_str = track.file_modified_at.as_ref().map(|d| d.to_rfc3339());
        let last_played_str = track.last_played_at.as_ref().map(|d| d.to_rfc3339());
        let album_id = track.album_id.map(|id| id.0);

        stmt.execute(params![
            file_path_str,
            track.title,
            track.duration_ms,
            track.file_size,
            file_modified_str,
            track.track_number,
            track.total_tracks,
            track.disc_number,
            track.sample_rate,
            track.bit_depth,
            track.channels,
            track.codec,
            track.play_count,
            last_played_str,
            track.rating,
            track.is_available as i32,
            track.is_favorite as i32,
            album_id,
            id.0,
        ])?;

        Ok(())
    }

    /// Delete a track by ID.
    pub fn delete(&self, id: TrackId) -> Result<bool> {
        let mut stmt = self.conn.prepare_cached("DELETE FROM tracks WHERE id = ?1")?;
        let rows_affected = stmt.execute(params![id.0])?;
        Ok(rows_affected > 0)
    }

    /// Mark a track as unavailable (file missing).
    pub fn mark_unavailable(&self, id: TrackId) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "UPDATE tracks SET is_available = 0, updated_at = datetime('now') WHERE id = ?1",
        )?;
        stmt.execute(params![id.0])?;
        Ok(())
    }

    /// Increment play count and update last played timestamp.
    pub fn increment_play_count(&self, id: TrackId) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "UPDATE tracks SET play_count = play_count + 1, last_played_at = datetime('now'),
             updated_at = datetime('now') WHERE id = ?1",
        )?;
        stmt.execute(params![id.0])?;
        Ok(())
    }

    /// List all tracks with pagination.
    pub fn list(&self, limit: i32, offset: i32) -> Result<Vec<Track>> {
        let stmt = self.conn.prepare_cached(
            "SELECT id, file_path, title, duration_ms, file_size, file_modified_at,
             track_number, total_tracks, disc_number, sample_rate, bit_depth, channels, codec,
             play_count, last_played_at, rating, is_available, is_favorite,
             added_at, updated_at, album_id
             FROM tracks ORDER BY title LIMIT ?1 OFFSET ?2",
        )?;

        self.fetch_tracks(stmt, params![limit, offset])
    }

    /// List tracks by album ID.
    pub fn list_by_album(&self, album_id: AlbumId, limit: i32, offset: i32) -> Result<Vec<Track>> {
        let stmt = self.conn.prepare_cached(
            "SELECT id, file_path, title, duration_ms, file_size, file_modified_at,
             track_number, total_tracks, disc_number, sample_rate, bit_depth, channels, codec,
             play_count, last_played_at, rating, is_available, is_favorite,
             added_at, updated_at, album_id
             FROM tracks WHERE album_id = ?1 ORDER BY disc_number, track_number LIMIT ?2 OFFSET ?3",
        )?;

        self.fetch_tracks(stmt, params![album_id.0, limit, offset])
    }

    /// List available tracks.
    pub fn list_available(&self, limit: i32, offset: i32) -> Result<Vec<Track>> {
        let stmt = self.conn.prepare_cached(
            "SELECT id, file_path, title, duration_ms, file_size, file_modified_at,
             track_number, total_tracks, disc_number, sample_rate, bit_depth, channels, codec,
             play_count, last_played_at, rating, is_available, is_favorite,
             added_at, updated_at, album_id
             FROM tracks WHERE is_available = 1 ORDER BY title LIMIT ?1 OFFSET ?2",
        )?;

        self.fetch_tracks(stmt, params![limit, offset])
    }

    /// List favorite tracks.
    pub fn list_favorites(&self, limit: i32, offset: i32) -> Result<Vec<Track>> {
        let stmt = self.conn.prepare_cached(
            "SELECT id, file_path, title, duration_ms, file_size, file_modified_at,
             track_number, total_tracks, disc_number, sample_rate, bit_depth, channels, codec,
             play_count, last_played_at, rating, is_available, is_favorite,
             added_at, updated_at, album_id
             FROM tracks WHERE is_favorite = 1 ORDER BY title LIMIT ?1 OFFSET ?2",
        )?;

        self.fetch_tracks(stmt, params![limit, offset])
    }

    /// Search tracks by title.
    pub fn search_by_title(&self, query: &str, limit: i32) -> Result<Vec<Track>> {
        let stmt = self.conn.prepare_cached(
            "SELECT id, file_path, title, duration_ms, file_size, file_modified_at,
             track_number, total_tracks, disc_number, sample_rate, bit_depth, channels, codec,
             play_count, last_played_at, rating, is_available, is_favorite,
             added_at, updated_at, album_id
             FROM tracks WHERE title LIKE ?1 AND is_available = 1 ORDER BY title LIMIT ?2",
        )?;

        let search_pattern = format!("%{}%", query);
        self.fetch_tracks(stmt, params![search_pattern, limit])
    }

    /// Search tracks using FTS5 full-text search.
    pub fn search_fts(&self, query: &str, limit: i32) -> Result<Vec<Track>> {
        let stmt = self.conn.prepare_cached(
            "SELECT t.id, t.file_path, t.title, t.duration_ms, t.file_size, t.file_modified_at,
             t.track_number, t.total_tracks, t.disc_number, t.sample_rate, t.bit_depth, t.channels, t.codec,
             t.play_count, t.last_played_at, t.rating, t.is_available, t.is_favorite,
             t.added_at, t.updated_at, t.album_id
             FROM tracks t
             JOIN tracks_fts fts ON t.id = fts.rowid
             WHERE tracks_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        self.fetch_tracks(stmt, params![query, limit])
    }

    // Helper method to fetch tracks from a prepared statement
    fn fetch_tracks(
        &self,
        mut stmt: rusqlite::CachedStatement<'_>,
        params: impl rusqlite::Params,
    ) -> Result<Vec<Track>> {
        let tracks = stmt
            .query_map(params, |row| {
                Ok(Track {
                    id: Some(TrackId(row.get(0)?)),
                    file_path: row.get::<_, String>(1)?.into(),
                    title: row.get(2)?,
                    duration_ms: row.get(3)?,
                    file_size: row.get(4)?,
                    file_modified_at: row.get::<_, Option<String>>(5)?.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
                    track_number: row.get(6)?,
                    total_tracks: row.get(7)?,
                    disc_number: row.get(8)?,
                    sample_rate: row.get(9)?,
                    bit_depth: row.get(10)?,
                    channels: row.get(11)?,
                    codec: row.get(12)?,
                    play_count: row.get(13)?,
                    last_played_at: row.get::<_, Option<String>>(14)?.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
                    rating: row.get(15)?,
                    is_available: row.get::<_, i32>(16)? != 0,
                    is_favorite: row.get::<_, i32>(17)? != 0,
                    added_at: row.get::<_, Option<String>>(18)?.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
                    updated_at: row.get::<_, Option<String>>(19)?.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
                    album_id: row.get::<_, Option<i64>>(20)?.map(AlbumId),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(tracks)
    }

    // ========================================================================
    // TRACK-ARTIST RELATIONSHIPS
    // ========================================================================

    /// Add an artist to a track.
    pub fn add_artist(&self, track_id: TrackId, artist_id: ArtistId, role: ArtistRole, display_order: i32) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT OR REPLACE INTO track_artists (track_id, artist_id, role, display_order)
             VALUES (?1, ?2, ?3, ?4)",
        )?;

        stmt.execute(params![track_id.0, artist_id.0, role.as_str(), display_order])?;
        Ok(())
    }

    /// Remove an artist from a track.
    pub fn remove_artist(&self, track_id: TrackId, artist_id: ArtistId) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "DELETE FROM track_artists WHERE track_id = ?1 AND artist_id = ?2",
        )?;
        stmt.execute(params![track_id.0, artist_id.0])?;
        Ok(())
    }

    /// Get all artists for a track.
    pub fn get_track_artists(&self, track_id: TrackId) -> Result<Vec<TrackArtist>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT track_id, artist_id, role, display_order FROM track_artists
             WHERE track_id = ?1 ORDER BY display_order",
        )?;

        let artists = stmt
            .query_map(params![track_id.0], |row| {
                Ok(TrackArtist {
                    track_id: TrackId(row.get(0)?),
                    artist_id: ArtistId(row.get(1)?),
                    role: row.get::<_, String>(2)?.as_str().parse().unwrap_or(ArtistRole::Primary),
                    display_order: row.get(3)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(artists)
    }

    /// Remove all artists from a track.
    pub fn clear_artists(&self, track_id: TrackId) -> Result<()> {
        let mut stmt = self.conn.prepare_cached("DELETE FROM track_artists WHERE track_id = ?1")?;
        stmt.execute(params![track_id.0])?;
        Ok(())
    }
}
