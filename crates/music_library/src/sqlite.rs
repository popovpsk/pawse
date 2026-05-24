use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension};

use crate::error::{LibraryError, Result};
use crate::migrations::MIGRATIONS;
use crate::models::{
    AlbumSearchEntry, AlbumSummary, ArtistSummary, CoverArt, NewTrack, PlaylistSummary,
    PlaylistTrackRef, Track,
};
use crate::repository::LibraryRepository;

const TRACK_COLUMNS: &str = "id, path, title, album_id, track_number, disc_number, \
    duration_ms, year, cover_art_id, start_offset_ms, liked";

fn map_track_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
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
        liked: row.get::<_, i64>(10)? != 0,
    })
}

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
                let has_liked: bool = check_conn
                    .query_row(
                        "SELECT COUNT(*) FROM pragma_table_info('tracks') WHERE name='liked'",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|c| c > 0)
                    .unwrap_or(false);
                let has_playlists: bool = check_conn
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='playlists'",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|c| c > 0)
                    .unwrap_or(false);
                let has_playlist_unique: bool = check_conn
                    .query_row(
                        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_playlist_tracks_pair'",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|c| c > 0)
                    .unwrap_or(false);
                drop(check_conn);
                if !has_cover_art || !has_liked || !has_playlists || !has_playlist_unique {
                    let _ = std::fs::remove_file(&db_path);
                }
            }
        }

        let conn = Connection::open(&db_path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
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
        conn.pragma_update(None, "foreign_keys", "ON")?;
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
                    cover_art_id = COALESCE(?7, cover_art_id),
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
                COALESCE(track_artists_concat.names, '') || ' ' ||
                COALESCE(tracks_concat.titles, '')
            FROM albums a
            LEFT JOIN (
                SELECT aa.album_id AS album_id, GROUP_CONCAT(art.name, ' ') AS names
                FROM album_artists aa
                JOIN artists art ON art.id = aa.artist_id
                GROUP BY aa.album_id
            ) artists_concat ON artists_concat.album_id = a.id
            LEFT JOIN (
                SELECT t.album_id AS album_id, GROUP_CONCAT(DISTINCT art.name) AS names
                FROM tracks t
                JOIN track_artists ta ON ta.track_id = t.id
                JOIN artists art ON art.id = ta.artist_id
                WHERE t.album_id IS NOT NULL
                GROUP BY t.album_id
            ) track_artists_concat ON track_artists_concat.album_id = a.id
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
        let sql = format!(
            "SELECT {TRACK_COLUMNS} FROM tracks WHERE album_id = ?1 \
             ORDER BY disc_number, track_number, title",
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([album_id], map_track_row)?;
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

    fn track_artists_with_ids(&self, track_id: i64) -> Result<Vec<(i64, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT a.id, a.name
            FROM artists a
            JOIN track_artists ta ON ta.artist_id = a.id
            WHERE ta.track_id = ?1
            ORDER BY ta.position
            "#,
        )?;
        let rows = stmt.query_map([track_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
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
        let sql = format!(
            "SELECT {} FROM tracks t \
             LEFT JOIN albums al ON al.id = t.album_id \
             WHERE t.title LIKE ?1 OR al.title LIKE ?1 \
             ORDER BY t.title",
            TRACK_COLUMNS
                .split(", ")
                .map(|c| format!("t.{c}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([&pattern], map_track_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn clear(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        // playlist_tracks references tracks by FK; clear before tracks. We
        // keep playlist definitions: a user's playlists survive a rescan.
        tx.execute("DELETE FROM playlist_tracks", [])?;
        tx.execute("DELETE FROM track_artists", [])?;
        tx.execute("DELETE FROM album_artists", [])?;
        tx.execute("DELETE FROM tracks", [])?;
        tx.execute("DELETE FROM albums", [])?;
        tx.execute("DELETE FROM artists", [])?;
        // cover_art rows are NOT deleted here; they are cleaned up after the
        // rescan via delete_orphaned_albums_and_artists so that the same SHA-256
        // hash always maps back to the same id. This keeps cover_art_ids valid
        // in PlaybackQueue Track objects across rescans.
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

    fn artists(&self) -> Result<Vec<ArtistSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT a.id, a.name, a.sort_name, COUNT(DISTINCT ta.track_id) AS track_count
            FROM artists a
            JOIN track_artists ta ON ta.artist_id = a.id
            GROUP BY a.id
            HAVING track_count > 0
            ORDER BY a.sort_name COLLATE NOCASE, a.name COLLATE NOCASE
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ArtistSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                sort_name: row.get(2)?,
                track_count: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn tracks_by_artist(&self, artist_id: i64) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT DISTINCT {} FROM tracks t \
             JOIN track_artists ta ON ta.track_id = t.id \
             LEFT JOIN albums al ON al.id = t.album_id \
             WHERE ta.artist_id = ?1 \
             ORDER BY COALESCE(al.year, 0), al.title COLLATE NOCASE, t.disc_number, t.track_number, t.title",
            TRACK_COLUMNS
                .split(", ")
                .map(|c| format!("t.{c}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([artist_id], map_track_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn liked_tracks(&self) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {} FROM tracks t \
             LEFT JOIN albums al ON al.id = t.album_id \
             LEFT JOIN album_artists aa ON aa.album_id = al.id AND aa.position = 0 \
             LEFT JOIN artists art ON art.id = aa.artist_id \
             WHERE t.liked = 1 \
             ORDER BY art.sort_name COLLATE NOCASE, COALESCE(al.year, 0), al.title COLLATE NOCASE, t.disc_number, t.track_number, t.title",
            TRACK_COLUMNS
                .split(", ")
                .map(|c| format!("t.{c}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_track_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn set_liked(&self, track_id: i64, liked: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tracks SET liked = ?1 WHERE id = ?2",
            rusqlite::params![liked as i64, track_id],
        )?;
        Ok(())
    }

    fn create_playlist(&self, name: &str) -> Result<i64> {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO playlists (name, created_at) VALUES (?1, ?2)",
            rusqlite::params![name, created_at],
        )?;
        Ok(conn.last_insert_rowid())
    }

    fn delete_playlist(&self, playlist_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM playlists WHERE id = ?1", [playlist_id])?;
        Ok(())
    }

    fn playlists(&self) -> Result<Vec<PlaylistSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT
                p.id,
                p.name,
                p.created_at,
                COUNT(pt.track_id) AS track_count
            FROM playlists p
            LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
            GROUP BY p.id
            ORDER BY p.created_at ASC, p.id ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PlaylistSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get(2)?,
                track_count: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn add_track_to_playlist(&self, playlist_id: i64, track_id: i64) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let next_position: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
                [playlist_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        // INSERT OR IGNORE: the (playlist_id, track_id) UNIQUE index silently
        // dedupes — double-clicks and stale "containing" UI checks become
        // harmless instead of erroring or producing duplicates.
        tx.execute(
            "INSERT OR IGNORE INTO playlist_tracks (playlist_id, position, track_id) VALUES (?1, ?2, ?3)",
            rusqlite::params![playlist_id, next_position, track_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn remove_track_from_playlist(&self, playlist_id: i64, track_id: i64) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        // Remove the lowest-position occurrence of the track (Spotify-ish: if
        // the same track is in the playlist multiple times, removes one copy).
        let position: Option<i64> = tx
            .query_row(
                "SELECT MIN(position) FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
                rusqlite::params![playlist_id, track_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let Some(position) = position else {
            tx.commit()?;
            return Ok(());
        };
        tx.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND position = ?2",
            rusqlite::params![playlist_id, position],
        )?;
        // Compact positions so they stay dense.
        tx.execute(
            "UPDATE playlist_tracks SET position = position - 1 \
             WHERE playlist_id = ?1 AND position > ?2",
            rusqlite::params![playlist_id, position],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn tracks_for_playlist(&self, playlist_id: i64) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {} FROM playlist_tracks pt \
             JOIN tracks t ON t.id = pt.track_id \
             WHERE pt.playlist_id = ?1 \
             ORDER BY pt.position",
            TRACK_COLUMNS
                .split(", ")
                .map(|c| format!("t.{c}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([playlist_id], map_track_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn playlists_containing_track(&self, track_id: i64) -> Result<Vec<i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT DISTINCT playlist_id FROM playlist_tracks WHERE track_id = ?1")?;
        let rows = stmt.query_map([track_id], |row| row.get::<_, i64>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn playlist_track_refs(&self) -> Result<Vec<PlaylistTrackRef>> {
        let conn = self.conn.lock().unwrap();
        // ORDER BY position is load-bearing: the restore step relies on Vec
        // order to assign new dense positions starting at 0.
        let mut stmt = conn.prepare(
            "SELECT pt.playlist_id, t.path, t.start_offset_ms \
             FROM playlist_tracks pt \
             JOIN tracks t ON t.id = pt.track_id \
             ORDER BY pt.playlist_id, pt.position",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PlaylistTrackRef {
                playlist_id: row.get(0)?,
                path: row.get(1)?,
                start_offset_ms: row.get(2)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn restore_playlist_track_refs(&self, refs: &[PlaylistTrackRef]) -> Result<()> {
        if refs.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut last_playlist_id: Option<i64> = None;
        let mut next_position: i64 = 0;
        for r in refs {
            if Some(r.playlist_id) != last_playlist_id {
                last_playlist_id = Some(r.playlist_id);
                next_position = 0;
            }
            let track_id: Option<i64> = tx
                .query_row(
                    "SELECT id FROM tracks WHERE path = ?1 AND start_offset_ms = ?2",
                    rusqlite::params![r.path, r.start_offset_ms],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(track_id) = track_id else { continue };
            // Skip rows that would violate the (playlist_id, track_id) UNIQUE
            // index — a track that appeared multiple times historically should
            // collapse to a single entry after restore.
            tx.execute(
                "INSERT OR IGNORE INTO playlist_tracks (playlist_id, position, track_id) VALUES (?1, ?2, ?3)",
                rusqlite::params![r.playlist_id, next_position, track_id],
            )?;
            // Only advance position when the insert actually happened. Without
            // checking `changes()` the positions would still be dense modulo
            // skipped duplicates, but the simpler invariant ("contiguous from
            // 0") matches what `add_track_to_playlist` produces.
            if tx.changes() > 0 {
                next_position += 1;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn track_artists_map(&self, track_ids: &[i64]) -> Result<HashMap<i64, Vec<String>>> {
        if track_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.conn.lock().unwrap();
        let placeholders = std::iter::repeat_n("?", track_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT ta.track_id, a.name FROM track_artists ta \
             JOIN artists a ON a.id = ta.artist_id \
             WHERE ta.track_id IN ({placeholders}) \
             ORDER BY ta.track_id, ta.position",
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = track_ids
            .iter()
            .map(|id| id as &dyn rusqlite::ToSql)
            .collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map: HashMap<i64, Vec<String>> = HashMap::new();
        for row in rows {
            let (id, name) = row.map_err(LibraryError::Database)?;
            map.entry(id).or_default().push(name);
        }
        Ok(map)
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
