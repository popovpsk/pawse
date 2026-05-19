use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension};

use crate::error::{LibraryError, Result};
use crate::migrations::MIGRATIONS;
use crate::models::{AlbumSearchEntry, AlbumSummary, CoverArt, NewTrack, Track};
use crate::repository::LibraryRepository;

pub struct SqliteLibrary {
    conn: Mutex<Connection>,
}

impl SqliteLibrary {
    pub fn open() -> Result<Self> {
        let db_dir = dirs::data_dir()
            .ok_or_else(|| LibraryError::InvalidData("no data dir".into()))?
            .join("pawse");
        std::fs::create_dir_all(&db_dir)?;

        let db_path = db_dir.join("library.db");
        if db_path.exists() {
            let check = Connection::open(&db_path);
            if let Ok(check_conn) = check {
                let has_cover_art: bool = check_conn
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='cover_art'",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|c| c > 0)
                    .unwrap_or(false);
                drop(check_conn);
                if !has_cover_art {
                    let _ = std::fs::remove_file(&db_path);
                }
            }
        }

        let conn = Connection::open(&db_path)?;
        let lib = Self {
            conn: Mutex::new(conn),
        };
        lib.run_migrations()?;
        Ok(lib)
    }

    pub fn open_at(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let db_dir = path.parent().unwrap_or(path);
        std::fs::create_dir_all(db_dir)?;
        let conn = Connection::open(path)?;
        let lib = Self {
            conn: Mutex::new(conn),
        };
        lib.run_migrations()?;
        Ok(lib)
    }

    fn run_migrations(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let user_version: i32 =
            tx.query_row("SELECT user_version FROM pragma_user_version", [], |row| {
                row.get(0)
            })?;

        for (version, sql) in MIGRATIONS.iter() {
            if user_version < *version {
                tx.execute_batch(sql)?;
                tx.pragma_update(None, "user_version", *version)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn get_or_insert_artist(&self, tx: &rusqlite::Transaction, name: &str) -> Result<i64> {
        let sort_name = compute_sort_name(name);
        if let Some(id) = tx
            .query_row("SELECT id FROM artists WHERE name = ?1", [name], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?
        {
            return Ok(id);
        }
        tx.execute(
            "INSERT INTO artists (name, sort_name) VALUES (?1, ?2)",
            [name, &sort_name],
        )?;
        Ok(tx.last_insert_rowid())
    }
}

impl LibraryRepository for SqliteLibrary {
    fn upsert_artist(&self, name: &str) -> Result<i64> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let id = self.get_or_insert_artist(&tx, name)?;
        tx.commit()?;
        Ok(id)
    }

    fn upsert_album(
        &self,
        title: &str,
        year: Option<i32>,
        cover_art_id: Option<i64>,
    ) -> Result<i64> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        if let Some(id) = tx
            .query_row(
                "SELECT id FROM albums WHERE title = ?1 AND (year IS ?2)",
                rusqlite::params![title, year],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
        {
            if cover_art_id.is_some() {
                tx.execute(
                    "UPDATE albums SET cover_art_id = ?1 WHERE id = ?2",
                    rusqlite::params![cover_art_id, id],
                )?;
            }
            tx.commit()?;
            return Ok(id);
        }
        tx.execute(
            "INSERT INTO albums (title, year, cover_art_id) VALUES (?1, ?2, ?3)",
            rusqlite::params![title, year, cover_art_id],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    fn set_album_artists(&self, album_id: i64, artist_ids: &[(i64, i32)]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM album_artists WHERE album_id = ?1", [album_id])?;
        for (artist_id, position) in artist_ids {
            tx.execute(
                "INSERT INTO album_artists (album_id, artist_id, position) VALUES (?1, ?2, ?3)",
                [album_id, *artist_id, *position as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn upsert_track(
        &self,
        track: &NewTrack,
        album_id: Option<i64>,
        artist_ids: &[(i64, i32)],
    ) -> Result<i64> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        let title = track
            .title
            .clone()
            .unwrap_or_else(|| fallback_title_from_path(&track.path));
        let track_number = track.track_number.map(|n| n as i32);
        let disc_number = track.disc_number.unwrap_or(1) as i32;
        let duration_ms = track.duration_ms.map(|n| n as i64);
        let start_offset_ms = track.start_offset_ms.unwrap_or(0) as i32;

        let existing_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM tracks WHERE path = ?1 AND start_offset_ms = ?2",
                rusqlite::params![track.path, start_offset_ms],
                |row| row.get(0),
            )
            .optional()?;

        let track_id = if let Some(id) = existing_id {
            tx.execute(
                r#"UPDATE tracks SET
                    title = ?1,
                    album_id = ?2,
                    track_number = ?3,
                    disc_number = ?4,
                    duration_ms = ?5,
                    year = ?6,
                    cover_art_id = ?7,
                    start_offset_ms = ?8
                WHERE id = ?9"#,
                rusqlite::params![
                    title,
                    album_id,
                    track_number,
                    disc_number,
                    duration_ms,
                    track.year,
                    track.cover_art_id,
                    start_offset_ms,
                    id,
                ],
            )?;
            id
        } else {
            tx.execute(
                r#"INSERT INTO tracks
                    (path, title, album_id, track_number, disc_number, duration_ms, year, cover_art_id, start_offset_ms)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
                rusqlite::params![
                    track.path,
                    title,
                    album_id,
                    track_number,
                    disc_number,
                    duration_ms,
                    track.year,
                    track.cover_art_id,
                    start_offset_ms,
                ],
            )?;
            tx.last_insert_rowid()
        };

        tx.execute("DELETE FROM track_artists WHERE track_id = ?1", [track_id])?;
        for (artist_id, position) in artist_ids {
            tx.execute(
                "INSERT INTO track_artists (track_id, artist_id, role, position) VALUES (?1, ?2, 'main', ?3)",
                [track_id, *artist_id, *position as i64],
            )?;
        }

        tx.commit()?;
        Ok(track_id)
    }

    fn albums(&self) -> Result<Vec<AlbumSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT
                a.id,
                a.title,
                a.year,
                a.cover_art_id,
                art.name
            FROM albums a
            LEFT JOIN album_artists aa ON aa.album_id = a.id AND aa.position = 0
            LEFT JOIN artists art ON art.id = aa.artist_id
            ORDER BY COALESCE(NULLIF(art.sort_name, ''), art.name), a.year, a.title
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(AlbumSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                year: row.get(2)?,
                cover_art_id: row.get(3)?,
                artist_name: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn album_search_entries(&self) -> Result<Vec<AlbumSearchEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT
                a.id,
                a.title || ' ' ||
                COALESCE(artists_concat.names, '') || ' ' ||
                COALESCE(tracks_concat.titles, '')
            FROM albums a
            LEFT JOIN (
                SELECT aa.album_id AS album_id, GROUP_CONCAT(art.name, ' ') AS names
                FROM album_artists aa
                JOIN artists art ON art.id = aa.artist_id
                GROUP BY aa.album_id
            ) artists_concat ON artists_concat.album_id = a.id
            LEFT JOIN (
                SELECT album_id, GROUP_CONCAT(title, ' ') AS titles
                FROM tracks
                WHERE album_id IS NOT NULL AND title IS NOT NULL
                GROUP BY album_id
            ) tracks_concat ON tracks_concat.album_id = a.id
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(AlbumSearchEntry {
                album_id: row.get(0)?,
                haystack: row.get(1)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn tracks_for_album(&self, album_id: i64) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT
                id,
                path,
                title,
                album_id,
                track_number,
                disc_number,
                duration_ms,
                year,
                cover_art_id,
                start_offset_ms
            FROM tracks
            WHERE album_id = ?1
            ORDER BY disc_number, track_number, title
            "#,
        )?;
        let rows = stmt.query_map([album_id], |row| {
            Ok(Track {
                id: row.get(0)?,
                path: row.get(1)?,
                title: row.get(2)?,
                album_id: row.get(3)?,
                track_number: row.get(4)?,
                disc_number: row.get(5)?,
                duration_ms: row.get(6)?,
                year: row.get(7)?,
                cover_art_id: row.get(8)?,
                start_offset_ms: row.get(9)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn track_artists(&self, track_id: i64) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT a.name
            FROM artists a
            JOIN track_artists ta ON ta.artist_id = a.id
            WHERE ta.track_id = ?1
            ORDER BY ta.position
            "#,
        )?;
        let rows = stmt.query_map([track_id], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn album_title(&self, album_id: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT title FROM albums WHERE id = ?1")?;
        let title = stmt
            .query_row([album_id], |row| row.get::<_, Option<String>>(0))
            .optional()?
            .flatten();
        Ok(title)
    }

    fn search(&self, query: &str) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            r#"
            SELECT
                t.id,
                t.path,
                t.title,
                t.album_id,
                t.track_number,
                t.disc_number,
                t.duration_ms,
                t.year,
                t.cover_art_id,
                t.start_offset_ms
            FROM tracks t
            LEFT JOIN albums al ON al.id = t.album_id
            WHERE t.title LIKE ?1
               OR al.title LIKE ?1
            ORDER BY t.title
            "#,
        )?;
        let rows = stmt.query_map([&pattern], |row| {
            Ok(Track {
                id: row.get(0)?,
                path: row.get(1)?,
                title: row.get(2)?,
                album_id: row.get(3)?,
                track_number: row.get(4)?,
                disc_number: row.get(5)?,
                duration_ms: row.get(6)?,
                year: row.get(7)?,
                cover_art_id: row.get(8)?,
                start_offset_ms: row.get(9)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn clear(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM track_artists", [])?;
        tx.execute("DELETE FROM album_artists", [])?;
        tx.execute("DELETE FROM tracks", [])?;
        tx.execute("DELETE FROM albums", [])?;
        tx.execute("DELETE FROM artists", [])?;
        tx.execute("DELETE FROM cover_art", [])?;
        tx.commit()?;
        Ok(())
    }

    fn has_tracks(&self) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |row| row.get(0))?;
        Ok(count > 0)
    }

    fn delete_orphaned_albums_and_artists(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM albums WHERE id NOT IN (SELECT DISTINCT album_id FROM tracks WHERE album_id IS NOT NULL)",
            [],
        )?;
        tx.execute(
            "DELETE FROM artists WHERE id NOT IN (SELECT DISTINCT artist_id FROM album_artists)
             AND id NOT IN (SELECT DISTINCT artist_id FROM track_artists)",
            [],
        )?;
        tx.execute(
            "DELETE FROM cover_art WHERE id NOT IN (SELECT DISTINCT cover_art_id FROM albums WHERE cover_art_id IS NOT NULL)
             AND id NOT IN (SELECT DISTINCT cover_art_id FROM tracks WHERE cover_art_id IS NOT NULL)",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn save_cover_art(&self, data: &[u8]) -> Result<i64> {
        let hash = compute_sha256(data);

        {
            let conn = self.conn.lock().unwrap();
            if let Some(id) = conn
                .query_row("SELECT id FROM cover_art WHERE hash = ?1", [&hash], |row| {
                    row.get::<_, i64>(0)
                })
                .optional()?
            {
                return Ok(id);
            }
        }

        let thumbnails = crate::thumbnail::generate_thumbnails(data)?;

        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO cover_art (hash, small, large) VALUES (?1, ?2, ?3)",
            rusqlite::params![hash, thumbnails.small, thumbnails.large],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    fn get_cover_art(&self, id: i64) -> Result<Option<CoverArt>> {
        let conn = self.conn.lock().unwrap();
        let result = conn
            .query_row(
                "SELECT id, small, large FROM cover_art WHERE id = ?1",
                [id],
                |row| {
                    Ok(CoverArt {
                        id: row.get(0)?,
                        small: row.get(1)?,
                        large: row.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    fn get_cover_art_small(&self, id: i64) -> Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        let result = conn
            .query_row("SELECT small FROM cover_art WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(result)
    }

    fn get_cover_art_large(&self, id: i64) -> Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        let result = conn
            .query_row("SELECT large FROM cover_art WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(result)
    }

    fn album_has_artists(&self, album_id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM album_artists WHERE album_id = ?1",
            [album_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
}

fn compute_sort_name(name: &str) -> String {
    let trimmed = name.trim();
    let lower = trimmed.to_lowercase();
    if let Some(rest) = lower.strip_prefix("the ") {
        return format!("{}, the", rest);
    }
    if let Some(rest) = lower.strip_prefix("a ") {
        return format!("{}, a", rest);
    }
    trimmed.to_string()
}

fn fallback_title_from_path(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

fn compute_sha256(data: &[u8]) -> String {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(data);
    format!("{:x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_sort_name_the() {
        assert_eq!(compute_sort_name("The Beatles"), "beatles, the");
    }

    #[test]
    fn test_compute_sort_name_a() {
        assert_eq!(
            compute_sort_name("A Tribe Called Quest"),
            "tribe called quest, a"
        );
    }

    #[test]
    fn test_compute_sort_name_no_article() {
        assert_eq!(compute_sort_name("Radiohead"), "Radiohead");
    }

    #[test]
    fn test_compute_sort_name_whitespace() {
        assert_eq!(compute_sort_name("  The Who  "), "who, the");
    }

    #[test]
    fn test_compute_sort_name_lowercase_article() {
        assert_eq!(compute_sort_name("the national"), "national, the");
    }

    #[test]
    fn test_compute_sort_name_empty() {
        assert_eq!(compute_sort_name(""), "");
    }

    #[test]
    fn test_fallback_title_from_path_basic() {
        assert_eq!(fallback_title_from_path("/music/song.flac"), "song");
    }

    #[test]
    fn test_fallback_title_from_path_no_ext() {
        assert_eq!(fallback_title_from_path("/music/song"), "song");
    }

    #[test]
    fn test_fallback_title_from_path_root() {
        assert_eq!(fallback_title_from_path("song.flac"), "song");
    }

    #[test]
    fn test_fallback_title_from_path_multiple_ext() {
        assert_eq!(fallback_title_from_path("/music/song.tar.gz"), "song.tar");
    }

    #[test]
    fn test_fallback_title_from_path_empty() {
        assert_eq!(fallback_title_from_path(""), "");
    }
}
