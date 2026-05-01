use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension};

use crate::error::{LibraryError, Result};
use crate::migrations::MIGRATIONS;
use crate::models::{AlbumSummary, NewTrack, Track};
use crate::repository::LibraryRepository;

pub struct SqliteLibrary {
    conn: Mutex<Connection>,
    db_dir: PathBuf,
}

impl SqliteLibrary {
    pub fn open() -> Result<Self> {
        let db_dir = dirs::data_dir()
            .ok_or_else(|| LibraryError::InvalidData("no data dir".into()))?
            .join("gpui-test");
        std::fs::create_dir_all(&db_dir)?;

        let db_path = db_dir.join("library.db");
        let conn = Connection::open(&db_path)?;
        let lib = Self {
            conn: Mutex::new(conn),
            db_dir,
        };
        lib.run_migrations()?;
        Ok(lib)
    }

    pub fn open_at(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let db_dir = path.parent().unwrap_or(path).to_path_buf();
        std::fs::create_dir_all(&db_dir)?;
        let conn = Connection::open(path)?;
        let lib = Self {
            conn: Mutex::new(conn),
            db_dir,
        };
        lib.run_migrations()?;
        Ok(lib)
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.db_dir.join("covers")
    }

    fn run_migrations(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let user_version: i32 =
            tx.query_row("SELECT user_version FROM pragma_user_version", [], |row| row.get(0))?;

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
            .query_row(
                "SELECT id FROM artists WHERE name = ?1",
                [name],
                |row| row.get::<_, i64>(0),
            )
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
        cover_art_path: Option<&str>,
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
            if cover_art_path.is_some() {
                tx.execute(
                    "UPDATE albums SET cover_art_path = ?1 WHERE id = ?2",
                    rusqlite::params![cover_art_path, id],
                )?;
            }
            tx.commit()?;
            return Ok(id);
        }
        tx.execute(
            "INSERT INTO albums (title, year, cover_art_path) VALUES (?1, ?2, ?3)",
            rusqlite::params![title, year, cover_art_path],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    fn set_album_artists(&self, album_id: i64, artist_ids: &[(i64, i32)]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM album_artists WHERE album_id = ?1",
            [album_id],
        )?;
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
        let cover_path = track
            .cover_art
            .as_ref()
            .and_then(|data| save_cover_art(&self.cache_dir(), data).ok());

        let existing_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM tracks WHERE path = ?1",
                [&track.path],
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
                    cover_art_path = ?7
                WHERE id = ?8"#,
                rusqlite::params![
                    title,
                    album_id,
                    track_number,
                    disc_number,
                    duration_ms,
                    track.year,
                    cover_path,
                    id,
                ],
            )?;
            id
        } else {
            tx.execute(
                r#"INSERT INTO tracks
                    (path, title, album_id, track_number, disc_number, duration_ms, year, cover_art_path)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
                rusqlite::params![
                    track.path,
                    title,
                    album_id,
                    track_number,
                    disc_number,
                    duration_ms,
                    track.year,
                    cover_path,
                ],
            )?;
            tx.last_insert_rowid()
        };

        tx.execute(
            "DELETE FROM track_artists WHERE track_id = ?1",
            [track_id],
        )?;
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
                a.cover_art_path,
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
                cover_art_path: row.get(3)?,
                artist_name: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
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
                cover_art_path
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
                cover_art_path: row.get(8)?,
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
                t.cover_art_path
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
                cover_art_path: row.get(8)?,
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
        tx.commit()?;
        Ok(())
    }

    fn has_tracks(&self) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tracks",
            [],
            |row| row.get(0),
        )?;
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
        tx.commit()?;
        Ok(())
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

fn save_cover_art(cache_dir: &Path, data: &[u8]) -> Result<String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    std::fs::create_dir_all(cache_dir)?;
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    let file_name = format!("{:x}.jpg", hasher.finish());
    let path = cache_dir.join(&file_name);
    if !path.exists() {
        std::fs::write(&path, data)?;
    }
    Ok(path.to_string_lossy().into_owned())
}
