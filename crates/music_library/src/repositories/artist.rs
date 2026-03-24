//! Artist repository for CRUD operations.

use crate::models::{Artist, ArtistId, ArtistType};
use crate::repositories::{RepositoryError, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};

/// Repository for managing artists in the database.
pub struct ArtistRepository<'conn> {
    conn: &'conn Connection,
}

impl<'conn> ArtistRepository<'conn> {
    pub fn new(conn: &'conn Connection) -> Self {
        Self { conn }
    }

    /// Create a new artist.
    pub fn create(&self, artist: &Artist) -> Result<ArtistId> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO artists (name, disambiguation, type, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;

        let now = Utc::now().to_rfc3339();
        stmt.execute(params![
            artist.name,
            artist.disambiguation,
            artist.artist_type.as_str(),
            now,
            now,
        ])?;

        let id = self.conn.last_insert_rowid();
        Ok(ArtistId(id))
    }

    /// Get artist by ID.
    pub fn get_by_id(&self, id: ArtistId) -> Result<Option<Artist>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, name, disambiguation, type, created_at, updated_at
             FROM artists WHERE id = ?1",
        )?;

        let artist = stmt.query_row(params![id.0], |row| {
            Ok(Artist {
                id: Some(ArtistId(row.get(0)?)),
                name: row.get(1)?,
                disambiguation: row.get(2)?,
                artist_type: row.get::<_, String>(3)?.as_str().parse().unwrap_or(ArtistType::Unknown),
                created_at: row.get::<_, Option<String>>(4).ok().flatten().and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc))),
                updated_at: row.get::<_, Option<String>>(5).ok().flatten().and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc))),
            })
        }).optional()?;

        Ok(artist)
    }

    /// Get artist by name (and optional disambiguation).
    pub fn get_by_name(&self, name: &str, disambiguation: Option<&str>) -> Result<Option<Artist>> {
        let query = match disambiguation {
            Some(_disamb) => {
                "SELECT id, name, disambiguation, type, created_at, updated_at
                 FROM artists WHERE name = ?1 AND (disambiguation = ?2 OR (disambiguation IS NULL AND ?2 IS NULL))"
            }
            None => {
                "SELECT id, name, disambiguation, type, created_at, updated_at
                 FROM artists WHERE name = ?1 AND disambiguation IS NULL"
            }
        };

        let mut stmt = self.conn.prepare_cached(query)?;

        let artist = match disambiguation {
            Some(disamb) => stmt.query_row(params![name, disamb], |row| {
                Ok(Artist {
                    id: Some(ArtistId(row.get(0)?)),
                    name: row.get(1)?,
                    disambiguation: row.get(2)?,
                    artist_type: row.get::<_, String>(3)?.as_str().parse().unwrap_or(ArtistType::Unknown),
                    created_at: row.get::<_, Option<String>>(4).ok().flatten().and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc))),
                    updated_at: row.get::<_, Option<String>>(5).ok().flatten().and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc))),
                })
            }).optional()?,
            None => stmt.query_row(params![name], |row| {
                Ok(Artist {
                    id: Some(ArtistId(row.get(0)?)),
                    name: row.get(1)?,
                    disambiguation: row.get(2)?,
                    artist_type: row.get::<_, String>(3)?.as_str().parse().unwrap_or(ArtistType::Unknown),
                    created_at: row.get::<_, Option<String>>(4).ok().flatten().and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc))),
                    updated_at: row.get::<_, Option<String>>(5).ok().flatten().and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc))),
                })
            }).optional()?,
        };

        Ok(artist)
    }

    /// Find or create an artist by name.
    pub fn find_or_create(&self, name: &str, disambiguation: Option<&str>) -> Result<ArtistId> {
        if let Some(artist) = self.get_by_name(name, disambiguation)? {
            return Ok(artist.id.unwrap());
        }

        let mut artist = Artist::new(name.to_string());
        if let Some(disamb) = disambiguation {
            artist = artist.with_disambiguation(disamb);
        }
        self.create(&artist)
    }

    /// Update an existing artist.
    pub fn update(&self, artist: &Artist) -> Result<()> {
        let Some(id) = artist.id else {
            return Err(RepositoryError::Validation("Artist must have an ID".into()));
        };

        let mut stmt = self.conn.prepare_cached(
            "UPDATE artists SET name = ?1, disambiguation = ?2, type = ?3, updated_at = datetime('now')
             WHERE id = ?4",
        )?;

        stmt.execute(params![
            artist.name,
            artist.disambiguation,
            artist.artist_type.as_str(),
            id.0,
        ])?;

        Ok(())
    }

    /// Delete an artist by ID.
    pub fn delete(&self, id: ArtistId) -> Result<bool> {
        let mut stmt = self.conn.prepare_cached("DELETE FROM artists WHERE id = ?1")?;
        let rows_affected = stmt.execute(params![id.0])?;
        Ok(rows_affected > 0)
    }

    /// List all artists with pagination.
    pub fn list(&self, limit: i32, offset: i32) -> Result<Vec<Artist>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, name, disambiguation, type, created_at, updated_at
             FROM artists ORDER BY name LIMIT ?1 OFFSET ?2",
        )?;

        let artists = stmt
            .query_map(params![limit, offset], |row| {
                Ok(Artist {
                    id: Some(ArtistId(row.get(0)?)),
                    name: row.get(1)?,
                    disambiguation: row.get(2)?,
                    artist_type: row.get::<_, String>(3)?.as_str().parse().unwrap_or(ArtistType::Unknown),
                    created_at: row.get::<_, Option<String>>(4).ok().flatten().and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc))),
                    updated_at: row.get::<_, Option<String>>(5).ok().flatten().and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc))),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(artists)
    }

    /// Search artists by name prefix.
    pub fn search(&self, query: &str, limit: i32) -> Result<Vec<Artist>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, name, disambiguation, type, created_at, updated_at
             FROM artists WHERE name LIKE ?1 ORDER BY name LIMIT ?2",
        )?;

        let search_pattern = format!("%{}%", query);
        let artists = stmt
            .query_map(params![search_pattern, limit], |row| {
                Ok(Artist {
                    id: Some(ArtistId(row.get(0)?)),
                    name: row.get(1)?,
                    disambiguation: row.get(2)?,
                    artist_type: row.get::<_, String>(3)?.as_str().parse().unwrap_or(ArtistType::Unknown),
                    created_at: row.get::<_, Option<String>>(4).ok().flatten().and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc))),
                    updated_at: row.get::<_, Option<String>>(5).ok().flatten().and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d: chrono::DateTime<chrono::FixedOffset>| d.with_timezone(&Utc))),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(artists)
    }
}
