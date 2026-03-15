use crate::db::Database;
use crate::error::{LibraryError, Result};
use crate::models::{Album, AlbumId, Artist, ArtistId, Track, TrackId};
use chrono::{DateTime, Utc};
use rusqlite::params;
use rusqlite::OptionalExtension;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Repository for async database operations
///
/// This struct wraps all database operations in `tokio::spawn_blocking`
/// to avoid blocking the async runtime.
#[derive(Clone)]
pub struct Repository {
    db: Arc<Database>,
}

impl Repository {
    pub fn new(db: Database) -> Self {
        Self {
            db: Arc::new(db),
        }
    }

    // =========================================================================
    // Artist operations
    // =========================================================================

    pub async fn get_artist_by_name(&self, name: &str) -> Result<Option<Artist>> {
        let name = name.to_string();
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            let mut stmt = conn.prepare("SELECT id, name, created_at FROM artists WHERE name = ?")?;

            let artist = stmt
                .query_row(params![name], |row| {
                    let id: i64 = row.get(0)?;
                    let name: String = row.get(1)?;
                    let created_at: Option<String> = row.get(2)?;

                    let created_at = created_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });

                    Ok(Artist {
                        id: Some(ArtistId(id)),
                        name,
                        created_at,
                    })
                })
                .optional()?;

            Ok(artist)
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    pub async fn create_artist(&self, name: &str) -> Result<ArtistId> {
        let name = name.to_string();
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            conn.execute("INSERT INTO artists (name) VALUES (?)", params![name])?;
            let id = conn.last_insert_rowid();
            Ok(ArtistId(id))
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    pub async fn get_or_create_artist(&self, name: &str) -> Result<ArtistId> {
        if let Some(artist) = self.get_artist_by_name(name).await? {
            return Ok(artist.id.unwrap());
        }
        self.create_artist(name).await
    }

    pub async fn get_all_artists(&self) -> Result<Vec<Artist>> {
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            let mut stmt = conn.prepare("SELECT id, name, created_at FROM artists ORDER BY name")?;

            let artists = stmt
                .query_map([], |row| {
                    let id: i64 = row.get(0)?;
                    let name: String = row.get(1)?;
                    let created_at: Option<String> = row.get(2)?;

                    let created_at = created_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });

                    Ok(Artist {
                        id: Some(ArtistId(id)),
                        name,
                        created_at,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(artists)
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    // =========================================================================
    // Album operations
    // =========================================================================

    pub async fn get_album(&self, title: &str, artist_id: Option<ArtistId>) -> Result<Option<Album>> {
        let title = title.to_string();
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            let artist_id = artist_id.map(|id| id.0);

            let mut stmt = conn.prepare(
                "SELECT id, title, artist_id, year, genre, created_at FROM albums WHERE title = ? AND (artist_id = ? OR (? IS NULL AND artist_id IS NULL))"
            )?;

            let album = stmt
                .query_row(params![title, artist_id, artist_id], |row| {
                    let id: i64 = row.get(0)?;
                    let title: String = row.get(1)?;
                    let artist_id: Option<i64> = row.get(2)?;
                    let year: Option<i32> = row.get(3)?;
                    let genre: Option<String> = row.get(4)?;
                    let created_at: Option<String> = row.get(5)?;

                    let created_at = created_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });

                    Ok(Album {
                        id: Some(AlbumId(id)),
                        title,
                        artist_id: artist_id.map(ArtistId),
                        year,
                        genre,
                        created_at,
                    })
                })
                .optional()?;

            Ok(album)
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    pub async fn create_album(
        &self,
        title: &str,
        artist_id: Option<ArtistId>,
        year: Option<i32>,
        genre: Option<String>,
    ) -> Result<AlbumId> {
        let title = title.to_string();
        let artist_id = artist_id.map(|id| id.0);
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            conn.execute(
                "INSERT INTO albums (title, artist_id, year, genre) VALUES (?, ?, ?, ?)",
                params![title, artist_id, year, genre],
            )?;
            let id = conn.last_insert_rowid();
            Ok(AlbumId(id))
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    pub async fn get_or_create_album(
        &self,
        title: &str,
        artist_id: Option<ArtistId>,
        year: Option<i32>,
        genre: Option<String>,
    ) -> Result<AlbumId> {
        if let Some(album) = self.get_album(title, artist_id).await? {
            return Ok(album.id.unwrap());
        }
        self.create_album(title, artist_id, year, genre).await
    }

    pub async fn get_all_albums(&self) -> Result<Vec<Album>> {
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            let mut stmt = conn.prepare(
                "SELECT id, title, artist_id, year, genre, created_at FROM albums ORDER BY title"
            )?;

            let albums = stmt
                .query_map([], |row| {
                    let id: i64 = row.get(0)?;
                    let title: String = row.get(1)?;
                    let artist_id: Option<i64> = row.get(2)?;
                    let year: Option<i32> = row.get(3)?;
                    let genre: Option<String> = row.get(4)?;
                    let created_at: Option<String> = row.get(5)?;

                    let created_at = created_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });

                    Ok(Album {
                        id: Some(AlbumId(id)),
                        title,
                        artist_id: artist_id.map(ArtistId),
                        year,
                        genre,
                        created_at,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(albums)
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    pub async fn get_albums_by_artist(&self, artist_id: ArtistId) -> Result<Vec<Album>> {
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            let mut stmt = conn.prepare(
                "SELECT id, title, artist_id, year, genre, created_at FROM albums WHERE artist_id = ? ORDER BY year, title"
            )?;

            let albums = stmt
                .query_map(params![artist_id.0], |row| {
                    let id: i64 = row.get(0)?;
                    let title: String = row.get(1)?;
                    let artist_id: Option<i64> = row.get(2)?;
                    let year: Option<i32> = row.get(3)?;
                    let genre: Option<String> = row.get(4)?;
                    let created_at: Option<String> = row.get(5)?;

                    let created_at = created_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });

                    Ok(Album {
                        id: Some(AlbumId(id)),
                        title,
                        artist_id: artist_id.map(ArtistId),
                        year,
                        genre,
                        created_at,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(albums)
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    // =========================================================================
    // Track operations
    // =========================================================================

    pub async fn get_track_by_path(&self, path: &Path) -> Result<Option<Track>> {
        let path = path.to_path_buf();
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            let mut stmt = conn.prepare(
                "SELECT id, file_path, title, artist_id, album_id, track_number, duration_ms, 
                        genre, year, sample_rate, channels, file_size, added_at, last_modified 
                 FROM tracks WHERE file_path = ?",
            )?;

            let track = stmt
                .query_row(params![path.to_string_lossy().as_ref()], |row| {
                    let id: i64 = row.get(0)?;
                    let file_path: String = row.get(1)?;
                    let title: String = row.get(2)?;
                    let artist_id: Option<i64> = row.get(3)?;
                    let album_id: Option<i64> = row.get(4)?;
                    let track_number: Option<i32> = row.get(5)?;
                    let duration_ms: i64 = row.get(6)?;
                    let genre: Option<String> = row.get(7)?;
                    let year: Option<i32> = row.get(8)?;
                    let sample_rate: Option<i32> = row.get(9)?;
                    let channels: Option<i32> = row.get(10)?;
                    let file_size: Option<i64> = row.get(11)?;
                    let added_at: Option<String> = row.get(12)?;
                    let last_modified: Option<String> = row.get(13)?;

                    let added_at = added_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });
                    let last_modified = last_modified.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });

                    Ok(Track {
                        id: Some(TrackId(id)),
                        file_path: PathBuf::from(file_path),
                        title,
                        artist_id: artist_id.map(ArtistId),
                        album_id: album_id.map(AlbumId),
                        track_number,
                        duration_ms,
                        genre,
                        year,
                        sample_rate,
                        channels,
                        file_size,
                        added_at,
                        last_modified,
                    })
                })
                .optional()?;

            Ok(track)
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    pub async fn create_track(&self, track: &Track) -> Result<TrackId> {
        let track = track.clone();
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            conn.execute(
                "INSERT INTO tracks (file_path, title, artist_id, album_id, track_number, 
                                     duration_ms, genre, year, sample_rate, channels, file_size, last_modified)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    track.file_path.to_string_lossy().as_ref(),
                    track.title,
                    track.artist_id.map(|id| id.0),
                    track.album_id.map(|id| id.0),
                    track.track_number,
                    track.duration_ms,
                    track.genre,
                    track.year,
                    track.sample_rate,
                    track.channels,
                    track.file_size,
                    track.last_modified.map(|dt| dt.to_rfc3339()),
                ],
            )?;
            let id = conn.last_insert_rowid();
            Ok(TrackId(id))
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    pub async fn update_track(&self, track: &Track) -> Result<()> {
        let track = track.clone();
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            conn.execute(
                "UPDATE tracks SET title = ?, artist_id = ?, album_id = ?, track_number = ?,
                                    duration_ms = ?, genre = ?, year = ?, sample_rate = ?,
                                    channels = ?, file_size = ?, last_modified = ?
                 WHERE id = ?",
                params![
                    track.title,
                    track.artist_id.map(|id| id.0),
                    track.album_id.map(|id| id.0),
                    track.track_number,
                    track.duration_ms,
                    track.genre,
                    track.year,
                    track.sample_rate,
                    track.channels,
                    track.file_size,
                    track.last_modified.map(|dt| dt.to_rfc3339()),
                    track.id.unwrap().0,
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    pub async fn delete_track(&self, path: &Path) -> Result<()> {
        let path = path.to_path_buf();
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            conn.execute(
                "DELETE FROM tracks WHERE file_path = ?",
                params![path.to_string_lossy().as_ref()],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    pub async fn get_all_tracks(&self) -> Result<Vec<Track>> {
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            let mut stmt = conn.prepare(
                "SELECT id, file_path, title, artist_id, album_id, track_number, duration_ms, 
                        genre, year, sample_rate, channels, file_size, added_at, last_modified 
                 FROM tracks ORDER BY title",
            )?;

            let tracks = stmt
                .query_map([], |row| {
                    let id: i64 = row.get(0)?;
                    let file_path: String = row.get(1)?;
                    let title: String = row.get(2)?;
                    let artist_id: Option<i64> = row.get(3)?;
                    let album_id: Option<i64> = row.get(4)?;
                    let track_number: Option<i32> = row.get(5)?;
                    let duration_ms: i64 = row.get(6)?;
                    let genre: Option<String> = row.get(7)?;
                    let year: Option<i32> = row.get(8)?;
                    let sample_rate: Option<i32> = row.get(9)?;
                    let channels: Option<i32> = row.get(10)?;
                    let file_size: Option<i64> = row.get(11)?;
                    let added_at: Option<String> = row.get(12)?;
                    let last_modified: Option<String> = row.get(13)?;

                    let added_at = added_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });
                    let last_modified = last_modified.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });

                    Ok(Track {
                        id: Some(TrackId(id)),
                        file_path: PathBuf::from(file_path),
                        title,
                        artist_id: artist_id.map(ArtistId),
                        album_id: album_id.map(AlbumId),
                        track_number,
                        duration_ms,
                        genre,
                        year,
                        sample_rate,
                        channels,
                        file_size,
                        added_at,
                        last_modified,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tracks)
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    pub async fn get_tracks_by_artist(&self, artist_id: ArtistId) -> Result<Vec<Track>> {
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            let mut stmt = conn.prepare(
                "SELECT id, file_path, title, artist_id, album_id, track_number, duration_ms, 
                        genre, year, sample_rate, channels, file_size, added_at, last_modified 
                 FROM tracks WHERE artist_id = ? ORDER BY album_id, track_number",
            )?;

            let tracks = stmt
                .query_map(params![artist_id.0], |row| {
                    let id: i64 = row.get(0)?;
                    let file_path: String = row.get(1)?;
                    let title: String = row.get(2)?;
                    let artist_id: Option<i64> = row.get(3)?;
                    let album_id: Option<i64> = row.get(4)?;
                    let track_number: Option<i32> = row.get(5)?;
                    let duration_ms: i64 = row.get(6)?;
                    let genre: Option<String> = row.get(7)?;
                    let year: Option<i32> = row.get(8)?;
                    let sample_rate: Option<i32> = row.get(9)?;
                    let channels: Option<i32> = row.get(10)?;
                    let file_size: Option<i64> = row.get(11)?;
                    let added_at: Option<String> = row.get(12)?;
                    let last_modified: Option<String> = row.get(13)?;

                    let added_at = added_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });
                    let last_modified = last_modified.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });

                    Ok(Track {
                        id: Some(TrackId(id)),
                        file_path: PathBuf::from(file_path),
                        title,
                        artist_id: artist_id.map(ArtistId),
                        album_id: album_id.map(AlbumId),
                        track_number,
                        duration_ms,
                        genre,
                        year,
                        sample_rate,
                        channels,
                        file_size,
                        added_at,
                        last_modified,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tracks)
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    pub async fn get_tracks_by_album(&self, album_id: AlbumId) -> Result<Vec<Track>> {
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            let mut stmt = conn.prepare(
                "SELECT id, file_path, title, artist_id, album_id, track_number, duration_ms, 
                        genre, year, sample_rate, channels, file_size, added_at, last_modified 
                 FROM tracks WHERE album_id = ? ORDER BY track_number",
            )?;

            let tracks = stmt
                .query_map(params![album_id.0], |row| {
                    let id: i64 = row.get(0)?;
                    let file_path: String = row.get(1)?;
                    let title: String = row.get(2)?;
                    let artist_id: Option<i64> = row.get(3)?;
                    let album_id: Option<i64> = row.get(4)?;
                    let track_number: Option<i32> = row.get(5)?;
                    let duration_ms: i64 = row.get(6)?;
                    let genre: Option<String> = row.get(7)?;
                    let year: Option<i32> = row.get(8)?;
                    let sample_rate: Option<i32> = row.get(9)?;
                    let channels: Option<i32> = row.get(10)?;
                    let file_size: Option<i64> = row.get(11)?;
                    let added_at: Option<String> = row.get(12)?;
                    let last_modified: Option<String> = row.get(13)?;

                    let added_at = added_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });
                    let last_modified = last_modified.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });

                    Ok(Track {
                        id: Some(TrackId(id)),
                        file_path: PathBuf::from(file_path),
                        title,
                        artist_id: artist_id.map(ArtistId),
                        album_id: album_id.map(AlbumId),
                        track_number,
                        duration_ms,
                        genre,
                        year,
                        sample_rate,
                        channels,
                        file_size,
                        added_at,
                        last_modified,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tracks)
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    pub async fn search_tracks(&self, query: &str) -> Result<Vec<Track>> {
        let query = format!("%{}%", query);
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            let mut stmt = conn.prepare(
                "SELECT id, file_path, title, artist_id, album_id, track_number, duration_ms, 
                        genre, year, sample_rate, channels, file_size, added_at, last_modified 
                 FROM tracks WHERE title LIKE ? ORDER BY title",
            )?;

            let tracks = stmt
                .query_map(params![query], |row| {
                    let id: i64 = row.get(0)?;
                    let file_path: String = row.get(1)?;
                    let title: String = row.get(2)?;
                    let artist_id: Option<i64> = row.get(3)?;
                    let album_id: Option<i64> = row.get(4)?;
                    let track_number: Option<i32> = row.get(5)?;
                    let duration_ms: i64 = row.get(6)?;
                    let genre: Option<String> = row.get(7)?;
                    let year: Option<i32> = row.get(8)?;
                    let sample_rate: Option<i32> = row.get(9)?;
                    let channels: Option<i32> = row.get(10)?;
                    let file_size: Option<i64> = row.get(11)?;
                    let added_at: Option<String> = row.get(12)?;
                    let last_modified: Option<String> = row.get(13)?;

                    let added_at = added_at.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });
                    let last_modified = last_modified.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .ok()
                            .map(|dt| dt.with_timezone(&Utc))
                    });

                    Ok(Track {
                        id: Some(TrackId(id)),
                        file_path: PathBuf::from(file_path),
                        title,
                        artist_id: artist_id.map(ArtistId),
                        album_id: album_id.map(AlbumId),
                        track_number,
                        duration_ms,
                        genre,
                        year,
                        sample_rate,
                        channels,
                        file_size,
                        added_at,
                        last_modified,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(tracks)
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }

    pub async fn get_tracks_count(&self) -> Result<usize> {
        let db = Arc::clone(&self.db);

        tokio::task::spawn_blocking(move || {
            let conn = db.connection();
            let conn = conn.lock().map_err(|e| {
                LibraryError::TaskJoinError(format!("Lock poisoned: {}", e))
            })?;
            let count: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |row| row.get(0))?;
            Ok(count as usize)
        })
        .await
        .map_err(|e| LibraryError::TaskJoinError(e.to_string()))?
    }
}
