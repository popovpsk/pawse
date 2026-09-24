use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use rusqlite::{Connection, OptionalExtension};

use crate::adoption::{Descriptor, Tier, WHOLE_FILE, match_tracks};
use crate::album_artists::{AlbumTrackArtists, derive_album_artists};
use crate::error::{LibraryError, Result};
use crate::migrations::MIGRATIONS;
use crate::models::{
    AlbumSearchEntry, AlbumSummary, ArtistGrouping, ArtistSummary, CoverArt, DeliveryOutcome,
    LocalFolder, NewLove, NewPlay, NewTrack, PendingLove, PendingPlay, PlaylistSummary,
    RemoteCover, RemoteSong, RemoteSource, RemoteSyncReport, ScanTrack, SourceSummary,
    StoredLyrics, Track,
};
use crate::repository::{LibraryRepository, ScanWrite};

/// Tracks committed per transaction during a batched scan. One `fsync` per
/// batch (with `synchronous = NORMAL`) instead of one per track.
const SCAN_BATCH_SIZE: usize = 256;
const SCAN_BATCH_TIME: std::time::Duration = std::time::Duration::from_millis(250);

const TRACK_COLUMNS: &str = "id, path, title, album_id, track_number, disc_number, \
    duration_ms, year, cover_art_id, start_offset_ms, \
    EXISTS(SELECT 1 FROM liked_track_ids lk WHERE lk.track_id = tracks.id), bitrate, is_cue, 1";

const TRACK_COLUMNS_T: &str = "t.id, t.path, t.title, t.album_id, t.track_number, \
    t.disc_number, t.duration_ms, t.year, t.cover_art_id, t.start_offset_ms, \
    EXISTS(SELECT 1 FROM liked_track_ids lk WHERE lk.track_id = t.id), t.bitrate, t.is_cue, 1";

const PLAYLIST_ENTRY_COLUMNS: &str = "m.id, COALESCE(t.path, b.source_key, ''), \
    COALESCE(t.title, m.title), t.album_id, t.track_number, COALESCE(t.disc_number, 1), \
    COALESCE(t.duration_ms, m.duration_ms), t.year, COALESCE(t.cover_art_id, m.cover_art_id), \
    COALESCE(t.start_offset_ms, b.start_offset_ms, 0), \
    EXISTS(SELECT 1 FROM liked_track_ids lk WHERE lk.track_id = m.id), t.bitrate, \
    COALESCE(t.is_cue, 0), t.id IS NOT NULL";

const PLACEHOLDER_SOURCE_ID: i64 = 1;

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
        bitrate: row.get(11)?,
        is_cue: row.get::<_, i64>(12)? != 0,
        available: row.get::<_, i64>(13)? != 0,
    })
}

fn display_ordered_tracks(conn: &Connection, where_clause: &str) -> Result<Vec<Track>> {
    let sql = format!(
        "SELECT {TRACK_COLUMNS_T} FROM tracks t \
         LEFT JOIN albums al ON al.id = t.album_id \
         LEFT JOIN album_artists aa ON aa.album_id = al.id AND aa.position = 0 \
         LEFT JOIN artists art ON art.id = aa.artist_id \
         {where_clause} \
         ORDER BY art.sort_name COLLATE NOCASE, COALESCE(al.year, 0), al.title COLLATE NOCASE, t.disc_number, t.track_number, t.title",
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], map_track_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(LibraryError::Database)
}

fn artist_membership_sql(grouping: ArtistGrouping) -> &'static str {
    match grouping {
        ArtistGrouping::TrackArtist => "SELECT artist_id, track_id, position FROM track_artists",
        ArtistGrouping::AlbumArtist => {
            "SELECT artist_id, track_id, position FROM track_album_artists \
             UNION ALL \
             SELECT aa.artist_id, t.id, aa.position FROM tracks t \
             JOIN albums al ON al.id = t.album_id AND al.artist_known = 1 \
             JOIN album_artists aa ON aa.album_id = al.id \
             WHERE NOT EXISTS (SELECT 1 FROM track_album_artists x WHERE x.track_id = t.id) \
             UNION ALL \
             SELECT ta.artist_id, ta.track_id, ta.position FROM track_artists ta \
             JOIN tracks t ON t.id = ta.track_id \
             LEFT JOIN albums al ON al.id = t.album_id \
             WHERE NOT EXISTS (SELECT 1 FROM track_album_artists x WHERE x.track_id = ta.track_id) \
             AND COALESCE(al.artist_known, 0) = 0"
        }
    }
}

fn membership_ctes(grouping: ArtistGrouping) -> String {
    let listed = artist_membership_sql(grouping);
    let credited = match grouping {
        ArtistGrouping::TrackArtist => "SELECT * FROM m".to_string(),
        ArtistGrouping::AlbumArtist => {
            "SELECT * FROM m UNION SELECT artist_id, track_id, position FROM track_artists"
                .to_string()
        }
    };
    format!("m AS ({listed}), u AS ({credited})")
}

fn map_artist_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArtistSummary> {
    Ok(ArtistSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        sort_name: row.get(2)?,
        track_count: row.get(3)?,
    })
}

fn no_metadata_artist(track_count: i64) -> ArtistSummary {
    ArtistSummary {
        id: crate::NO_METADATA_ARTIST_ID,
        name: String::new(),
        sort_name: String::new(),
        track_count,
    }
}

fn orphan_track_count(conn: &Connection, grouping: ArtistGrouping) -> Result<i64> {
    let membership = artist_membership_sql(grouping);
    let sql = format!(
        "WITH m AS ({membership}) SELECT COUNT(*) FROM tracks t \
         WHERE NOT EXISTS (SELECT 1 FROM m WHERE m.track_id = t.id)"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let count: i64 = stmt.query_row([], |row| row.get(0))?;
    Ok(count)
}

pub struct SqliteLibrary {
    conn: Mutex<Connection>,
    scrobble_conn: Mutex<Connection>,
    db_path: PathBuf,
    liked_playlist_id: i64,
}

/// Remove the SQLite database and its WAL sidecar files. Used when an
/// incompatible on-disk schema is detected (no users → no migrations).
fn remove_db_files(db_path: &Path) {
    let _ = std::fs::remove_file(db_path);
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = db_path.as_os_str().to_owned();
        sidecar.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(sidecar));
    }
}

fn premigration_schema(db_path: &Path) -> bool {
    let Ok(conn) = Connection::open(db_path) else {
        return false;
    };
    let Ok(version) = conn.query_row("SELECT user_version FROM pragma_user_version", [], |row| {
        row.get::<_, i64>(0)
    }) else {
        return false;
    };
    version == 0
}

fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    // WAL lets the scan-writer connection commit concurrently with UI reads on
    // the main connection; NORMAL drops the per-commit fsync to one per WAL
    // checkpoint — the single biggest reindex speedup, especially on Windows.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "cache_size", -16384)?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    // The scan writer holds a transaction open across each batch; without a
    // busy timeout a concurrent UI write (e.g. liking a track mid-scan) would
    // fail with SQLITE_BUSY instead of waiting for the batch to commit.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn compress_lyrics(text: &str) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    let _ = encoder.write_all(text.as_bytes());
    encoder.finish().unwrap_or_default()
}

fn decompress_lyrics(blob: &[u8]) -> Option<String> {
    let mut out = String::new();
    match ZlibDecoder::new(blob).read_to_string(&mut out) {
        Ok(_) => Some(out),
        Err(e) => {
            log::warn!(
                "Failed to decompress lyrics blob ({} bytes): {e}",
                blob.len()
            );
            None
        }
    }
}

const IDENTITY_MIGRATION: i32 = 9;

const CLEAR_CATALOG: &str = "DELETE FROM track_artists; \
    DELETE FROM track_album_artists; \
    DELETE FROM track_genres; \
    DELETE FROM album_artists; \
    DELETE FROM tracks; \
    DELETE FROM albums; \
    DELETE FROM artists; \
    DELETE FROM genres; \
    DELETE FROM lyrics WHERE source IN ('lrc', 'embedded');";

const RETIRE_UNSEEN_LOCAL_BINDINGS: &str = "UPDATE media_bindings SET present = 0 \
    WHERE present = 1 AND last_seen_scan <> ?1 \
    AND source_id IN (SELECT id FROM sources WHERE kind = 'local' AND enabled = 1 AND available = 1)";

const IDENTITY_ITEMS: &str =
    "SELECT id, title, artist, album, duration_ms FROM media_items ORDER BY id";

const IDENTITY_BINDINGS: &str = "SELECT b.id, b.item_id, b.source_id, s.kind = 'local', \
    s.enabled, s.available, b.present, b.last_seen_scan, b.file_size, b.start_offset_ms, \
    b.start_offset_ms > 0 OR EXISTS (SELECT 1 FROM media_bindings o \
        WHERE o.source_key = b.source_key AND o.start_offset_ms > 0 AND o.source_id = b.source_id) \
    FROM media_bindings b JOIN sources s ON s.id = b.source_id";

const UPSERT_REMOTE_TRACK: &str = "INSERT INTO remote_tracks \
    (binding_id, title, artist, album, album_artist, track_number, disc_number, year, genre, \
     duration_ms, size, suffix, content_type, bitrate, cover_key, cover_hash, updated_at) \
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17) \
    ON CONFLICT(binding_id) DO UPDATE SET title = excluded.title, artist = excluded.artist, \
    album = excluded.album, album_artist = excluded.album_artist, \
    track_number = excluded.track_number, disc_number = excluded.disc_number, \
    year = excluded.year, genre = excluded.genre, duration_ms = excluded.duration_ms, \
    size = excluded.size, suffix = excluded.suffix, content_type = excluded.content_type, \
    bitrate = excluded.bitrate, cover_key = excluded.cover_key, \
    cover_hash = excluded.cover_hash, updated_at = excluded.updated_at \
    WHERE remote_tracks.title IS NOT excluded.title OR remote_tracks.artist IS NOT excluded.artist \
    OR remote_tracks.album IS NOT excluded.album \
    OR remote_tracks.album_artist IS NOT excluded.album_artist \
    OR remote_tracks.track_number IS NOT excluded.track_number \
    OR remote_tracks.disc_number IS NOT excluded.disc_number \
    OR remote_tracks.year IS NOT excluded.year OR remote_tracks.genre IS NOT excluded.genre \
    OR remote_tracks.duration_ms IS NOT excluded.duration_ms \
    OR remote_tracks.suffix IS NOT excluded.suffix \
    OR remote_tracks.content_type IS NOT excluded.content_type \
    OR remote_tracks.bitrate IS NOT excluded.bitrate \
    OR remote_tracks.cover_hash IS NOT excluded.cover_hash";

const PROJECTABLE_REMOTE_TRACKS: &str = "SELECT b.item_id, b.source_id, b.source_key, \
    rt.title, rt.artist, rt.album, rt.album_artist, rt.track_number, rt.disc_number, rt.year, \
    rt.genre, rt.duration_ms, rt.suffix, rt.content_type, rt.bitrate, rt.cover_hash \
    FROM remote_tracks rt JOIN media_bindings b ON b.id = rt.binding_id \
    JOIN sources s ON s.id = b.source_id \
    WHERE b.present = 1 AND s.enabled = 1 AND s.available = 1 \
    AND (b.file_size IS NULL \
        OR EXISTS (SELECT 1 FROM media_bindings o WHERE o.item_id = b.item_id \
            AND o.source_id <> b.source_id) \
        OR NOT EXISTS (SELECT 1 FROM media_bindings c JOIN sources cs ON cs.id = c.source_id \
            WHERE c.file_size = b.file_size AND c.start_offset_ms > 0 AND c.present = 1 \
            AND c.source_id <> b.source_id AND cs.enabled = 1 AND cs.available = 1)) \
    ORDER BY b.item_id, CASE s.kind WHEN 'subsonic' THEN 1 ELSE 2 END, s.id, b.id";

const ITEMS_WITH_USER_DATA: &str = "SELECT track_id FROM playlist_tracks \
    UNION SELECT track_id FROM lyrics WHERE source NOT IN ('lrc', 'embedded') \
    UNION SELECT track_id FROM plays WHERE track_id IS NOT NULL \
    UNION SELECT track_id FROM loves WHERE track_id IS NOT NULL";

const UNWRITE_TRACK: [&str; 5] = [
    "DELETE FROM track_artists WHERE track_id = ?1",
    "DELETE FROM track_album_artists WHERE track_id = ?1",
    "DELETE FROM track_genres WHERE track_id = ?1",
    "DELETE FROM lyrics WHERE track_id = ?1 AND source IN ('lrc', 'embedded')",
    "DELETE FROM tracks WHERE id = ?1",
];

const ABSORB_ITEM: [&str; 9] = [
    "UPDATE tracks SET id = ?1 WHERE id = ?2",
    "UPDATE track_artists SET track_id = ?1 WHERE track_id = ?2",
    "UPDATE track_album_artists SET track_id = ?1 WHERE track_id = ?2",
    "UPDATE track_genres SET track_id = ?1 WHERE track_id = ?2",
    "INSERT INTO lyrics (track_id, source, text, not_found, updated_at) \
        SELECT ?1, source, text, not_found, updated_at FROM lyrics WHERE track_id = ?2 \
        ON CONFLICT(track_id) DO UPDATE SET source = excluded.source, text = excluded.text, \
        not_found = excluded.not_found, updated_at = excluded.updated_at",
    "DELETE FROM lyrics WHERE track_id = ?2",
    "UPDATE media_bindings SET item_id = ?1 WHERE item_id = ?2",
    "DELETE FROM adoptions WHERE item_id = ?2",
    "DELETE FROM media_items WHERE id = ?2",
];

const SWEEP_UNREFERENCED_ITEMS: &str = "DELETE FROM media_items WHERE \
    NOT EXISTS (SELECT 1 FROM tracks WHERE tracks.id = media_items.id) \
    AND NOT EXISTS (SELECT 1 FROM media_bindings b JOIN sources s ON s.id = b.source_id \
        WHERE b.item_id = media_items.id AND b.present = 1 AND s.enabled = 1) \
    AND NOT EXISTS (SELECT 1 FROM playlist_tracks WHERE track_id = media_items.id) \
    AND NOT EXISTS (SELECT 1 FROM lyrics WHERE track_id = media_items.id \
        AND source NOT IN ('lrc', 'embedded')) \
    AND NOT EXISTS (SELECT 1 FROM plays WHERE track_id = media_items.id) \
    AND NOT EXISTS (SELECT 1 FROM loves WHERE track_id = media_items.id)";

const REFRESH_ITEM_SNAPSHOTS: &str = "UPDATE media_items SET \
    title = c.title, artist = c.artist, album = c.album, duration_ms = c.duration_ms, \
    cover_art_id = c.cover_art_id, updated_at = ?1 \
    FROM ( \
        SELECT t.id AS id, t.title AS title, \
            COALESCE((SELECT a.name FROM track_artists ta JOIN artists a ON a.id = ta.artist_id \
                WHERE ta.track_id = t.id ORDER BY ta.position LIMIT 1), '') AS artist, \
            (SELECT al.title FROM albums al WHERE al.id = t.album_id) AS album, \
            t.duration_ms AS duration_ms, t.cover_art_id AS cover_art_id \
        FROM tracks t \
    ) c \
    WHERE c.id = media_items.id \
    AND (media_items.title IS NOT c.title OR media_items.artist IS NOT c.artist \
        OR media_items.album IS NOT c.album OR media_items.duration_ms IS NOT c.duration_ms \
        OR media_items.cover_art_id IS NOT c.cover_art_id)";

fn sidecar(db_path: &Path, suffix: &str) -> PathBuf {
    let mut path = db_path.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}

fn backup_before_migration(conn: &Connection, db_path: &Path, version: i32) -> Result<()> {
    let target = sidecar(db_path, &format!(".bak-v{version}"));
    if target.exists() {
        return Ok(());
    }
    let partial = sidecar(db_path, &format!(".bak-v{version}.partial"));
    let _ = std::fs::remove_file(&partial);
    conn.execute("VACUUM INTO ?1", [partial.to_string_lossy().into_owned()])?;
    std::fs::rename(&partial, &target)?;
    Ok(())
}

fn foreign_key_violations(conn: &Connection) -> Result<i64> {
    Ok(
        conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?,
    )
}

fn apply_migrations(conn: &mut Connection, from: i32) -> Result<()> {
    let tx = conn.transaction()?;
    let violations_before = foreign_key_violations(&tx)?;
    for (version, sql) in MIGRATIONS.iter() {
        if from < *version {
            if *version == IDENTITY_MIGRATION {
                seed_liked_playlist_from_column(&tx)?;
            }
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", *version)?;
        }
    }
    let violations_after = foreign_key_violations(&tx)?;
    if violations_after > violations_before {
        return Err(LibraryError::InvalidData(format!(
            "migration raised foreign key violations from {violations_before} to {violations_after}"
        )));
    }
    tx.commit()?;
    Ok(())
}

fn seed_liked_playlist_from_column(conn: &Connection) -> Result<()> {
    if stored_liked_playlist(conn)?.is_some() {
        return Ok(());
    }
    let playlist_id = create_liked_playlist(conn)?;
    conn.execute(
        "INSERT INTO playlist_tracks (playlist_id, position, track_id) \
         SELECT ?1, ROW_NUMBER() OVER (ORDER BY art.sort_name COLLATE NOCASE, \
             COALESCE(al.year, 0), al.title COLLATE NOCASE, t.disc_number, \
             t.track_number, t.title) - 1, t.id \
         FROM tracks t \
         LEFT JOIN albums al ON al.id = t.album_id \
         LEFT JOIN album_artists aa ON aa.album_id = al.id AND aa.position = 0 \
         LEFT JOIN artists art ON art.id = aa.artist_id \
         WHERE t.liked = 1",
        [playlist_id],
    )?;
    Ok(())
}

fn stored_liked_playlist(conn: &Connection) -> Result<Option<i64>> {
    let stored = conn
        .query_row(
            "SELECT value FROM scan_meta WHERE key = 'liked_playlist_id'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .and_then(|value| value.parse::<i64>().ok());
    let Some(id) = stored else {
        return Ok(None);
    };
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM playlists WHERE id = ?1)",
        [id],
        |row| row.get(0),
    )?;
    Ok(exists.then_some(id))
}

fn create_liked_playlist(conn: &Connection) -> Result<i64> {
    conn.execute(
        "INSERT INTO playlists (name, created_at) VALUES ('Liked', ?1)",
        [unix_now()],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO scan_meta (key, value) VALUES ('liked_playlist_id', ?1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [id.to_string()],
    )?;
    Ok(id)
}

struct LocalRoot {
    path: PathBuf,
    source_id: i64,
    available: bool,
}

fn load_local_roots(conn: &Connection) -> Result<Vec<LocalRoot>> {
    let mut stmt = conn
        .prepare("SELECT id, uri, available FROM sources WHERE kind = 'local' AND enabled = 1")?;
    let mut roots = stmt
        .query_map([], |row| {
            Ok(LocalRoot {
                source_id: row.get(0)?,
                path: PathBuf::from(row.get::<_, String>(1)?),
                available: row.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    roots.sort_by_key(|root| std::cmp::Reverse(root.path.components().count()));
    Ok(roots)
}

fn root_for_path<'a>(roots: &'a [LocalRoot], path: &str) -> Option<&'a LocalRoot> {
    let path = Path::new(path);
    roots.iter().find(|root| path.starts_with(&root.path))
}

fn source_for_path(roots: &[LocalRoot], path: &str) -> i64 {
    root_for_path(roots, path).map_or(PLACEHOLDER_SOURCE_ID, |root| root.source_id)
}

struct ItemSnapshot<'a> {
    title: &'a str,
    artist: &'a str,
    album: Option<&'a str>,
    duration_ms: Option<i64>,
    cover_art_id: Option<i64>,
}

struct BindingFacts {
    binding_id: i64,
    source_id: i64,
    local: bool,
    enabled: bool,
    available: bool,
    present: bool,
    last_seen_scan: i64,
    file_size: Option<i64>,
    start_offset_ms: i64,
    cue: bool,
}

struct Identity {
    item_id: i64,
    title: String,
    artist: String,
    album: Option<String>,
    duration_ms: Option<i64>,
    bindings: Vec<BindingFacts>,
    cherished: bool,
}

impl Identity {
    fn descriptor(&self, occupies: &dyn Fn(&BindingFacts) -> bool) -> Descriptor {
        let mut sources: Vec<i64> = self
            .bindings
            .iter()
            .filter(|b| occupies(b))
            .map(|b| b.source_id)
            .collect();
        sources.sort_unstable();
        sources.dedup();
        Descriptor {
            item_id: self.item_id,
            title: self.title.clone(),
            artist: self.artist.clone(),
            artist_aliases: Vec::new(),
            album: self.album.clone(),
            duration_ms: self.duration_ms,
            files: self
                .bindings
                .iter()
                .filter_map(|b| {
                    b.file_size
                        .map(|size| (size, file_piece(b.cue, b.start_offset_ms)))
                })
                .collect(),
            live: !sources.is_empty(),
            sources,
        }
    }

    fn playable(&self) -> bool {
        self.bindings
            .iter()
            .any(|b| b.enabled && b.available && b.present)
    }
}

fn file_piece(cue: bool, start_offset_ms: i64) -> i64 {
    if cue { start_offset_ms } else { WHOLE_FILE }
}

fn load_identities(conn: &Connection) -> Result<Vec<Identity>> {
    let mut identities: Vec<Identity> = {
        let mut stmt = conn.prepare(IDENTITY_ITEMS)?;
        stmt.query_map([], |row| {
            Ok(Identity {
                item_id: row.get(0)?,
                title: row.get(1)?,
                artist: row.get(2)?,
                album: row.get(3)?,
                duration_ms: row.get(4)?,
                bindings: Vec::new(),
                cherished: false,
            })
        })?
        .collect::<std::result::Result<_, _>>()?
    };
    let by_item: HashMap<i64, usize> = identities
        .iter()
        .enumerate()
        .map(|(ix, identity)| (identity.item_id, ix))
        .collect();
    {
        let mut stmt = conn.prepare(IDENTITY_BINDINGS)?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(1)?,
                BindingFacts {
                    binding_id: row.get(0)?,
                    source_id: row.get(2)?,
                    local: row.get(3)?,
                    enabled: row.get(4)?,
                    available: row.get(5)?,
                    present: row.get(6)?,
                    last_seen_scan: row.get(7)?,
                    file_size: row.get(8)?,
                    start_offset_ms: row.get(9)?,
                    cue: row.get(10)?,
                },
            ))
        })?;
        for row in rows {
            let (item_id, facts) = row?;
            if let Some(&ix) = by_item.get(&item_id) {
                identities[ix].bindings.push(facts);
            }
        }
    }
    let mut stmt = conn.prepare(ITEMS_WITH_USER_DATA)?;
    let cherished = stmt.query_map([], |row| row.get::<_, i64>(0))?;
    for item_id in cherished {
        if let Some(&ix) = by_item.get(&item_id?) {
            identities[ix].cherished = true;
        }
    }
    Ok(identities)
}

fn upsert_remote_track(conn: &Connection, binding_id: i64, song: &RemoteSong) -> Result<usize> {
    Ok(conn.execute(
        UPSERT_REMOTE_TRACK,
        rusqlite::params![
            binding_id,
            song.title,
            song.artist,
            song.album,
            song.album_artist,
            song.track_number,
            song.disc_number,
            song.year,
            song.genre,
            song.duration_ms,
            song.size,
            song.suffix,
            song.content_type,
            song.bitrate,
            song.cover_key,
            song.cover_hash,
            unix_now(),
        ],
    )?)
}

fn apply_remote_listing(
    conn: &mut Connection,
    source_id: i64,
    songs: &[RemoteSong],
    covers: &[RemoteCover],
) -> Result<RemoteSyncReport> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mut known: HashMap<String, (i64, i64, bool)> = HashMap::new();
    {
        let mut stmt = tx.prepare(
            "SELECT source_key, id, item_id, present FROM media_bindings WHERE source_id = ?1",
        )?;
        let rows = stmt.query_map([source_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                (row.get(1)?, row.get(2)?, row.get(3)?),
            ))
        })?;
        for row in rows {
            let (key, value) = row?;
            known.insert(key, value);
        }
    }
    if songs.is_empty() && known.values().any(|(_, _, present)| *present) {
        return Err(LibraryError::InvalidData(
            "the server listed no songs; keeping what it had".into(),
        ));
    }
    let was_available: bool = tx.query_row(
        "SELECT available FROM sources WHERE id = ?1",
        [source_id],
        |row| row.get(0),
    )?;
    for cover in covers {
        tx.execute(
            "INSERT OR IGNORE INTO cover_art (hash, small, large, source_path, embedded) \
             VALUES (?1, ?2, ?3, ?4, 0)",
            rusqlite::params![cover.hash, cover.small, cover.large, cover.source_path],
        )?;
    }
    let now = unix_now();
    let mut report = RemoteSyncReport {
        total: songs.len(),
        became_available: !was_available,
        ..Default::default()
    };
    let mut seen: Vec<i64> = Vec::with_capacity(songs.len());
    let mut arrivals: Vec<&RemoteSong> = Vec::new();
    for song in songs {
        match known.get(&song.key) {
            Some(&(binding_id, _, present)) => {
                if !present {
                    report.revived += 1;
                }
                tx.execute(
                    "UPDATE media_bindings SET present = 1, last_seen_at = ?1, file_size = ?2 \
                     WHERE id = ?3",
                    rusqlite::params![now, song.size, binding_id],
                )?;
                report.updated += upsert_remote_track(&tx, binding_id, song)?;
                seen.push(binding_id);
            }
            None => arrivals.push(song),
        }
    }
    arrivals.sort_by(|a, b| a.key.cmp(&b.key));
    arrivals.dedup_by(|a, b| a.key == b.key);
    if !arrivals.is_empty() {
        let seen_bindings: std::collections::HashSet<i64> = seen.iter().copied().collect();
        let occupies = |b: &BindingFacts| {
            b.enabled
                && if b.source_id == source_id {
                    seen_bindings.contains(&b.binding_id)
                } else {
                    b.present
                }
        };
        let candidates: Vec<Descriptor> = load_identities(&tx)?
            .iter()
            .map(|identity| identity.descriptor(&occupies))
            .collect();
        let descriptors: Vec<Descriptor> = arrivals
            .iter()
            .map(|song| Descriptor {
                title: song.title.clone(),
                artist: song.artist.clone().unwrap_or_default(),
                artist_aliases: song.artist_aliases.clone(),
                album: song.album.clone(),
                duration_ms: song.duration_ms,
                files: song
                    .size
                    .map(|size| (size, WHOLE_FILE))
                    .into_iter()
                    .collect(),
                sources: vec![source_id],
                ..Default::default()
            })
            .collect();
        let assignments = match_tracks(&descriptors, &candidates);
        for (song, assignment) in arrivals.into_iter().zip(assignments) {
            let binding_id = match assignment {
                Some((item_id, tier)) => {
                    let binding_id = bind_item(&tx, item_id, &remote_spec(source_id, song))?;
                    tx.execute(
                        "INSERT INTO adoptions (item_id, binding_id, tier, at) \
                         VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![item_id, binding_id, tier.as_str(), now],
                    )?;
                    report.adopted += 1;
                    binding_id
                }
                None => {
                    let (binding_id, _) = create_item(
                        &tx,
                        &remote_spec(source_id, song),
                        &ItemSnapshot {
                            title: &song.title,
                            artist: song.artist.as_deref().unwrap_or(""),
                            album: song.album.as_deref(),
                            duration_ms: song.duration_ms,
                            cover_art_id: None,
                        },
                    )?;
                    report.added += 1;
                    binding_id
                }
            };
            upsert_remote_track(&tx, binding_id, song)?;
            seen.push(binding_id);
        }
    }
    let seen_json = serde_json::to_string(&seen).unwrap_or_else(|_| "[]".into());
    report.retired = tx.execute(
        "UPDATE media_bindings SET present = 0 WHERE source_id = ?1 AND present = 1 \
         AND id NOT IN (SELECT value FROM json_each(?2))",
        rusqlite::params![source_id, seen_json],
    )?;
    tx.execute(
        "UPDATE sources SET available = 1, last_sync_at = ?1, last_sync_ok = 1, last_error = NULL \
         WHERE id = ?2",
        rusqlite::params![now, source_id],
    )?;
    tx.commit()?;
    Ok(report)
}

fn remote_spec(source_id: i64, song: &RemoteSong) -> BindingSpec<'_> {
    BindingSpec {
        source_id,
        key: &song.key,
        start_offset_ms: 0,
        file_size: song.size,
        scan_id: 0,
    }
}

struct BindingSpec<'a> {
    source_id: i64,
    key: &'a str,
    start_offset_ms: i64,
    file_size: Option<i64>,
    scan_id: i64,
}

fn bind_item(conn: &Connection, item_id: i64, spec: &BindingSpec<'_>) -> Result<i64> {
    conn.execute(
        "INSERT INTO media_bindings \
         (item_id, source_id, source_key, start_offset_ms, present, last_seen_scan, last_seen_at, \
          file_size) \
         VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7)",
        rusqlite::params![
            item_id,
            spec.source_id,
            spec.key,
            spec.start_offset_ms,
            spec.scan_id,
            unix_now(),
            spec.file_size
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn create_item(
    conn: &Connection,
    spec: &BindingSpec<'_>,
    snapshot: &ItemSnapshot<'_>,
) -> Result<(i64, i64)> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO media_items \
         (kind, title, artist, album, duration_ms, cover_art_id, created_at, updated_at) \
         VALUES ('track', ?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        rusqlite::params![
            snapshot.title,
            snapshot.artist,
            snapshot.album,
            snapshot.duration_ms,
            snapshot.cover_art_id,
            now
        ],
    )?;
    let item_id = conn.last_insert_rowid();
    let binding_id = bind_item(conn, item_id, spec)?;
    Ok((binding_id, item_id))
}

fn resolve_local_item(
    conn: &Connection,
    path: &str,
    start_offset_ms: i64,
    snapshot: &ItemSnapshot<'_>,
) -> Result<i64> {
    let existing: Option<(i64, i64)> = conn
        .query_row(
            "SELECT b.id, b.item_id FROM media_bindings b \
             JOIN sources s ON s.id = b.source_id \
             WHERE s.kind = 'local' AND b.source_key = ?1 AND b.start_offset_ms = ?2 \
             LIMIT 1",
            rusqlite::params![path, start_offset_ms],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((binding_id, item_id)) = existing {
        conn.execute(
            "UPDATE media_bindings SET present = 1, last_seen_at = ?1 WHERE id = ?2",
            rusqlite::params![unix_now(), binding_id],
        )?;
        return Ok(item_id);
    }
    let roots = load_local_roots(conn)?;
    let source_id = source_for_path(&roots, path);
    let (_, item_id) = create_item(
        conn,
        &BindingSpec {
            source_id,
            key: path,
            start_offset_ms,
            file_size: None,
            scan_id: 0,
        },
        snapshot,
    )?;
    Ok(item_id)
}

impl SqliteLibrary {
    pub fn open() -> Result<Self> {
        let db_dir = dirs::data_dir()
            .ok_or_else(|| LibraryError::InvalidData("no data dir".into()))?
            .join("pawse");
        std::fs::create_dir_all(&db_dir)?;

        let db_path = db_dir.join("library.db");
        if db_path.exists() && premigration_schema(&db_path) {
            let check = Connection::open(&db_path);
            if let Ok(check_conn) = check {
                let has_cover_art: bool = check_conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cover_art')",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|c| c > 0)
                    .unwrap_or(false);
                let has_liked: bool = check_conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('tracks') WHERE name='liked')",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|c| c > 0)
                    .unwrap_or(false);
                let has_playlists: bool = check_conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='playlists')",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|c| c > 0)
                    .unwrap_or(false);
                let has_playlist_unique: bool = check_conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_playlist_tracks_pair')",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|c| c > 0)
                    .unwrap_or(false);
                let has_scan_meta: bool = check_conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='scan_meta')",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|c| c > 0)
                    .unwrap_or(false);
                let has_bitrate: bool = check_conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('tracks') WHERE name='bitrate')",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|c| c > 0)
                    .unwrap_or(false);
                drop(check_conn);
                if !has_cover_art
                    || !has_liked
                    || !has_playlists
                    || !has_playlist_unique
                    || !has_scan_meta
                    || !has_bitrate
                {
                    remove_db_files(&db_path);
                }
            }
        }

        let conn = Connection::open(&db_path)?;
        apply_pragmas(&conn)?;
        let scrobble_conn = Connection::open(&db_path)?;
        apply_pragmas(&scrobble_conn)?;
        let mut lib = Self {
            conn: Mutex::new(conn),
            scrobble_conn: Mutex::new(scrobble_conn),
            db_path,
            liked_playlist_id: 0,
        };
        lib.run_migrations()?;
        lib.liked_playlist_id = lib.ensure_liked_playlist()?;
        Ok(lib)
    }

    pub fn open_at(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let db_dir = path.parent().unwrap_or(path);
        std::fs::create_dir_all(db_dir)?;
        let conn = Connection::open(path)?;
        apply_pragmas(&conn)?;
        let scrobble_conn = Connection::open(path)?;
        apply_pragmas(&scrobble_conn)?;
        let mut lib = Self {
            conn: Mutex::new(conn),
            scrobble_conn: Mutex::new(scrobble_conn),
            db_path: path.to_path_buf(),
            liked_playlist_id: 0,
        };
        lib.run_migrations()?;
        lib.liked_playlist_id = lib.ensure_liked_playlist()?;
        Ok(lib)
    }

    fn run_migrations(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let user_version: i32 =
            conn.query_row("SELECT user_version FROM pragma_user_version", [], |row| {
                row.get(0)
            })?;
        let latest = MIGRATIONS.last().map_or(0, |(version, _)| *version);
        if user_version >= latest {
            return Ok(());
        }
        if user_version > 0 && user_version < IDENTITY_MIGRATION {
            backup_before_migration(&conn, &self.db_path, user_version)?;
        }
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        let applied = apply_migrations(&mut conn, user_version);
        conn.pragma_update(None, "foreign_keys", "ON")?;
        applied
    }

    fn ensure_liked_playlist(&self) -> Result<i64> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let id = match stored_liked_playlist(&tx)? {
            Some(id) => id,
            None => create_liked_playlist(&tx)?,
        };
        tx.commit()?;
        Ok(id)
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
        tx.execute(
            "UPDATE albums SET artist_known = ?2 WHERE id = ?1",
            [album_id, !artist_ids.is_empty() as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn set_track_album_artists(&self, track_id: i64, artist_ids: &[(i64, i32)]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM track_album_artists WHERE track_id = ?1",
            [track_id],
        )?;
        for (artist_id, position) in artist_ids {
            tx.execute(
                "INSERT OR IGNORE INTO track_album_artists (track_id, artist_id, position) VALUES (?1, ?2, ?3)",
                [track_id, *artist_id, *position as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn track_album_artists(&self, track_id: i64) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            r#"
            SELECT a.name
            FROM artists a
            JOIN track_album_artists x ON x.artist_id = a.id
            WHERE x.track_id = ?1
            ORDER BY x.position
            "#,
        )?;
        let rows = stmt.query_map([track_id], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
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
                    start_offset_ms = ?8,
                    bitrate = ?10
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
                    track.bitrate,
                ],
            )?;
            id
        } else {
            let album_title = match album_id {
                Some(id) => tx
                    .query_row("SELECT title FROM albums WHERE id = ?1", [id], |row| {
                        row.get::<_, String>(0)
                    })
                    .optional()?,
                None => None,
            };
            let artist = match artist_ids.first() {
                Some((id, _)) => tx
                    .query_row("SELECT name FROM artists WHERE id = ?1", [id], |row| {
                        row.get::<_, String>(0)
                    })
                    .optional()?
                    .unwrap_or_default(),
                None => String::new(),
            };
            let item_id = resolve_local_item(
                &tx,
                &track.path,
                start_offset_ms as i64,
                &ItemSnapshot {
                    title: &title,
                    artist: &artist,
                    album: album_title.as_deref(),
                    duration_ms,
                    cover_art_id: track.cover_art_id,
                },
            )?;
            tx.execute(
                r#"INSERT INTO tracks
                    (id, path, title, album_id, track_number, disc_number, duration_ms, year, cover_art_id, start_offset_ms, bitrate)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"#,
                rusqlite::params![
                    item_id,
                    track.path,
                    title,
                    album_id,
                    track_number,
                    disc_number,
                    duration_ms,
                    track.year,
                    track.cover_art_id,
                    start_offset_ms,
                    track.bitrate,
                ],
            )?;
            item_id
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
        let mut stmt = conn.prepare_cached(
            r#"
            SELECT
                a.id,
                a.title,
                a.year,
                a.cover_art_id,
                art.name,
                art.id
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
                artist_id: row.get::<_, Option<i64>>(5)?,
            })
        })?;
        let mut albums = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)?;
        let has_orphans: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tracks WHERE album_id IS NULL)",
            [],
            |row| row.get(0),
        )?;
        if has_orphans {
            albums.push(AlbumSummary {
                id: crate::NO_METADATA_ALBUM_ID,
                title: String::new(),
                year: None,
                cover_art_id: None,
                artist_name: String::new(),
                artist_id: None,
            });
        }
        Ok(albums)
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
        let mut entries = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)?;
        let orphan_titles: Option<String> = conn.query_row(
            "SELECT GROUP_CONCAT(title, ' ') FROM tracks WHERE album_id IS NULL",
            [],
            |row| row.get(0),
        )?;
        if let Some(haystack) = orphan_titles {
            entries.push(AlbumSearchEntry {
                album_id: crate::NO_METADATA_ALBUM_ID,
                haystack,
            });
        }
        Ok(entries)
    }

    fn tracks_for_album(&self, album_id: i64) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        if album_id == crate::NO_METADATA_ALBUM_ID {
            let sql = format!(
                "SELECT {TRACK_COLUMNS} FROM tracks WHERE album_id IS NULL \
                 ORDER BY disc_number, track_number, title",
            );
            let mut stmt = conn.prepare_cached(&sql)?;
            let rows = stmt.query_map([], map_track_row)?;
            return rows
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(LibraryError::Database);
        }
        let sql = format!(
            "SELECT {TRACK_COLUMNS} FROM tracks WHERE album_id = ?1 \
             ORDER BY disc_number, track_number, title",
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map([album_id], map_track_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn album_track_counts(&self) -> Result<HashMap<i64, i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT album_id, COUNT(*) FROM tracks WHERE album_id IS NOT NULL GROUP BY album_id",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
        rows.collect::<std::result::Result<HashMap<_, _>, _>>()
            .map_err(LibraryError::Database)
    }

    fn track_artists(&self, track_id: i64) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
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
        let mut stmt = conn.prepare_cached(
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
        let mut stmt = conn.prepare_cached("SELECT title FROM albums WHERE id = ?1")?;
        let title = stmt
            .query_row([album_id], |row| row.get::<_, Option<String>>(0))
            .optional()?
            .flatten();
        Ok(title)
    }

    fn album_genres(&self, album_id: i64) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            r#"
            SELECT g.name
            FROM genres g
            JOIN track_genres tg ON tg.genre_id = g.id
            JOIN tracks t ON t.id = tg.track_id
            WHERE t.album_id = ?1
            GROUP BY g.id
            ORDER BY COUNT(*) DESC, g.name
            "#,
        )?;
        let rows = stmt.query_map([album_id], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn track_genres(&self, track_id: i64) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            r#"
            SELECT g.name
            FROM genres g
            JOIN track_genres tg ON tg.genre_id = g.id
            WHERE tg.track_id = ?1
            ORDER BY tg.position
            "#,
        )?;
        let rows = stmt.query_map([track_id], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn album_artists(&self, album_id: i64) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            r#"
            SELECT ar.name
            FROM album_artists aa
            JOIN artists ar ON ar.id = aa.artist_id
            WHERE aa.album_id = ?1
            ORDER BY aa.position
            "#,
        )?;
        let rows = stmt.query_map([album_id], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn album_genres_map(&self) -> Result<HashMap<i64, Vec<String>>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            r#"
            SELECT t.album_id, g.name
            FROM genres g
            JOIN track_genres tg ON tg.genre_id = g.id
            JOIN tracks t ON t.id = tg.track_id
            GROUP BY t.album_id, g.id
            ORDER BY t.album_id, COUNT(*) DESC, g.name
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map: HashMap<i64, Vec<String>> = HashMap::new();
        for row in rows {
            let (album_id, name) = row.map_err(LibraryError::Database)?;
            map.entry(album_id).or_default().push(name);
        }
        Ok(map)
    }

    fn clear(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute_batch(CLEAR_CATALOG)?;
        tx.commit()?;
        Ok(())
    }

    fn has_tracks(&self) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let exists: bool =
            conn.query_row("SELECT EXISTS(SELECT 1 FROM tracks)", [], |row| row.get(0))?;
        Ok(exists)
    }

    fn delete_orphaned_albums_and_artists(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM albums WHERE NOT EXISTS (SELECT 1 FROM tracks WHERE tracks.album_id = albums.id)",
            [],
        )?;
        tx.execute(
            "DELETE FROM artists WHERE NOT EXISTS (SELECT 1 FROM album_artists WHERE album_artists.artist_id = artists.id)
             AND NOT EXISTS (SELECT 1 FROM track_artists WHERE track_artists.artist_id = artists.id)
             AND NOT EXISTS (SELECT 1 FROM track_album_artists WHERE track_album_artists.artist_id = artists.id)",
            [],
        )?;
        tx.execute(
            "DELETE FROM cover_art WHERE NOT EXISTS (SELECT 1 FROM albums WHERE albums.cover_art_id = cover_art.id)
             AND NOT EXISTS (SELECT 1 FROM tracks WHERE tracks.cover_art_id = cover_art.id)
             AND NOT EXISTS (SELECT 1 FROM media_items WHERE media_items.cover_art_id = cover_art.id)
             AND NOT EXISTS (SELECT 1 FROM remote_tracks WHERE remote_tracks.cover_hash = cover_art.hash)",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn save_cover_art(&self, data: &[u8], source_path: &str, embedded: bool) -> Result<i64> {
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
            "INSERT INTO cover_art (hash, small, large, source_path, embedded) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                hash,
                thumbnails.small,
                thumbnails.large,
                source_path,
                embedded
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    fn set_track_cover(&self, track_id: i64, cover_art_id: Option<i64>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tracks SET cover_art_id = ?1 WHERE id = ?2",
            rusqlite::params![cover_art_id, track_id],
        )?;
        Ok(())
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

    fn get_cover_art_source(&self, id: i64) -> Result<Option<(String, bool)>> {
        let conn = self.conn.lock().unwrap();
        let result: Option<(Option<String>, bool)> = conn
            .query_row(
                "SELECT source_path, embedded FROM cover_art WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(result.and_then(|(path, embedded)| path.map(|p| (p, embedded))))
    }

    fn get_track_path_for_cover(&self, id: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let result = conn
            .query_row(
                "SELECT path FROM tracks WHERE cover_art_id = ?1 LIMIT 1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result)
    }

    fn resolve_album_covers(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"
            UPDATE albums SET cover_art_id = (
                SELECT t.cover_art_id FROM tracks t
                WHERE t.album_id = albums.id AND t.cover_art_id IS NOT NULL
                ORDER BY t.disc_number, COALESCE(t.track_number, 999999), t.path
                LIMIT 1
            )
            "#,
            [],
        )?;
        Ok(())
    }

    fn resolve_album_artists(&self) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        let mut explicit: HashMap<i64, Vec<i64>> = HashMap::new();
        {
            let mut stmt = tx.prepare(
                "SELECT track_id, artist_id FROM track_album_artists ORDER BY track_id, position",
            )?;
            let rows =
                stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
            for row in rows {
                let (track_id, artist_id) = row?;
                explicit.entry(track_id).or_default().push(artist_id);
            }
        }
        let mut credited: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
        {
            let mut stmt = tx.prepare(
                "SELECT ta.track_id, a.id, a.name FROM track_artists ta \
                 JOIN artists a ON a.id = ta.artist_id ORDER BY ta.track_id, ta.position",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (track_id, artist_id, name) = row?;
                credited
                    .entry(track_id)
                    .or_default()
                    .push((artist_id, name));
            }
        }
        let mut albums: Vec<(i64, Vec<AlbumTrackArtists>)> = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT t.id, t.album_id FROM tracks t WHERE t.album_id IS NOT NULL \
                 ORDER BY t.album_id, t.disc_number, COALESCE(t.track_number, 999999), t.path",
            )?;
            let rows =
                stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
            for row in rows {
                let (track_id, album_id) = row?;
                let entry = AlbumTrackArtists {
                    explicit: explicit.remove(&track_id).unwrap_or_default(),
                    artists: credited.remove(&track_id).unwrap_or_default(),
                };
                match albums.last_mut() {
                    Some((id, tracks)) if *id == album_id => tracks.push(entry),
                    _ => albums.push((album_id, vec![entry])),
                }
            }
        }

        tx.execute("DELETE FROM album_artists", [])?;
        tx.execute("UPDATE albums SET artist_known = 0", [])?;
        {
            let mut insert = tx.prepare(
                "INSERT OR IGNORE INTO album_artists (album_id, artist_id, position) VALUES (?1, ?2, ?3)",
            )?;
            let mut mark_known = tx.prepare("UPDATE albums SET artist_known = 1 WHERE id = ?1")?;
            for (album_id, tracks) in &albums {
                let derived = derive_album_artists(tracks);
                for (position, artist_id) in derived.artist_ids.iter().enumerate() {
                    insert.execute([*album_id, *artist_id, position as i64])?;
                }
                if derived.known {
                    mark_known.execute([*album_id])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn album_artist_known(&self, album_id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT artist_known FROM albums WHERE id = ?1",
            [album_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map(|known| known.unwrap_or(0) != 0)
        .map_err(LibraryError::Database)
    }

    fn artists(&self, grouping: ArtistGrouping) -> Result<Vec<ArtistSummary>> {
        let conn = self.conn.lock().unwrap();
        let ctes = membership_ctes(grouping);
        let sql = format!(
            "WITH {ctes} \
             SELECT a.id, a.name, a.sort_name, COUNT(DISTINCT u.track_id) AS track_count \
             FROM artists a \
             JOIN u ON u.artist_id = a.id \
             WHERE EXISTS (SELECT 1 FROM m WHERE m.artist_id = a.id) \
             GROUP BY a.id \
             HAVING track_count > 0 \
             ORDER BY a.sort_name COLLATE NOCASE, a.name COLLATE NOCASE"
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map([], map_artist_summary)?;
        let mut artists = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)?;
        let orphan_count = orphan_track_count(&conn, grouping)?;
        if orphan_count > 0 {
            artists.push(no_metadata_artist(orphan_count));
        }
        Ok(artists)
    }

    fn artist_summary(&self, id: i64, grouping: ArtistGrouping) -> Result<Option<ArtistSummary>> {
        let conn = self.conn.lock().unwrap();
        if id == crate::NO_METADATA_ARTIST_ID {
            let orphan_count = orphan_track_count(&conn, grouping)?;
            return Ok((orphan_count > 0).then(|| no_metadata_artist(orphan_count)));
        }
        let ctes = membership_ctes(grouping);
        let sql = format!(
            "WITH {ctes} \
             SELECT a.id, a.name, a.sort_name, COUNT(DISTINCT u.track_id) AS track_count \
             FROM artists a \
             JOIN u ON u.artist_id = a.id \
             WHERE a.id = ?1 AND EXISTS (SELECT 1 FROM m WHERE m.artist_id = a.id) \
             GROUP BY a.id \
             HAVING track_count > 0"
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        stmt.query_row([id], map_artist_summary)
            .optional()
            .map_err(LibraryError::Database)
    }

    fn artist_search_haystacks(&self, grouping: ArtistGrouping) -> Result<HashMap<i64, String>> {
        let conn = self.conn.lock().unwrap();
        let ctes = membership_ctes(grouping);
        let sql = match grouping {
            ArtistGrouping::TrackArtist => format!(
                "WITH {ctes} \
                 SELECT a.id, a.name FROM artists a \
                 WHERE EXISTS (SELECT 1 FROM m WHERE m.artist_id = a.id)"
            ),
            ArtistGrouping::AlbumArtist => format!(
                "WITH {ctes} \
                 SELECT a.id, a.name || ' ' || COALESCE(names.list, '') \
                 FROM artists a \
                 LEFT JOIN ( \
                     SELECT u.artist_id, GROUP_CONCAT(DISTINCT art.name) AS list \
                     FROM u \
                     JOIN track_artists ta ON ta.track_id = u.track_id \
                     JOIN artists art ON art.id = ta.artist_id \
                     WHERE art.id != u.artist_id \
                     GROUP BY u.artist_id \
                 ) names ON names.artist_id = a.id \
                 WHERE EXISTS (SELECT 1 FROM m WHERE m.artist_id = a.id)"
            ),
        };
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<std::result::Result<HashMap<_, _>, _>>()
            .map_err(LibraryError::Database)
    }

    fn artist_name(&self, id: i64) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT name FROM artists WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(LibraryError::Database)
    }

    fn tracks_by_artist(&self, artist_id: i64, grouping: ArtistGrouping) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        let listed = artist_membership_sql(grouping);
        if artist_id == crate::NO_METADATA_ARTIST_ID {
            let sql = format!(
                "WITH m AS ({listed}) \
                 SELECT {TRACK_COLUMNS_T} FROM tracks t \
                 LEFT JOIN albums al ON al.id = t.album_id \
                 WHERE NOT EXISTS (SELECT 1 FROM m WHERE m.track_id = t.id) \
                 ORDER BY COALESCE(al.year, 0), al.title COLLATE NOCASE, t.disc_number, t.track_number, t.title",
            );
            let mut stmt = conn.prepare_cached(&sql)?;
            let rows = stmt.query_map([], map_track_row)?;
            return rows
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(LibraryError::Database);
        }
        let ctes = membership_ctes(grouping);
        let sql = format!(
            "WITH {ctes} \
             SELECT DISTINCT {TRACK_COLUMNS_T} FROM tracks t \
             JOIN u ON u.track_id = t.id \
             LEFT JOIN albums al ON al.id = t.album_id \
             WHERE u.artist_id = ?1 \
             ORDER BY COALESCE(al.year, 0), al.title COLLATE NOCASE, t.disc_number, t.track_number, t.title",
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map([artist_id], map_track_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn track(&self, id: i64) -> Result<Option<Track>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {TRACK_COLUMNS} FROM tracks WHERE id = ?1");
        let mut stmt = conn.prepare_cached(&sql)?;
        let mut rows = stmt.query_map([id], map_track_row)?;
        match rows.next() {
            Some(row) => Ok(Some(row.map_err(LibraryError::Database)?)),
            None => Ok(None),
        }
    }

    fn liked_tracks(&self) -> Result<Vec<Track>> {
        self.tracks_for_playlist(self.liked_playlist_id)
    }

    fn all_tracks(&self) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        display_ordered_tracks(&conn, "")
    }

    fn track_count(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached("SELECT COUNT(*) FROM tracks")?;
        let count = stmt.query_row([], |row| row.get(0))?;
        Ok(count)
    }

    fn set_liked(&self, track_id: i64, liked: bool) -> Result<()> {
        let liked_playlist_id = self.liked_playlist_id;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if liked {
            let next_position: i64 = tx
                .query_row(
                    "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
                    [liked_playlist_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            tx.execute(
                "INSERT OR IGNORE INTO playlist_tracks (playlist_id, position, track_id) VALUES (?1, ?2, ?3)",
                rusqlite::params![liked_playlist_id, next_position, track_id],
            )?;
        } else {
            let position: Option<i64> = tx
                .query_row(
                    "SELECT MIN(position) FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
                    rusqlite::params![liked_playlist_id, track_id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            if let Some(position) = position {
                tx.execute(
                    "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND position = ?2",
                    rusqlite::params![liked_playlist_id, position],
                )?;
                tx.execute(
                    "UPDATE playlist_tracks SET position = position - 1 \
                     WHERE playlist_id = ?1 AND position > ?2",
                    rusqlite::params![liked_playlist_id, position],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn like_many(&self, track_ids: &[i64]) -> Result<()> {
        if track_ids.is_empty() {
            return Ok(());
        }
        let liked_playlist_id = self.liked_playlist_id;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut next_position: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
                [liked_playlist_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        for track_id in track_ids {
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO playlist_tracks (playlist_id, position, track_id) VALUES (?1, ?2, ?3)",
                rusqlite::params![liked_playlist_id, next_position, track_id],
            )?;
            next_position += inserted as i64;
        }
        tx.commit()?;
        Ok(())
    }

    fn set_track_genres(&self, track_id: i64, genres: &[String]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM track_genres WHERE track_id = ?1", [track_id])?;

        for (position, name) in genres.iter().enumerate() {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let key = name.to_lowercase();
            tx.execute(
                "INSERT OR IGNORE INTO genres (name, key) VALUES (?1, ?2)",
                [name, key.as_str()],
            )?;
            let genre_id: i64 =
                tx.query_row("SELECT id FROM genres WHERE key = ?1", [&key], |row| {
                    row.get(0)
                })?;
            tx.execute(
                "INSERT OR IGNORE INTO track_genres (track_id, genre_id, position) VALUES (?1, ?2, ?3)",
                [track_id, genre_id, position as i64],
            )?;
        }

        tx.execute(
            "DELETE FROM genres WHERE NOT EXISTS \
             (SELECT 1 FROM track_genres WHERE track_genres.genre_id = genres.id)",
            [],
        )?;
        tx.commit()?;
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
        let mut stmt = conn.prepare_cached(
            r#"
            SELECT
                p.id,
                p.name,
                p.created_at,
                COUNT(pt.track_id) AS track_count
            FROM playlists p
            LEFT JOIN playlist_tracks pt ON pt.playlist_id = p.id
            WHERE p.id != ?1
            GROUP BY p.id
            ORDER BY p.created_at ASC, p.id ASC
            "#,
        )?;
        let rows = stmt.query_map([self.liked_playlist_id], |row| {
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
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
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
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
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

    fn move_track_in_playlist(&self, playlist_id: i64, from: usize, to: usize) -> Result<()> {
        if from == to {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut ids: Vec<i64> = {
            let mut stmt = tx.prepare(
                "SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position",
            )?;
            let rows = stmt.query_map([playlist_id], |row| row.get(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        if from >= ids.len() || to >= ids.len() {
            tx.commit()?;
            return Ok(());
        }
        let moved = ids.remove(from);
        ids.insert(to, moved);
        tx.execute(
            "DELETE FROM playlist_tracks WHERE playlist_id = ?1",
            [playlist_id],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO playlist_tracks (playlist_id, position, track_id) VALUES (?1, ?2, ?3)",
            )?;
            for (position, track_id) in ids.iter().enumerate() {
                stmt.execute(rusqlite::params![playlist_id, position as i64, track_id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn move_liked_track(&self, from: usize, to: usize) -> Result<()> {
        self.move_track_in_playlist(self.liked_playlist_id, from, to)
    }

    fn tracks_for_playlist(&self, playlist_id: i64) -> Result<Vec<Track>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT {PLAYLIST_ENTRY_COLUMNS} FROM playlist_tracks pt \
             JOIN media_items m ON m.id = pt.track_id \
             LEFT JOIN tracks t ON t.id = m.id \
             LEFT JOIN media_bindings b ON b.id = ( \
                 SELECT id FROM media_bindings WHERE item_id = m.id \
                 ORDER BY present DESC, id LIMIT 1) \
             WHERE pt.playlist_id = ?1 \
             ORDER BY pt.position",
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map([playlist_id], map_track_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn tracks_by_keys(&self, keys: &[(String, i32)]) -> Result<Vec<Track>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let mut paths: Vec<&str> = keys.iter().map(|(p, _)| p.as_str()).collect();
        paths.sort_unstable();
        paths.dedup();
        let mut out = Vec::new();
        for chunk in paths.chunks(512) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!("SELECT {TRACK_COLUMNS} FROM tracks WHERE path IN ({placeholders})");
            let mut stmt = conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::ToSql> =
                chunk.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
            let rows = stmt.query_map(params.as_slice(), map_track_row)?;
            for row in rows {
                out.push(row.map_err(LibraryError::Database)?);
            }
        }
        Ok(out)
    }

    fn playlists_containing_track(&self, track_id: i64) -> Result<Vec<i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT DISTINCT playlist_id FROM playlist_tracks WHERE track_id = ?1 AND playlist_id != ?2",
        )?;
        let rows = stmt.query_map([track_id, self.liked_playlist_id], |row| {
            row.get::<_, i64>(0)
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn lyrics_for_track(&self, track_id: i64) -> Result<Option<StoredLyrics>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT source, text, not_found FROM lyrics WHERE track_id = ?1",
                [track_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, i64>(2)? != 0,
                    ))
                },
            )
            .optional()
            .map_err(LibraryError::Database)?;
        let Some((source, blob, not_found)) = row else {
            return Ok(None);
        };
        if not_found {
            return Ok(Some(StoredLyrics {
                source,
                text: String::new(),
                not_found: true,
            }));
        }
        Ok(decompress_lyrics(&blob).map(|text| StoredLyrics {
            source,
            text,
            not_found: false,
        }))
    }

    fn upsert_lyrics(
        &self,
        track_id: i64,
        text: &str,
        source: &str,
        not_found: bool,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO lyrics (track_id, source, text, not_found, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(track_id) DO UPDATE SET \
                source = excluded.source, \
                text = excluded.text, \
                not_found = excluded.not_found, \
                updated_at = excluded.updated_at",
            rusqlite::params![
                track_id,
                source,
                compress_lyrics(text),
                not_found as i64,
                unix_now()
            ],
        )?;
        Ok(())
    }

    fn track_artists_map(&self, track_ids: &[i64]) -> Result<HashMap<i64, Vec<String>>> {
        if track_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let ids = format!(
            "[{}]",
            track_ids
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "WITH ids(id) AS (SELECT value FROM json_each(?1)) \
             SELECT ta.track_id, a.name, ta.position FROM track_artists ta \
             JOIN artists a ON a.id = ta.artist_id \
             WHERE ta.track_id IN (SELECT id FROM ids) \
             UNION ALL \
             SELECT m.id, m.artist, 0 FROM media_items m \
             WHERE m.id IN (SELECT id FROM ids) AND m.artist <> '' \
             AND NOT EXISTS (SELECT 1 FROM track_artists x WHERE x.track_id = m.id) \
             ORDER BY 1, 3",
        )?;
        let rows = stmt.query_map([ids], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map: HashMap<i64, Vec<String>> = HashMap::new();
        for row in rows {
            let (id, name) = row.map_err(LibraryError::Database)?;
            map.entry(id).or_default().push(name);
        }
        Ok(map)
    }

    fn artist_album_covers(&self, grouping: ArtistGrouping) -> Result<HashMap<i64, Vec<i64>>> {
        let conn = self.conn.lock().unwrap();
        let ctes = membership_ctes(grouping);
        let sql = format!(
            "WITH {ctes} \
             SELECT u.artist_id, al.cover_art_id, COALESCE(al.year, 9999999999) AS sort_year \
             FROM u \
             JOIN tracks t ON t.id = u.track_id \
             JOIN albums al ON al.id = t.album_id \
             WHERE al.cover_art_id IS NOT NULL \
             AND EXISTS (SELECT 1 FROM m WHERE m.artist_id = u.artist_id) \
             GROUP BY u.artist_id, al.id \
             ORDER BY u.artist_id, sort_year ASC, al.title COLLATE NOCASE"
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
        let mut map: HashMap<i64, Vec<i64>> = HashMap::new();
        for row in rows {
            let (artist_id, cover_art_id) = row.map_err(LibraryError::Database)?;
            let covers = map.entry(artist_id).or_default();
            if covers.len() < 3 && !covers.contains(&cover_art_id) {
                covers.push(cover_art_id);
            }
        }
        Ok(map)
    }

    fn cover_art_hashes(&self) -> Result<Vec<(String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT hash, id FROM cover_art")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn reconcile_local_sources(&self, folders: &[LocalFolder]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("UPDATE sources SET enabled = 0 WHERE kind = 'local'", [])?;
        for folder in folders {
            let name = Path::new(&folder.path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| folder.path.clone());
            tx.execute(
                "INSERT INTO sources (kind, name, uri, enabled, available) \
                 VALUES ('local', ?1, ?2, 1, ?3) \
                 ON CONFLICT(kind, uri) DO UPDATE SET enabled = 1, available = excluded.available",
                rusqlite::params![name, folder.path, folder.available],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn has_media_under(&self, root: &str) -> Result<bool> {
        let separator = std::path::MAIN_SEPARATOR;
        let prefix = if root.ends_with(separator) {
            root.to_string()
        } else {
            format!("{root}{separator}")
        };
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM media_bindings b JOIN sources s ON s.id = b.source_id \
             WHERE s.kind = 'local' AND b.present = 1 \
             AND substr(b.source_key, 1, length(?1)) = ?1)",
            [prefix],
            |row| row.get(0),
        )?)
    }

    fn has_unplaced_media(&self) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM media_bindings WHERE source_id = ?1 AND present = 1)",
            [PLACEHOLDER_SOURCE_ID],
            |row| row.get(0),
        )?)
    }

    fn sources(&self) -> Result<Vec<SourceSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.kind, s.uri, s.enabled, s.available, \
             (SELECT COUNT(*) FROM media_bindings b \
              WHERE b.source_id = s.id AND b.present = 1) \
             FROM sources s WHERE s.uri <> '' ORDER BY s.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SourceSummary {
                id: row.get(0)?,
                kind: row.get(1)?,
                uri: row.get(2)?,
                enabled: row.get(3)?,
                available: row.get(4)?,
                track_count: row.get(5)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn reconcile_remote_sources(&self, kind: &str, sources: &[RemoteSource]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("UPDATE sources SET enabled = 0 WHERE kind = ?1", [kind])?;
        for source in sources {
            tx.execute(
                "INSERT INTO sources (kind, name, uri, enabled) VALUES (?1, ?2, ?3, 1) \
                 ON CONFLICT(kind, uri) DO UPDATE SET enabled = 1, name = excluded.name",
                rusqlite::params![kind, source.name, source.uri],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn set_source_available(&self, source_id: i64, available: bool) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE sources SET available = ?1 WHERE id = ?2 AND available <> ?1",
            rusqlite::params![available, source_id],
        )?;
        Ok(changed > 0)
    }

    fn remote_cover_hashes(&self, source_id: i64) -> Result<HashMap<String, String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT rt.cover_key, rt.cover_hash FROM remote_tracks rt \
             JOIN media_bindings b ON b.id = rt.binding_id \
             JOIN cover_art c ON c.hash = rt.cover_hash \
             WHERE b.source_id = ?1 AND rt.cover_key IS NOT NULL",
        )?;
        let rows = stmt.query_map([source_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<std::result::Result<HashMap<_, _>, _>>()
            .map_err(LibraryError::Database)
    }

    fn apply_remote_listing(
        &self,
        source_id: i64,
        songs: &[RemoteSong],
        covers: &[RemoteCover],
    ) -> Result<RemoteSyncReport> {
        let mut conn = Connection::open(&self.db_path)?;
        apply_pragmas(&conn)?;
        apply_remote_listing(&mut conn, source_id, songs, covers)
    }

    fn items_for_remote_keys(&self, source_id: i64, keys: &[String]) -> Result<Vec<i64>> {
        let conn = self.conn.lock().unwrap();
        let keys_json = serde_json::to_string(keys).unwrap_or_else(|_| "[]".into());
        let mut stmt = conn.prepare(
            "SELECT DISTINCT item_id FROM media_bindings \
             WHERE source_id = ?1 AND source_key IN (SELECT value FROM json_each(?2))",
        )?;
        let rows = stmt.query_map(rusqlite::params![source_id, keys_json], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn playback_locators(&self, item_id: i64) -> Result<Vec<(String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.kind, b.source_id, b.source_key, b.start_offset_ms, rt.suffix, rt.content_type \
             FROM media_bindings b JOIN sources s ON s.id = b.source_id \
             LEFT JOIN remote_tracks rt ON rt.binding_id = b.id \
             WHERE b.item_id = ?1 AND b.present = 1 AND s.enabled = 1 AND s.available = 1 \
             ORDER BY CASE s.kind WHEN 'local' THEN 0 WHEN 'subsonic' THEN 1 ELSE 2 END, \
             s.id, b.id",
        )?;
        let rows = stmt.query_map([item_id], |row| {
            let kind: String = row.get(0)?;
            let key: String = row.get(2)?;
            let offset: i64 = row.get(3)?;
            if kind == "local" {
                return Ok((key, offset));
            }
            let suffix: Option<String> = row.get(4)?;
            let content_type: Option<String> = row.get(5)?;
            Ok((
                crate::remote::locator(
                    row.get(1)?,
                    &key,
                    &crate::remote::suffix_for(suffix.as_deref(), content_type.as_deref()),
                ),
                offset,
            ))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn invalidate_scan_fingerprint(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM scan_meta WHERE key = 'fingerprint'", [])?;
        Ok(())
    }

    fn refresh_item_snapshots(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(REFRESH_ITEM_SNAPSHOTS, [unix_now()])?;
        Ok(())
    }

    fn open_scan_session(&self) -> Result<Box<dyn ScanWrite>> {
        Ok(Box::new(ScanSession::open(&self.db_path)?))
    }

    fn scan_fingerprint(&self) -> Result<Option<String>> {
        self.scan_meta_value("fingerprint")
    }

    fn scan_folders(&self) -> Result<Option<String>> {
        self.scan_meta_value("folders")
    }

    fn set_scan_meta(&self, fingerprint: &str, folders: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO scan_meta (key, value) VALUES ('fingerprint', ?1) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [fingerprint],
        )?;
        tx.execute(
            "INSERT INTO scan_meta (key, value) VALUES ('folders', ?1) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [folders],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn vacuum(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("VACUUM; ANALYZE;")?;
        Ok(())
    }

    fn record_play(&self, play: &NewPlay, targets: &[&str]) -> Result<i64> {
        let mut conn = self.scrobble_conn.lock().unwrap();
        let tx = conn.transaction()?;
        let id: i64 = tx.query_row(
            "INSERT INTO plays (track_id, artist, title, album, album_artist, track_number, \
             duration_secs, played_secs, started_at, qualified) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
             ON CONFLICT(started_at, artist, title) DO UPDATE SET \
             played_secs = MAX(COALESCE(played_secs, 0), COALESCE(excluded.played_secs, 0)), \
             qualified = MAX(qualified, excluded.qualified) \
             RETURNING id",
            rusqlite::params![
                play.track_id,
                play.artist,
                play.title,
                play.album,
                play.album_artist,
                play.track_number.map(i64::from),
                play.duration_secs.map(|secs| secs as i64),
                play.played_secs.map(|secs| secs as i64),
                play.started_at as i64,
                play.qualified as i64,
            ],
            |row| row.get(0),
        )?;
        if play.qualified {
            let now = unix_now();
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO play_deliveries (play_id, target, state, attempts, \
                 last_error, updated_at) VALUES (?1, ?2, 0, 0, NULL, ?3)",
            )?;
            for target in targets {
                stmt.execute(rusqlite::params![id, target, now])?;
            }
            drop(stmt);
        }
        tx.commit()?;
        Ok(id)
    }

    fn record_love(&self, love: &NewLove, targets: &[&str]) -> Result<i64> {
        let mut conn = self.scrobble_conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO loves (track_id, artist, title, loved, at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                love.track_id,
                love.artist,
                love.title,
                love.loved as i64,
                love.at as i64,
            ],
        )?;
        let id = tx.last_insert_rowid();
        let now = unix_now();
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO love_deliveries (love_id, target, state, attempts, \
                 last_error, updated_at) VALUES (?1, ?2, 0, 0, NULL, ?3)",
            )?;
            for target in targets {
                stmt.execute(rusqlite::params![id, target, now])?;
            }
        }
        tx.commit()?;
        Ok(id)
    }

    fn pending_plays(&self, target: &str, max: usize) -> Result<Vec<PendingPlay>> {
        let conn = self.scrobble_conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT p.id, p.artist, p.title, p.album, p.album_artist, p.track_number, \
             p.duration_secs, p.started_at FROM play_deliveries d \
             JOIN plays p ON p.id = d.play_id \
             WHERE d.target = ?1 AND d.state = 0 ORDER BY d.play_id LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![target, max as i64], |row| {
            Ok(PendingPlay {
                id: row.get(0)?,
                artist: row.get(1)?,
                title: row.get(2)?,
                album: row.get(3)?,
                album_artist: row.get(4)?,
                track_number: row.get::<_, Option<i64>>(5)?.map(|n| n as u32),
                duration_secs: row.get::<_, Option<i64>>(6)?.map(|n| n as u64),
                started_at: row.get::<_, i64>(7)? as u64,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn pending_loves(&self, target: &str, max: usize) -> Result<Vec<PendingLove>> {
        let conn = self.scrobble_conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT l.id, l.artist, l.title, l.loved, l.at FROM love_deliveries d \
             JOIN loves l ON l.id = d.love_id \
             WHERE d.target = ?1 AND d.state = 0 ORDER BY d.love_id LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![target, max as i64], |row| {
            Ok(PendingLove {
                id: row.get(0)?,
                artist: row.get(1)?,
                title: row.get(2)?,
                loved: row.get::<_, i64>(3)? != 0,
                at: row.get::<_, i64>(4)? as u64,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(LibraryError::Database)
    }

    fn settle_plays(&self, ids: &[i64], target: &str, outcome: &DeliveryOutcome) -> Result<()> {
        self.settle_deliveries("play_deliveries", "play_id", ids, target, outcome)
    }

    fn settle_loves(&self, ids: &[i64], target: &str, outcome: &DeliveryOutcome) -> Result<()> {
        self.settle_deliveries("love_deliveries", "love_id", ids, target, outcome)
    }

    fn pending_scrobble_count(&self, targets: &[&str]) -> Result<usize> {
        if targets.is_empty() {
            return Ok(0);
        }
        let placeholders = (1..=targets.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT (SELECT COUNT(DISTINCT play_id) FROM play_deliveries \
             WHERE state = 0 AND target IN ({placeholders})) \
             + (SELECT COUNT(DISTINCT love_id) FROM love_deliveries \
             WHERE state = 0 AND target IN ({placeholders}))"
        );
        let conn = self.scrobble_conn.lock().unwrap();
        let count: i64 =
            conn.query_row(&sql, rusqlite::params_from_iter(targets.iter()), |row| {
                row.get(0)
            })?;
        Ok(count.max(0) as usize)
    }

    fn trim_pending_deliveries(&self, cap: usize) -> Result<usize> {
        let mut conn = self.scrobble_conn.lock().unwrap();
        let tx = conn.transaction()?;
        let total: i64 = tx.query_row(
            "SELECT (SELECT COUNT(DISTINCT play_id) FROM play_deliveries WHERE state = 0) \
             + (SELECT COUNT(DISTINCT love_id) FROM love_deliveries WHERE state = 0)",
            [],
            |row| row.get(0),
        )?;
        let cap = cap as i64;
        if total <= cap {
            tx.commit()?;
            return Ok(0);
        }
        let now = unix_now();
        let mut remaining = total - cap;
        let mut dropped = drop_oldest_items(&tx, "play_deliveries", "play_id", remaining, now)?;
        remaining -= dropped;
        if remaining > 0 {
            dropped += drop_oldest_items(&tx, "love_deliveries", "love_id", remaining, now)?;
        }
        tx.commit()?;
        Ok(dropped.max(0) as usize)
    }
}

fn drop_oldest_items(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    key: &str,
    limit: i64,
    now: i64,
) -> Result<i64> {
    if limit <= 0 {
        return Ok(0);
    }
    let victims =
        format!("SELECT DISTINCT {key} FROM {table} WHERE state = 0 ORDER BY {key} LIMIT ?1");
    let ids: Vec<i64> = {
        let mut stmt = tx.prepare(&victims)?;
        let rows = stmt.query_map([limit], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };
    if ids.is_empty() {
        return Ok(0);
    }
    let sql = format!(
        "UPDATE {table} SET state = 2, last_error = 'queue overflow', updated_at = ?1 \
         WHERE {key} = ?2 AND state = 0"
    );
    let mut stmt = tx.prepare(&sql)?;
    for id in &ids {
        stmt.execute(rusqlite::params![now, id])?;
    }
    Ok(ids.len() as i64)
}

impl SqliteLibrary {
    fn settle_deliveries(
        &self,
        table: &str,
        key: &str,
        ids: &[i64],
        target: &str,
        outcome: &DeliveryOutcome,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let sql = match outcome {
            DeliveryOutcome::Sent => format!(
                "UPDATE {table} SET state = 1, attempts = attempts + 1, last_error = NULL, \
                 updated_at = ?2 WHERE {key} = ?1 AND target = ?3 AND state = 0"
            ),
            DeliveryOutcome::Dropped(_) => format!(
                "UPDATE {table} SET state = 2, attempts = attempts + 1, last_error = ?4, \
                 updated_at = ?2 WHERE {key} = ?1 AND target = ?3 AND state = 0"
            ),
            DeliveryOutcome::Deferred(_) => format!(
                "UPDATE {table} SET attempts = attempts + 1, last_error = ?4, updated_at = ?2 \
                 WHERE {key} = ?1 AND target = ?3 AND state = 0"
            ),
        };
        let message = match outcome {
            DeliveryOutcome::Sent => None,
            DeliveryOutcome::Dropped(msg) | DeliveryOutcome::Deferred(msg) => Some(msg.as_str()),
        };
        let now = unix_now();
        let mut conn = self.scrobble_conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(&sql)?;
            for id in ids {
                match message {
                    Some(msg) => stmt.execute(rusqlite::params![id, now, target, msg])?,
                    None => stmt.execute(rusqlite::params![id, now, target])?,
                };
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn scan_meta_value(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let value = conn
            .query_row("SELECT value FROM scan_meta WHERE key = ?1", [key], |row| {
                row.get::<_, String>(0)
            })
            .optional()?;
        Ok(value)
    }
}

/// Batched scan writer on a dedicated WAL connection. Holds one transaction
/// open across `SCAN_BATCH_SIZE` track inserts, then commits and reopens — so
/// the whole rescan costs a handful of `fsync`s instead of thousands. All
/// id resolution (artists / albums / covers) is served from in-memory caches,
/// eliminating the per-track `SELECT`s the old per-op path did.
pub struct ScanSession {
    conn: Connection,
    in_tx: bool,
    tx_started: std::time::Instant,
    finishing: bool,
    uncommitted: usize,
    artist_cache: HashMap<String, i64>,
    genre_cache: HashMap<String, i64>,
    album_cache: HashMap<(String, Option<i32>), i64>,
    cover_cache: HashMap<String, i64>,
    pending_by_hash: HashMap<String, Vec<ScanTrack>>,
    roots: Vec<LocalRoot>,
    bindings: HashMap<(String, i64), (i64, i64)>,
    scan_id: i64,
    arrivals: Option<Vec<ScanTrack>>,
    written: HashMap<i64, i64>,
    placed: std::collections::HashSet<(i64, i64)>,
    seen: std::collections::HashSet<(String, i64)>,
}

impl ScanSession {
    fn open(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        apply_pragmas(&conn)?;

        // Covers survive clear(), so seed the hash→id cache up front: most
        // covers on a re-scan resolve here with neither a thumbnail nor a SELECT.
        let mut cover_cache = HashMap::new();
        {
            let mut stmt = conn.prepare("SELECT hash, id FROM cover_art")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            for row in rows {
                let (hash, id) = row?;
                cover_cache.insert(hash, id);
            }
        }

        let roots = load_local_roots(&conn)?;
        let mut bindings = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT b.id, b.item_id, b.source_key, b.start_offset_ms FROM media_bindings b \
                 JOIN sources s ON s.id = b.source_id WHERE s.kind = 'local'",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    (row.get::<_, String>(2)?, row.get::<_, i64>(3)?),
                    (row.get::<_, i64>(0)?, row.get::<_, i64>(1)?),
                ))
            })?;
            for row in rows {
                let (key, value) = row?;
                bindings.insert(key, value);
            }
        }
        let scan_id: i64 = conn.query_row(
            "SELECT COALESCE(MAX(last_seen_scan), 0) + 1 FROM media_bindings",
            [],
            |row| row.get(0),
        )?;
        let has_items: bool =
            conn.query_row("SELECT EXISTS(SELECT 1 FROM media_items)", [], |row| {
                row.get(0)
            })?;
        let hold_arrivals = has_items || roots.len() > 1;

        let session = Self {
            conn,
            in_tx: false,
            tx_started: std::time::Instant::now(),
            finishing: false,
            uncommitted: 0,
            artist_cache: HashMap::new(),
            genre_cache: HashMap::new(),
            album_cache: HashMap::new(),
            cover_cache,
            pending_by_hash: HashMap::new(),
            roots,
            bindings,
            scan_id,
            arrivals: hold_arrivals.then(Vec::new),
            written: HashMap::new(),
            placed: std::collections::HashSet::new(),
            seen: std::collections::HashSet::new(),
        };
        Ok(session)
    }

    fn begin(&mut self) -> Result<()> {
        if !self.in_tx {
            self.conn.execute_batch("BEGIN IMMEDIATE")?;
            self.in_tx = true;
            self.tx_started = std::time::Instant::now();
        }
        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        if self.in_tx {
            self.conn.execute_batch("COMMIT")?;
            self.in_tx = false;
            self.uncommitted = 0;
        }
        Ok(())
    }

    fn maybe_commit(&mut self) -> Result<()> {
        self.uncommitted += 1;
        if !self.finishing
            && (self.uncommitted >= SCAN_BATCH_SIZE || self.tx_started.elapsed() >= SCAN_BATCH_TIME)
        {
            self.commit()?;
        }
        Ok(())
    }

    fn resolve_artist(&mut self, name: &str) -> Result<i64> {
        if let Some(&id) = self.artist_cache.get(name) {
            return Ok(id);
        }
        let sort_name = compute_sort_name(name);
        self.conn.execute(
            "INSERT INTO artists (name, sort_name) VALUES (?1, ?2)",
            [name, &sort_name],
        )?;
        let id = self.conn.last_insert_rowid();
        self.artist_cache.insert(name.to_string(), id);
        Ok(id)
    }

    fn resolve_genre(&mut self, name: &str) -> Result<i64> {
        let key = name.to_lowercase();
        if let Some(&id) = self.genre_cache.get(&key) {
            return Ok(id);
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO genres (name, key) VALUES (?1, ?2)",
            [name, key.as_str()],
        )?;
        let id: i64 =
            self.conn
                .query_row("SELECT id FROM genres WHERE key = ?1", [&key], |row| {
                    row.get(0)
                })?;
        self.genre_cache.insert(key, id);
        Ok(id)
    }

    /// The album row only; its cover is left NULL and filled afterwards by
    /// [`LibraryRepository::resolve_album_covers`], which picks deterministically
    /// instead of taking whichever track the scan happened to finish first.
    fn resolve_album(&mut self, title: &str, year: Option<i32>) -> Result<i64> {
        let key = (title.to_string(), year);
        if let Some(&id) = self.album_cache.get(&key) {
            return Ok(id);
        }
        self.conn.execute(
            "INSERT INTO albums (title, year, cover_art_id) VALUES (?1, ?2, NULL)",
            rusqlite::params![title, year],
        )?;
        let id = self.conn.last_insert_rowid();
        self.album_cache.insert(key, id);
        Ok(id)
    }

    fn start_offset(track: &ScanTrack) -> i64 {
        track.start_offset_ms.unwrap_or(0) as i64
    }

    fn title_of(track: &ScanTrack) -> String {
        track
            .title
            .clone()
            .unwrap_or_else(|| fallback_title_from_path(&track.path))
    }

    fn mint_item(&mut self, track: &ScanTrack) -> Result<i64> {
        let start_offset_ms = Self::start_offset(track);
        let cover_art_id = track
            .cover_hash
            .as_ref()
            .and_then(|h| self.cover_cache.get(h).copied());
        let title = Self::title_of(track);
        let (binding_id, item_id) = create_item(
            &self.conn,
            &self.spec(track),
            &ItemSnapshot {
                title: &title,
                artist: track.artist_names.first().map_or("", String::as_str),
                album: track.album_title.as_deref(),
                duration_ms: track.duration_ms.map(|n| n as i64),
                cover_art_id,
            },
        )?;
        self.bindings
            .insert((track.path.clone(), start_offset_ms), (binding_id, item_id));
        Ok(item_id)
    }

    fn adopt_item(&mut self, item_id: i64, track: &ScanTrack, tier: Tier) -> Result<()> {
        let start_offset_ms = Self::start_offset(track);
        let binding_id = bind_item(&self.conn, item_id, &self.spec(track))?;
        self.conn.execute(
            "INSERT INTO adoptions (item_id, binding_id, tier, at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![item_id, binding_id, tier.as_str(), unix_now()],
        )?;
        self.bindings
            .insert((track.path.clone(), start_offset_ms), (binding_id, item_id));
        Ok(())
    }

    fn spec<'a>(&self, track: &'a ScanTrack) -> BindingSpec<'a> {
        BindingSpec {
            source_id: source_for_path(&self.roots, &track.path),
            key: &track.path,
            start_offset_ms: Self::start_offset(track),
            file_size: track.file_size.map(|size| size as i64),
            scan_id: self.scan_id,
        }
    }

    fn descriptor_of(&self, track: &ScanTrack) -> Descriptor {
        Descriptor {
            title: Self::title_of(track),
            artist: track.artist_names.first().cloned().unwrap_or_default(),
            artist_aliases: track.artist_names.iter().skip(1).cloned().collect(),
            album: track.album_title.clone(),
            duration_ms: track.duration_ms.map(|n| n as i64),
            files: track
                .file_size
                .map(|size| {
                    (
                        size as i64,
                        file_piece(track.is_cue, Self::start_offset(track)),
                    )
                })
                .into_iter()
                .collect(),
            sources: vec![source_for_path(&self.roots, &track.path)],
            ..Default::default()
        }
    }

    fn place(&mut self, track: ScanTrack, item_id: i64) -> Result<()> {
        let source_id = source_for_path(&self.roots, &track.path);
        self.placed.insert((item_id, source_id));
        match self.written.get(&item_id) {
            None => self.write_track(track, item_id),
            Some(&written_by) if source_id < written_by => {
                for sql in UNWRITE_TRACK {
                    self.conn.execute(sql, [item_id])?;
                }
                self.write_track(track, item_id)
            }
            Some(_) => self.maybe_commit(),
        }
    }

    fn project_remote_tracks(&mut self) -> Result<()> {
        let rows: Vec<(i64, ScanTrack)> = {
            let mut stmt = self.conn.prepare(PROJECTABLE_REMOTE_TRACKS)?;
            stmt.query_map([], |row| {
                let source_id: i64 = row.get(1)?;
                let key: String = row.get(2)?;
                let suffix: Option<String> = row.get(12)?;
                let content_type: Option<String> = row.get(13)?;
                let artist: Option<String> = row.get(4)?;
                let album_artist: Option<String> = row.get(6)?;
                let genre: Option<String> = row.get(10)?;
                let duration_ms: Option<i64> = row.get(11)?;
                Ok((
                    row.get(0)?,
                    ScanTrack {
                        path: crate::remote::locator(
                            source_id,
                            &key,
                            &crate::remote::suffix_for(suffix.as_deref(), content_type.as_deref()),
                        ),
                        title: Some(row.get(3)?),
                        album_title: row.get(5)?,
                        artist_names: artist.into_iter().filter(|a| !a.is_empty()).collect(),
                        album_artist_names: album_artist
                            .into_iter()
                            .filter(|a| !a.is_empty())
                            .collect(),
                        track_number: row.get(7)?,
                        disc_number: row.get(8)?,
                        year: row.get(9)?,
                        genres: genre.into_iter().filter(|g| !g.is_empty()).collect(),
                        duration_ms: duration_ms.map(|d| d.max(0) as u64),
                        cover_hash: row.get(15)?,
                        start_offset_ms: None,
                        bitrate: row.get(14)?,
                        is_cue: false,
                        lyrics: None,
                        file_size: None,
                    },
                ))
            })?
            .collect::<std::result::Result<_, _>>()?
        };
        for (item_id, mut track) in rows {
            if self.written.contains_key(&item_id) {
                continue;
            }
            if track
                .cover_hash
                .as_ref()
                .is_some_and(|hash| !self.cover_cache.contains_key(hash))
            {
                track.cover_hash = None;
            }
            self.write_track(track, item_id)?;
        }
        Ok(())
    }

    fn absorb_item(&mut self, live: i64, orphan: i64, tier: Tier) -> Result<()> {
        self.conn.pragma_update(None, "defer_foreign_keys", "ON")?;
        let adopted: Vec<i64> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM media_bindings WHERE item_id = ?1")?;
            stmt.query_map([live], |row| row.get(0))?
                .collect::<std::result::Result<_, _>>()?
        };
        for sql in ABSORB_ITEM {
            self.conn.execute(sql, [orphan, live])?;
        }
        let now = unix_now();
        for binding_id in adopted {
            self.conn.execute(
                "INSERT INTO adoptions (item_id, binding_id, tier, at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![orphan, binding_id, tier.as_str(), now],
            )?;
        }
        if let Some(source_id) = self.written.remove(&live) {
            self.written.insert(orphan, source_id);
        }
        Ok(())
    }

    fn retire_unplaced_bindings(&mut self) -> Result<()> {
        let unplaced: Vec<(i64, String)> = {
            let mut stmt = self.conn.prepare(
                "SELECT id, source_key FROM media_bindings WHERE source_id = ?1 AND present = 1",
            )?;
            stmt.query_map([PLACEHOLDER_SOURCE_ID], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<std::result::Result<_, _>>()?
        };
        for (binding_id, key) in unplaced {
            if root_for_path(&self.roots, &key).is_none_or(|root| root.available) {
                self.conn.execute(
                    "UPDATE media_bindings SET present = 0 WHERE id = ?1",
                    [binding_id],
                )?;
            }
        }
        Ok(())
    }

    fn revive_lost_items(&mut self) -> Result<()> {
        let identities = load_identities(&self.conn)?;
        let occupies = |b: &BindingFacts| b.enabled && b.present;
        let lost: Vec<Descriptor> = identities
            .iter()
            .filter(|identity| identity.cherished && !identity.playable())
            .map(|identity| identity.descriptor(&occupies))
            .collect();
        if lost.is_empty() {
            return Ok(());
        }
        let spare: Vec<Descriptor> = identities
            .iter()
            .filter(|identity| !identity.cherished && identity.playable())
            .map(|identity| identity.descriptor(&occupies))
            .collect();
        let assignments = match_tracks(&spare, &lost);
        let mut revived = 0usize;
        for (live, assignment) in spare.iter().zip(assignments) {
            if let Some((orphan, tier)) = assignment {
                self.absorb_item(live.item_id, orphan, tier)?;
                revived += 1;
            }
        }
        if revived > 0 {
            log::info!("Re-attached {revived} files to entries that had lost theirs");
        }
        Ok(())
    }

    fn settle_arrivals(&mut self, mut tracks: Vec<ScanTrack>) -> Result<()> {
        tracks.sort_by(|a, b| {
            (a.path.as_str(), Self::start_offset(a)).cmp(&(b.path.as_str(), Self::start_offset(b)))
        });
        tracks.dedup_by(|a, b| a.path == b.path && Self::start_offset(a) == Self::start_offset(b));
        let mut groups: std::collections::BTreeMap<i64, Vec<ScanTrack>> = Default::default();
        for track in tracks {
            groups
                .entry(source_for_path(&self.roots, &track.path))
                .or_default()
                .push(track);
        }
        let mut adopted = 0usize;
        for (_, group) in groups {
            let scan_id = self.scan_id;
            let occupies = |b: &BindingFacts| {
                b.enabled
                    && if b.local && b.available {
                        b.last_seen_scan == scan_id
                    } else {
                        b.present
                    }
            };
            let candidates: Vec<Descriptor> = load_identities(&self.conn)?
                .iter()
                .map(|identity| identity.descriptor(&occupies))
                .collect();
            let arrivals: Vec<Descriptor> = group
                .iter()
                .map(|track| self.descriptor_of(track))
                .collect();
            let assignments = match_tracks(&arrivals, &candidates);
            for (track, assignment) in group.into_iter().zip(assignments) {
                let path = track.path.clone();
                let settled = match assignment {
                    Some((item_id, tier)) => self.adopt_item(item_id, &track, tier).map(|()| {
                        adopted += 1;
                        item_id
                    }),
                    None => self.mint_item(&track),
                }
                .and_then(|item_id| self.place(track, item_id));
                if let Err(e) = settled {
                    log::error!("Failed to insert track {path}: {e}");
                }
            }
        }
        if adopted > 0 {
            log::info!("Re-attached {adopted} moved or renamed files to their library entries");
        }
        Ok(())
    }

    fn insert_track(&mut self, track: ScanTrack) -> Result<()> {
        let start_offset_ms = Self::start_offset(&track);
        let key = (track.path.clone(), start_offset_ms);
        if !self.seen.insert(key.clone()) {
            return Ok(());
        }
        let source_id = source_for_path(&self.roots, &track.path);
        if !self.bindings.contains_key(&key)
            && let Some(arrivals) = &mut self.arrivals
        {
            arrivals.push(track);
            return Ok(());
        }
        self.begin()?;
        let item_id = if let Some(&(binding_id, item_id)) = self.bindings.get(&key)
            && self.placed.contains(&(item_id, source_id))
        {
            self.conn
                .execute("DELETE FROM media_bindings WHERE id = ?1", [binding_id])?;
            self.bindings.remove(&key);
            self.mint_item(&track)?
        } else if let Some(&(binding_id, item_id)) = self.bindings.get(&key) {
            self.conn.execute(
                "UPDATE media_bindings SET present = 1, last_seen_scan = ?1, last_seen_at = ?2, \
                 source_id = ?3, file_size = ?4 WHERE id = ?5",
                rusqlite::params![
                    self.scan_id,
                    unix_now(),
                    source_id,
                    track.file_size.map(|size| size as i64),
                    binding_id
                ],
            )?;
            item_id
        } else {
            self.mint_item(&track)?
        };
        self.place(track, item_id)
    }

    fn write_track(&mut self, track: ScanTrack, item_id: i64) -> Result<()> {
        self.written
            .insert(item_id, source_for_path(&self.roots, &track.path));
        let cover_id = track
            .cover_hash
            .as_ref()
            .and_then(|h| self.cover_cache.get(h).copied());

        let mut artist_ids = Vec::with_capacity(track.artist_names.len());
        for (pos, name) in track.artist_names.iter().enumerate() {
            let id = self.resolve_artist(name)?;
            artist_ids.push((id, pos as i64));
        }

        let mut album_artist_ids = Vec::with_capacity(track.album_artist_names.len());
        for (pos, name) in track.album_artist_names.iter().enumerate() {
            let id = self.resolve_artist(name)?;
            album_artist_ids.push((id, pos as i64));
        }

        let mut genre_ids = Vec::with_capacity(track.genres.len());
        for name in &track.genres {
            genre_ids.push(self.resolve_genre(name)?);
        }

        let album_id = match &track.album_title {
            Some(title) => Some(self.resolve_album(title, track.year)?),
            None => None,
        };

        let title = Self::title_of(&track);
        let track_number = track.track_number.map(|n| n as i64);
        let disc_number = track.disc_number.unwrap_or(1) as i64;
        let duration_ms = track.duration_ms.map(|n| n as i64);
        let start_offset_ms = Self::start_offset(&track);

        // OR IGNORE: the same file can appear under two overlapping scan roots.
        // The UNIQUE(path, start_offset_ms) index drops the duplicate instead of
        // failing the statement.
        let inserted = self.conn.execute(
            r#"INSERT OR IGNORE INTO tracks
                (id, path, title, album_id, track_number, disc_number, duration_ms, year, cover_art_id, start_offset_ms, bitrate, is_cue)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"#,
            rusqlite::params![
                item_id,
                track.path,
                title,
                album_id,
                track_number,
                disc_number,
                duration_ms,
                track.year,
                cover_id,
                start_offset_ms,
                track.bitrate,
                track.is_cue,
            ],
        )?;
        // A duplicate was ignored: bail before linking artists, otherwise
        // last_insert_rowid() would still point at the prior insert and we'd
        // attach track_artists rows to the wrong track.
        if inserted == 0 {
            return self.maybe_commit();
        }
        let track_id = item_id;

        for (artist_id, position) in &artist_ids {
            self.conn.execute(
                "INSERT INTO track_artists (track_id, artist_id, role, position) VALUES (?1, ?2, 'main', ?3)",
                [track_id, *artist_id, *position],
            )?;
        }

        for (artist_id, position) in &album_artist_ids {
            self.conn.execute(
                "INSERT OR IGNORE INTO track_album_artists (track_id, artist_id, position) VALUES (?1, ?2, ?3)",
                [track_id, *artist_id, *position],
            )?;
        }

        for (position, genre_id) in genre_ids.iter().enumerate() {
            self.conn.execute(
                "INSERT OR IGNORE INTO track_genres (track_id, genre_id, position) VALUES (?1, ?2, ?3)",
                [track_id, *genre_id, position as i64],
            )?;
        }

        if let Some(lyrics) = &track.lyrics {
            self.conn.execute(
                "INSERT INTO lyrics (track_id, source, text, not_found, updated_at) \
                 VALUES (?1, ?2, ?3, 0, ?4) \
                 ON CONFLICT(track_id) DO UPDATE SET source = excluded.source, \
                 text = excluded.text, not_found = 0, updated_at = excluded.updated_at",
                rusqlite::params![
                    track_id,
                    lyrics.source,
                    compress_lyrics(&lyrics.text),
                    unix_now(),
                ],
            )?;
        }

        self.maybe_commit()
    }
}

impl ScanWrite for ScanSession {
    fn flush(&mut self) -> Result<()> {
        self.commit()
    }

    fn clear(&mut self) -> Result<()> {
        self.begin()?;
        self.conn.execute_batch(CLEAR_CATALOG)?;
        Ok(())
    }

    fn add_cover(
        &mut self,
        hash: &str,
        small: Vec<u8>,
        large: Vec<u8>,
        source_path: &str,
        embedded: bool,
    ) -> Result<()> {
        if !self.cover_cache.contains_key(hash) {
            self.begin()?;
            self.conn.execute(
                "INSERT OR IGNORE INTO cover_art (hash, small, large, source_path, embedded) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![hash, small, large, source_path, embedded],
            )?;
            let id: i64 =
                self.conn
                    .query_row("SELECT id FROM cover_art WHERE hash = ?1", [hash], |row| {
                        row.get(0)
                    })?;
            self.cover_cache.insert(hash.to_string(), id);
            self.maybe_commit()?;
        }
        if let Some(tracks) = self.pending_by_hash.remove(hash) {
            for track in tracks {
                self.insert_track(track)?;
            }
        }
        Ok(())
    }

    fn add_track(&mut self, track: ScanTrack) -> Result<()> {
        if let Some(hash) = &track.cover_hash
            && !self.cover_cache.contains_key(hash)
        {
            // Cover thumbnail hasn't been inserted yet; hold the track until the
            // matching add_cover arrives (it always does for a claimed hash).
            self.pending_by_hash
                .entry(hash.clone())
                .or_default()
                .push(track);
            return Ok(());
        }
        self.insert_track(track)
    }

    fn finish(mut self: Box<Self>) -> Result<()> {
        self.begin()?;
        self.finishing = true;
        // Any track still waiting on a cover that never materialized (e.g. a
        // thumbnail-generation error) is inserted cover-less.
        let leftovers: Vec<ScanTrack> = self
            .pending_by_hash
            .drain()
            .flat_map(|(_, tracks)| tracks)
            .map(|mut t| {
                t.cover_hash = None;
                t
            })
            .collect();
        for track in leftovers {
            self.insert_track(track)?;
        }
        if let Some(arrivals) = self.arrivals.take()
            && !arrivals.is_empty()
        {
            self.settle_arrivals(arrivals)?;
        }
        self.conn
            .execute(RETIRE_UNSEEN_LOCAL_BINDINGS, [self.scan_id])?;
        self.retire_unplaced_bindings()?;
        self.revive_lost_items()?;
        self.project_remote_tracks()?;
        self.conn.execute(SWEEP_UNREFERENCED_ITEMS, [])?;
        self.commit()
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
    sha256_hex(data)
}

/// Hex-encoded SHA-256 of `data`. Used both for cover-art content addressing
/// and for the scan fingerprint, so the indexer can dedupe covers with the
/// exact same hash the DB stores.
pub fn sha256_hex(data: &[u8]) -> String {
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
    fn test_lyrics_compression_round_trips_unicode() {
        let text = "[00:01.00]Привет, мир\n[00:02.00]こんにちは\n[00:03.00]🎵";
        assert_eq!(
            decompress_lyrics(&compress_lyrics(text)).as_deref(),
            Some(text)
        );
    }

    #[test]
    fn test_lyrics_compression_shrinks_repetitive_text() {
        let chorus = "[00:30.00]We are the champions, my friend\n".repeat(40);
        let compressed = compress_lyrics(&chorus);
        assert!(compressed.len() * 3 < chorus.len());
        assert_eq!(
            decompress_lyrics(&compressed).as_deref(),
            Some(chorus.as_str())
        );
    }

    #[test]
    fn test_decompress_garbage_is_none() {
        assert!(decompress_lyrics(b"not a zlib stream").is_none());
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

    fn open_test_lib() -> SqliteLibrary {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let temp_dir = std::env::temp_dir().join("pawse-music-library-sqlite");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join(format!(
            "test-{}-{}.db",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_file(&db_path);
        SqliteLibrary::open_at(&db_path).unwrap()
    }

    #[test]
    fn test_upsert_lyrics_insert_then_update_single_row() {
        let lib = open_test_lib();
        let track_id = lib
            .upsert_track(
                &NewTrack {
                    path: "/m/u.flac".into(),
                    title: Some("U".into()),
                    ..Default::default()
                },
                None,
                &[],
            )
            .unwrap();

        lib.upsert_lyrics(track_id, "plain words", "embedded", false)
            .unwrap();
        let first = lib.lyrics_for_track(track_id).unwrap().unwrap();
        assert!(!first.not_found);
        assert_eq!(first.source, "embedded");
        assert_eq!(first.text, "plain words");

        lib.upsert_lyrics(track_id, "[00:00.00] synced now", "lrclib", false)
            .unwrap();
        let second = lib.lyrics_for_track(track_id).unwrap().unwrap();
        assert_eq!(second.source, "lrclib");
        assert_eq!(second.text, "[00:00.00] synced now");

        lib.upsert_lyrics(track_id, "", "lrclib", true).unwrap();
        let third = lib.lyrics_for_track(track_id).unwrap().unwrap();
        assert!(third.not_found);
        assert_eq!(third.text, "");

        let count: i64 = {
            let conn = lib.conn.lock().unwrap();
            conn.query_row(
                "SELECT COUNT(*) FROM lyrics WHERE track_id = ?1",
                [track_id],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(count, 1);
    }
}
