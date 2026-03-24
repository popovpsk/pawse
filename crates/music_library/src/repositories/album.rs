//! Album repository for CRUD operations.

use crate::models::{Album, AlbumId, AlbumType};
use crate::repositories::Result;
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};

/// Repository for managing albums in the database.
pub struct AlbumRepository<'conn> {
    conn: &'conn Connection,
}

impl<'conn> AlbumRepository<'conn> {
    pub fn new(conn: &'conn Connection) -> Self {
        Self { conn }
    }

    /// Create a new album.
    pub fn create(&self, album: &Album) -> Result<AlbumId> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO albums (title, year, genre, cover_art_path, total_discs, type, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;

        let now = Utc::now().to_rfc3339();
        let cover_path_str = album
            .cover_art_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());

        stmt.execute(params![
            album.title,
            album.year,
            album.genre,
            cover_path_str,
            album.total_discs,
            album.album_type.as_str(),
            now,
            now,
        ])?;

        let id = self.conn.last_insert_rowid();
        Ok(AlbumId(id))
    }

    /// Get album by ID.
    pub fn get_by_id(&self, id: AlbumId) -> Result<Option<Album>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, title, year, genre, cover_art_path, total_discs, type, created_at, updated_at
             FROM albums WHERE id = ?1",
        )?;

        let album = stmt
            .query_row(params![id.0], |row| {
                Ok(Album {
                    id: Some(AlbumId(row.get(0)?)),
                    title: row.get(1)?,
                    year: row.get(2)?,
                    genre: row.get(3)?,
                    cover_art_path: row.get::<_, Option<String>>(4)?.map(|s| s.into()),
                    total_discs: row.get(5)?,
                    album_type: row
                        .get::<_, String>(6)?
                        .as_str()
                        .parse()
                        .unwrap_or(AlbumType::Unknown),
                    created_at: row
                        .get::<_, Option<String>>(7)
                        .ok()
                        .flatten()
                        .and_then(|s| {
                            chrono::DateTime::parse_from_rfc3339(&s).ok().map(
                                |d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc),
                            )
                        }),
                    updated_at: row
                        .get::<_, Option<String>>(8)
                        .ok()
                        .flatten()
                        .and_then(|s| {
                            chrono::DateTime::parse_from_rfc3339(&s).ok().map(
                                |d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc),
                            )
                        }),
                })
            })
            .optional()?;

        Ok(album)
    }

    /// Get album by title and year.
    pub fn get_by_title_and_year(&self, title: &str, year: Option<i32>) -> Result<Option<Album>> {
        let query = match year {
            Some(_) => "SELECT id, title, year, genre, cover_art_path, total_discs, type, created_at, updated_at
                        FROM albums WHERE title = ?1 AND year = ?2",
            None => "SELECT id, title, year, genre, cover_art_path, total_discs, type, created_at, updated_at
                     FROM albums WHERE title = ?1 AND year IS NULL",
        };

        let mut stmt = self.conn.prepare_cached(query)?;

        let album =
            match year {
                Some(y) => stmt
                    .query_row(params![title, y], |row| {
                        Ok(Album {
                            id: Some(AlbumId(row.get(0)?)),
                            title: row.get(1)?,
                            year: row.get(2)?,
                            genre: row.get(3)?,
                            cover_art_path: row.get::<_, Option<String>>(4)?.map(|s| s.into()),
                            total_discs: row.get(5)?,
                            album_type: row
                                .get::<_, String>(6)?
                                .as_str()
                                .parse()
                                .unwrap_or(AlbumType::Unknown),
                            created_at: row.get::<_, Option<String>>(7).ok().flatten().and_then(
                                |s| {
                                    chrono::DateTime::parse_from_rfc3339(&s).ok().map(
                                        |d: chrono::DateTime<chrono::FixedOffset>| {
                                            d.with_timezone(&Utc)
                                        },
                                    )
                                },
                            ),
                            updated_at: row.get::<_, Option<String>>(8).ok().flatten().and_then(
                                |s| {
                                    chrono::DateTime::parse_from_rfc3339(&s).ok().map(
                                        |d: chrono::DateTime<chrono::FixedOffset>| {
                                            d.with_timezone(&Utc)
                                        },
                                    )
                                },
                            ),
                        })
                    })
                    .optional()?,
                None => stmt
                    .query_row(params![title], |row| {
                        Ok(Album {
                            id: Some(AlbumId(row.get(0)?)),
                            title: row.get(1)?,
                            year: row.get(2)?,
                            genre: row.get(3)?,
                            cover_art_path: row.get::<_, Option<String>>(4)?.map(|s| s.into()),
                            total_discs: row.get(5)?,
                            album_type: row
                                .get::<_, String>(6)?
                                .as_str()
                                .parse()
                                .unwrap_or(AlbumType::Unknown),
                            created_at: row.get::<_, Option<String>>(7).ok().flatten().and_then(
                                |s| {
                                    chrono::DateTime::parse_from_rfc3339(&s).ok().map(
                                        |d: chrono::DateTime<chrono::FixedOffset>| {
                                            d.with_timezone(&Utc)
                                        },
                                    )
                                },
                            ),
                            updated_at: row.get::<_, Option<String>>(8).ok().flatten().and_then(
                                |s| {
                                    chrono::DateTime::parse_from_rfc3339(&s).ok().map(
                                        |d: chrono::DateTime<chrono::FixedOffset>| {
                                            d.with_timezone(&Utc)
                                        },
                                    )
                                },
                            ),
                        })
                    })
                    .optional()?,
            };

        Ok(album)
    }

    /// Update an existing album.
    pub fn update(&self, album: &Album) -> Result<()> {
        let Some(id) = album.id else {
            return Err(crate::repositories::RepositoryError::Validation(
                "Album must have an ID".into(),
            ));
        };

        let mut stmt = self.conn.prepare_cached(
            "UPDATE albums SET title = ?1, year = ?2, genre = ?3, cover_art_path = ?4,
             total_discs = ?5, type = ?6, updated_at = datetime('now') WHERE id = ?7",
        )?;

        let cover_path_str = album
            .cover_art_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());

        stmt.execute(params![
            album.title,
            album.year,
            album.genre,
            cover_path_str,
            album.total_discs,
            album.album_type.as_str(),
            id.0,
        ])?;

        Ok(())
    }

    /// Delete an album by ID.
    pub fn delete(&self, id: AlbumId) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare_cached("DELETE FROM albums WHERE id = ?1")?;
        let rows_affected = stmt.execute(params![id.0])?;
        Ok(rows_affected > 0)
    }

    /// List all albums with pagination.
    pub fn list(&self, limit: i32, offset: i32) -> Result<Vec<Album>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, title, year, genre, cover_art_path, total_discs, type, created_at, updated_at
             FROM albums ORDER BY title LIMIT ?1 OFFSET ?2",
        )?;

        let albums = stmt
            .query_map(params![limit, offset], |row| {
                Ok(Album {
                    id: Some(AlbumId(row.get(0)?)),
                    title: row.get(1)?,
                    year: row.get(2)?,
                    genre: row.get(3)?,
                    cover_art_path: row.get::<_, Option<String>>(4)?.map(|s| s.into()),
                    total_discs: row.get(5)?,
                    album_type: row
                        .get::<_, String>(6)?
                        .as_str()
                        .parse()
                        .unwrap_or(AlbumType::Unknown),
                    created_at: row
                        .get::<_, Option<String>>(7)
                        .ok()
                        .flatten()
                        .and_then(|s| {
                            chrono::DateTime::parse_from_rfc3339(&s).ok().map(
                                |d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc),
                            )
                        }),
                    updated_at: row
                        .get::<_, Option<String>>(8)
                        .ok()
                        .flatten()
                        .and_then(|s| {
                            chrono::DateTime::parse_from_rfc3339(&s).ok().map(
                                |d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc),
                            )
                        }),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(albums)
    }
}
