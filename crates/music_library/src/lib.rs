pub mod adoption;
pub mod album_artists;
pub mod error;
pub mod migrations;
pub mod models;
pub mod remote;
pub mod repository;
pub mod sqlite;
pub mod thumbnail;

pub use adoption::normalize_tag;
pub use error::{LibraryError, Result};
pub use models::{
    Album, AlbumSearchEntry, AlbumSummary, Artist, ArtistGrouping, ArtistSummary, CoverArt,
    LocalFolder, NewTrack, Playlist, PlaylistSummary, RemoteCover, RemoteSong, RemoteSource,
    RemoteSyncReport, ScanLyrics, ScanTrack, SourceSummary, StoredLyrics, Track, lyrics_source,
};
pub use repository::{LibraryRepository, ScanWrite};
pub use sqlite::{SqliteLibrary, sha256_hex};

pub const NO_METADATA_ALBUM_ID: i64 = -1;
pub const NO_METADATA_ARTIST_ID: i64 = -2;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_db() -> (SqliteLibrary, PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let temp_dir = std::env::temp_dir().join("pawse-music-library");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join(format!(
            "test-{}-{}.db",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_file(&db_path);
        (SqliteLibrary::open_at(&db_path).unwrap(), db_path)
    }

    fn fresh_db_path() -> PathBuf {
        let (lib, path) = create_test_db();
        drop(lib);
        let _ = std::fs::remove_file(&path);
        for suffix in ["-wal", "-shm", ".bak-v8"] {
            let mut sidecar = path.as_os_str().to_owned();
            sidecar.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(sidecar));
        }
        path
    }

    fn build_v8_db(path: &PathBuf, seed: &str) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        for (version, sql) in migrations::MIGRATIONS.iter().filter(|(v, _)| *v <= 8) {
            conn.execute_batch(sql).unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
        }
        conn.execute_batch(seed).unwrap();
    }

    #[test]
    fn server_track_locators_move_to_the_neutral_scheme() {
        let dir =
            std::env::temp_dir().join(format!("pawse-locator-migration-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("library.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            for (version, sql) in migrations::MIGRATIONS.iter().filter(|(v, _)| *v <= 11) {
                conn.execute_batch(sql).unwrap();
                conn.pragma_update(None, "user_version", version).unwrap();
            }
            conn.execute_batch(
                "INSERT INTO media_items (id, title, created_at, updated_at) VALUES (1, 'R', 0, 0), (2, 'L', 0, 0);
                 INSERT INTO tracks (id, path, title) VALUES
                     (1, 'subsonic://3/song-1.flac', 'R'),
                     (2, '/music/subsonic://odd.flac', 'L');",
            )
            .unwrap();
        }
        let lib = SqliteLibrary::open_at(&path).unwrap();
        let mut paths: Vec<String> = lib
            .all_tracks()
            .unwrap()
            .into_iter()
            .map(|t| t.path)
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            vec![
                "/music/subsonic://odd.flac".to_string(),
                remote::locator(3, "song-1", "flac")
            ]
        );
        drop(lib);
        let _ = std::fs::remove_dir_all(&dir);
    }

    const V8_CATALOG: &str = "
        INSERT INTO artists (id, name, sort_name) VALUES (1, 'Band', 'band');
        INSERT INTO albums (id, title, year) VALUES (1, 'Record', 2001);
        INSERT INTO album_artists (album_id, artist_id, position) VALUES (1, 1, 0);
        INSERT INTO tracks
            (id, path, title, album_id, track_number, duration_ms, start_offset_ms, liked, is_cue)
        VALUES
            (10, '/m/one.flac', 'One', 1, 1, 1000, 0, 1, 0),
            (11, '/m/cue.flac', 'Cue A', 1, 2, 2000, 0, 0, 1),
            (12, '/m/cue.flac', 'Cue B', 1, 3, 2000, 2000, 1, 1);
        INSERT INTO track_artists (track_id, artist_id, position)
            VALUES (10, 1, 0), (11, 1, 0), (12, 1, 0);
    ";

    fn user_version(path: &PathBuf) -> i64 {
        count_rows(path, "SELECT user_version FROM pragma_user_version")
    }

    #[test]
    fn migration_to_v9_keeps_every_user_row_under_the_same_id() {
        let path = fresh_db_path();
        build_v8_db(
            &path,
            &format!(
                "{V8_CATALOG}
                INSERT INTO playlists (id, name, created_at) VALUES (5, 'Liked', 0), (6, 'Mix', 0);
                INSERT INTO scan_meta (key, value) VALUES ('liked_playlist_id', '5');
                INSERT INTO playlist_tracks (playlist_id, position, track_id)
                    VALUES (5, 0, 12), (5, 1, 10), (6, 0, 11), (6, 1, 10);
                INSERT INTO lyrics (track_id, source, text, not_found, updated_at)
                    VALUES (10, 'lrclib', x'00', 0, 1), (11, 'lrc', x'00', 0, 1);
                INSERT INTO plays (id, track_id, artist, title, started_at, qualified)
                    VALUES (100, 10, 'Band', 'One', 1000, 1), (101, NULL, 'Gone', 'Old', 900, 1);
                INSERT INTO play_deliveries (play_id, target, state, updated_at)
                    VALUES (100, 'lastfm', 0, 1), (101, 'lastfm', 0, 1);
                INSERT INTO loves (id, track_id, artist, title, loved, at)
                    VALUES (200, 12, 'Band', 'Cue B', 1, 1);
                INSERT INTO love_deliveries (love_id, target, state, updated_at)
                    VALUES (200, 'lastfm', 0, 1);"
            ),
        );

        let lib = SqliteLibrary::open_at(&path).unwrap();

        assert_eq!(
            user_version(&path),
            migrations::MIGRATIONS.last().unwrap().0 as i64
        );
        let mut backup = path.as_os_str().to_owned();
        backup.push(".bak-v8");
        assert!(PathBuf::from(backup).exists());

        let mut ids: Vec<i64> = lib.all_tracks().unwrap().iter().map(|t| t.id).collect();
        ids.sort();
        assert_eq!(ids, vec![10, 11, 12]);

        let liked: Vec<(i64, bool)> = lib
            .liked_tracks()
            .unwrap()
            .iter()
            .map(|t| (t.id, t.liked))
            .collect();
        assert_eq!(liked, vec![(12, true), (10, true)]);
        assert!(!lib.track(11).unwrap().unwrap().liked);

        let mix: Vec<i64> = lib
            .tracks_for_playlist(6)
            .unwrap()
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(mix, vec![11, 10]);

        for (sql, expected) in [
            ("SELECT COUNT(*) FROM media_items", 3),
            ("SELECT COUNT(*) FROM media_bindings WHERE source_id = 1", 3),
            (
                "SELECT COUNT(*) FROM lyrics WHERE track_id = 10 AND source = 'lrclib'",
                1,
            ),
            (
                "SELECT COUNT(*) FROM lyrics WHERE track_id = 11 AND source = 'lrc'",
                1,
            ),
            (
                "SELECT COUNT(*) FROM plays WHERE id = 100 AND track_id = 10",
                1,
            ),
            (
                "SELECT COUNT(*) FROM plays WHERE id = 101 AND track_id IS NULL",
                1,
            ),
            ("SELECT COUNT(*) FROM play_deliveries", 2),
            (
                "SELECT COUNT(*) FROM loves WHERE id = 200 AND track_id = 12",
                1,
            ),
            ("SELECT COUNT(*) FROM love_deliveries", 1),
            ("SELECT COUNT(*) FROM pragma_foreign_key_check", 0),
            (
                "SELECT COUNT(*) FROM pragma_table_info('tracks') WHERE name = 'liked'",
                0,
            ),
            (
                "SELECT COUNT(*) FROM media_items WHERE id = 12 AND title = 'Cue B' AND artist = 'Band' AND album = 'Record'",
                1,
            ),
        ] {
            assert_eq!(count_rows(&path, sql), expected, "{sql}");
        }
    }

    #[test]
    fn migrated_bindings_count_as_unplaced_until_a_scan_moves_them_to_real_folders() {
        let path = fresh_db_path();
        build_v8_db(&path, V8_CATALOG);
        let lib = SqliteLibrary::open_at(&path).unwrap();
        assert!(lib.has_unplaced_media().unwrap());

        lib.reconcile_local_sources(&folders(&["/m"])).unwrap();
        assert!(lib.has_unplaced_media().unwrap());

        let mut cue_b = scan_track("/m/cue.flac", "Cue B");
        cue_b.start_offset_ms = Some(2000);
        scan(
            &lib,
            vec![
                scan_track("/m/one.flac", "One"),
                scan_track("/m/cue.flac", "Cue A"),
                cue_b,
            ],
        );
        assert!(!lib.has_unplaced_media().unwrap());
        assert_eq!(lib.sources().unwrap()[0].track_count, 3);
    }

    #[test]
    fn migration_to_v9_seeds_likes_from_the_column_when_no_liked_playlist_exists() {
        let path = fresh_db_path();
        build_v8_db(&path, V8_CATALOG);

        let lib = SqliteLibrary::open_at(&path).unwrap();

        let liked: Vec<i64> = lib.liked_tracks().unwrap().iter().map(|t| t.id).collect();
        assert_eq!(liked, vec![10, 12]);
    }

    #[test]
    fn a_fresh_database_gets_no_migration_backup() {
        let path = fresh_db_path();
        let lib = SqliteLibrary::open_at(&path).unwrap();
        drop(lib);
        let mut backup = path.as_os_str().to_owned();
        backup.push(".bak-v8");
        assert!(!PathBuf::from(backup).exists());
        assert_eq!(
            user_version(&path),
            migrations::MIGRATIONS.last().unwrap().0 as i64
        );
    }

    #[test]
    fn test_migrations_create_schema() {
        let (lib, _path) = create_test_db();
        assert!(!lib.has_tracks().unwrap());
    }

    #[test]
    fn test_upsert_artist() {
        let (lib, _path) = create_test_db();
        let id1 = lib.upsert_artist("The Beatles").unwrap();
        let id2 = lib.upsert_artist("The Beatles").unwrap();
        assert_eq!(id1, id2, "upsert should return same id for same artist");

        let id3 = lib.upsert_artist("Radiohead").unwrap();
        assert_ne!(id1, id3, "different artists should have different ids");
    }

    #[test]
    fn test_upsert_album() {
        let (lib, _path) = create_test_db();
        let album_id = lib.upsert_album("Abbey Road", Some(1969), None).unwrap();
        let album_id2 = lib.upsert_album("Abbey Road", Some(1969), None).unwrap();
        assert_eq!(
            album_id, album_id2,
            "upsert should return same id for same album"
        );
    }

    #[test]
    fn test_album_artists() {
        let (lib, _path) = create_test_db();
        let artist1 = lib.upsert_artist("The Beatles").unwrap();
        let artist2 = lib.upsert_artist("Billy Preston").unwrap();
        let album_id = lib.upsert_album("Let It Be", Some(1970), None).unwrap();
        lib.set_album_artists(album_id, &[(artist1, 0), (artist2, 1)])
            .unwrap();

        let albums = lib.albums().unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].artist_name, "The Beatles");
    }

    #[test]
    fn test_upsert_track_and_query() {
        let (lib, _path) = create_test_db();
        let artist_id = lib.upsert_artist("Radiohead").unwrap();
        let album_id = lib.upsert_album("OK Computer", Some(1997), None).unwrap();
        lib.set_album_artists(album_id, &[(artist_id, 0)]).unwrap();

        let track = NewTrack {
            path: "/music/01 Airbag.flac".into(),
            title: Some("Airbag".into()),
            album_title: Some("OK Computer".into()),
            artist_names: vec!["Radiohead".into()],
            track_number: Some(1),
            disc_number: Some(1),
            year: Some(1997),
            duration_ms: Some(285_000),
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        let _track_id = lib
            .upsert_track(&track, Some(album_id), &[(artist_id, 0)])
            .unwrap();

        let tracks = lib.tracks_for_album(album_id).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "Airbag");
        assert_eq!(tracks[0].track_number, Some(1));
    }

    #[test]
    fn test_albums_sorted_by_artist_sort_name() {
        let (lib, _path) = create_test_db();
        let beatles = lib.upsert_artist("The Beatles").unwrap();
        let zeppelin = lib.upsert_artist("Led Zeppelin").unwrap();

        let album1 = lib.upsert_album("Abbey Road", Some(1969), None).unwrap();
        let album2 = lib.upsert_album("IV", Some(1971), None).unwrap();

        lib.set_album_artists(album1, &[(beatles, 0)]).unwrap();
        lib.set_album_artists(album2, &[(zeppelin, 0)]).unwrap();

        let albums = lib.albums().unwrap();
        assert_eq!(albums.len(), 2);
        assert_eq!(albums[0].title, "IV");
        assert_eq!(albums[1].title, "Abbey Road");
    }

    #[test]
    fn test_tracks_by_keys_matches_path_and_offset() {
        let (lib, _path) = create_test_db();
        let artist_id = lib.upsert_artist("Artist").unwrap();
        let album_id = lib.upsert_album("Album", None, None).unwrap();
        lib.set_album_artists(album_id, &[(artist_id, 0)]).unwrap();

        let new_track = |path: &str, title: &str, offset: Option<u64>| {
            let t = NewTrack {
                path: path.into(),
                title: Some(title.into()),
                album_title: Some("Album".into()),
                artist_names: vec!["Artist".into()],
                track_number: None,
                disc_number: None,
                year: None,
                duration_ms: None,
                cover_art_id: None,
                start_offset_ms: offset,
                bitrate: None,
            };
            lib.upsert_track(&t, Some(album_id), &[(artist_id, 0)])
                .unwrap()
        };

        new_track("/music/a.flac", "A", None);
        new_track("/album.flac", "Cue 1", Some(0));
        new_track("/album.flac", "Cue 2", Some(5000));

        let found = lib
            .tracks_by_keys(&[
                ("/music/a.flac".into(), 0),
                ("/album.flac".into(), 5000),
                ("/missing.flac".into(), 0),
            ])
            .unwrap();
        let mut rows: Vec<(String, i32)> = found
            .iter()
            .map(|t| (t.title.clone(), t.start_offset_ms))
            .collect();
        rows.sort();
        assert_eq!(
            rows,
            vec![
                ("A".to_string(), 0),
                ("Cue 1".to_string(), 0),
                ("Cue 2".to_string(), 5000),
            ]
        );

        assert!(lib.tracks_by_keys(&[]).unwrap().is_empty());
    }

    #[test]
    fn test_all_tracks_ordered_and_counted() {
        let (lib, _path) = create_test_db();
        assert_eq!(lib.track_count().unwrap(), 0);
        assert!(lib.all_tracks().unwrap().is_empty());

        let beatles = lib.upsert_artist("The Beatles").unwrap();
        let radiohead = lib.upsert_artist("Radiohead").unwrap();
        let abbey = lib.upsert_album("Abbey Road", Some(1969), None).unwrap();
        let ok = lib.upsert_album("OK Computer", Some(1997), None).unwrap();
        lib.set_album_artists(abbey, &[(beatles, 0)]).unwrap();
        lib.set_album_artists(ok, &[(radiohead, 0)]).unwrap();

        let add = |path: &str, title: &str, album: &str, artist: &str, album_id, artist_id, no| {
            let t = NewTrack {
                path: path.into(),
                title: Some(title.into()),
                album_title: Some(album.into()),
                artist_names: vec![artist.into()],
                track_number: Some(no),
                disc_number: Some(1),
                year: None,
                duration_ms: None,
                cover_art_id: None,
                start_offset_ms: None,
                bitrate: None,
            };
            lib.upsert_track(&t, Some(album_id), &[(artist_id, 0)])
                .unwrap();
        };

        add(
            "/r/2.flac",
            "Paranoid Android",
            "OK Computer",
            "Radiohead",
            ok,
            radiohead,
            2,
        );
        add(
            "/r/1.flac",
            "Airbag",
            "OK Computer",
            "Radiohead",
            ok,
            radiohead,
            1,
        );
        add(
            "/b/1.flac",
            "Come Together",
            "Abbey Road",
            "The Beatles",
            abbey,
            beatles,
            1,
        );

        assert_eq!(lib.track_count().unwrap(), 3);

        let titles: Vec<String> = lib
            .all_tracks()
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(titles, vec!["Come Together", "Airbag", "Paranoid Android"]);
    }

    #[test]
    fn test_no_metadata_bucket() {
        let (lib, _path) = create_test_db();
        let beatles = lib.upsert_artist("The Beatles").unwrap();
        let abbey = lib.upsert_album("Abbey Road", Some(1969), None).unwrap();
        lib.set_album_artists(abbey, &[(beatles, 0)]).unwrap();

        let tagged = NewTrack {
            path: "/b/1.flac".into(),
            title: Some("Come Together".into()),
            album_title: Some("Abbey Road".into()),
            artist_names: vec!["The Beatles".into()],
            track_number: Some(1),
            disc_number: Some(1),
            year: None,
            duration_ms: None,
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        lib.upsert_track(&tagged, Some(abbey), &[(beatles, 0)])
            .unwrap();

        assert!(
            !lib.albums()
                .unwrap()
                .iter()
                .any(|a| a.id == NO_METADATA_ALBUM_ID)
        );
        assert!(
            !lib.artists(ArtistGrouping::TrackArtist)
                .unwrap()
                .iter()
                .any(|a| a.id == NO_METADATA_ARTIST_ID)
        );

        let bare = NewTrack {
            path: "/loose/track.flac".into(),
            title: Some("track".into()),
            album_title: None,
            artist_names: Vec::new(),
            track_number: None,
            disc_number: None,
            year: None,
            duration_ms: None,
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        lib.upsert_track(&bare, None, &[]).unwrap();

        let albums = lib.albums().unwrap();
        assert_eq!(albums.last().map(|a| a.id), Some(NO_METADATA_ALBUM_ID));
        let album_tracks = lib.tracks_for_album(NO_METADATA_ALBUM_ID).unwrap();
        assert_eq!(album_tracks.len(), 1);
        assert_eq!(album_tracks[0].path, "/loose/track.flac");

        let artists = lib.artists(ArtistGrouping::TrackArtist).unwrap();
        let no_meta_artist = artists.iter().find(|a| a.id == NO_METADATA_ARTIST_ID);
        assert_eq!(no_meta_artist.map(|a| a.track_count), Some(1));
        let artist_tracks = lib
            .tracks_by_artist(NO_METADATA_ARTIST_ID, ArtistGrouping::TrackArtist)
            .unwrap();
        assert_eq!(artist_tracks.len(), 1);
        assert_eq!(artist_tracks[0].path, "/loose/track.flac");
    }

    #[test]
    fn test_tracks_by_keys_chunks_beyond_parameter_limit() {
        let (lib, _path) = create_test_db();
        let artist_id = lib.upsert_artist("Artist").unwrap();
        let album_id = lib.upsert_album("Album", None, None).unwrap();
        lib.set_album_artists(album_id, &[(artist_id, 0)]).unwrap();

        let mut keys: Vec<(String, i32)> = Vec::new();
        for i in 0..1100 {
            let path = format!("/music/{i}.flac");
            let track = NewTrack {
                path: path.clone(),
                title: Some(format!("T{i}")),
                album_title: Some("Album".into()),
                artist_names: vec!["Artist".into()],
                track_number: None,
                disc_number: None,
                year: None,
                duration_ms: None,
                cover_art_id: None,
                start_offset_ms: None,
                bitrate: None,
            };
            lib.upsert_track(&track, Some(album_id), &[(artist_id, 0)])
                .unwrap();
            keys.push((path, 0));
        }

        let found = lib.tracks_by_keys(&keys).unwrap();
        assert_eq!(found.len(), 1100);
    }

    #[test]
    fn test_clear() {
        let (lib, _path) = create_test_db();
        let artist_id = lib.upsert_artist("Artist").unwrap();
        let album_id = lib.upsert_album("Album", None, None).unwrap();
        lib.set_album_artists(album_id, &[(artist_id, 0)]).unwrap();

        let track = NewTrack {
            path: "/music/song.flac".into(),
            title: Some("Song".into()),
            album_title: Some("Album".into()),
            artist_names: vec!["Artist".into()],
            track_number: None,
            disc_number: None,
            year: None,
            duration_ms: None,
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        lib.upsert_track(&track, Some(album_id), &[(artist_id, 0)])
            .unwrap();

        assert!(lib.has_tracks().unwrap());
        lib.clear().unwrap();
        assert!(!lib.has_tracks().unwrap());
    }

    #[test]
    fn test_track_title_fallback_from_path() {
        let (lib, _path) = create_test_db();
        let artist_id = lib.upsert_artist("Artist").unwrap();
        let album_id = lib.upsert_album("Album", None, None).unwrap();
        lib.set_album_artists(album_id, &[(artist_id, 0)]).unwrap();

        let track = NewTrack {
            path: "/music/Unknown Title.flac".into(),
            title: None,
            album_title: Some("Album".into()),
            artist_names: vec!["Artist".into()],
            track_number: None,
            disc_number: None,
            year: None,
            duration_ms: None,
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        let _track_id = lib
            .upsert_track(&track, Some(album_id), &[(artist_id, 0)])
            .unwrap();
        let tracks = lib.tracks_for_album(album_id).unwrap();
        assert_eq!(tracks[0].title, "Unknown Title");
    }

    #[test]
    fn test_multidisc_tracks_ordered_by_disc() {
        let (lib, _path) = create_test_db();
        let album_artist_id = lib.upsert_artist("Album Artist").unwrap();
        let track1_artist_id = lib.upsert_artist("Artist One").unwrap();
        let track2_artist_id = lib.upsert_artist("Artist Two").unwrap();
        let album_id = lib
            .upsert_album("Multi-Disc Album", Some(2020), None)
            .unwrap();

        let track1 = NewTrack {
            path: "/music/disc1/track01.flac".into(),
            title: Some("Track One".into()),
            album_title: Some("Multi-Disc Album".into()),
            artist_names: vec!["Artist One".into()],
            track_number: Some(1),
            disc_number: Some(1),
            year: Some(2020),
            duration_ms: Some(180_000),
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        let track2 = NewTrack {
            path: "/music/disc2/track01.flac".into(),
            title: Some("Track Two".into()),
            album_title: Some("Multi-Disc Album".into()),
            artist_names: vec!["Artist Two".into()],
            track_number: Some(1),
            disc_number: Some(2),
            year: Some(2020),
            duration_ms: Some(200_000),
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };

        lib.upsert_track(&track1, Some(album_id), &[(track1_artist_id, 0)])
            .unwrap();
        lib.upsert_track(&track2, Some(album_id), &[(track2_artist_id, 0)])
            .unwrap();
        lib.set_album_artists(album_id, &[(album_artist_id, 0)])
            .unwrap();

        let tracks = lib.tracks_for_album(album_id).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].disc_number, 1);
        assert_eq!(tracks[0].title, "Track One");
        assert_eq!(tracks[1].disc_number, 2);
        assert_eq!(tracks[1].title, "Track Two");

        let albums = lib.albums().unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].artist_name, "Album Artist");
    }

    #[test]
    fn test_track_artists() {
        let (lib, _path) = create_test_db();
        let artist1 = lib.upsert_artist("Artist One").unwrap();
        let artist2 = lib.upsert_artist("Artist Two").unwrap();
        let album_id = lib.upsert_album("Album", None, None).unwrap();

        let track = NewTrack {
            path: "/music/track.flac".into(),
            title: Some("Track".into()),
            album_title: Some("Album".into()),
            artist_names: vec!["Artist One".into(), "Artist Two".into()],
            track_number: None,
            disc_number: None,
            year: None,
            duration_ms: None,
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        let track_id = lib
            .upsert_track(&track, Some(album_id), &[(artist1, 0), (artist2, 1)])
            .unwrap();
        let artists = lib.track_artists(track_id).unwrap();
        assert_eq!(artists, vec!["Artist One", "Artist Two"]);
    }

    #[test]
    fn test_album_title_found() {
        let (lib, _path) = create_test_db();
        let album_id = lib.upsert_album("Test Album", Some(2000), None).unwrap();
        assert_eq!(
            lib.album_title(album_id).unwrap(),
            Some("Test Album".into())
        );
    }

    #[test]
    fn test_album_title_not_found() {
        let (lib, _path) = create_test_db();
        assert!(lib.album_title(999).unwrap().is_none());
    }

    #[test]
    fn test_vacuum_preserves_data() {
        let (lib, _path) = create_test_db();
        let track_id = seed_track(&lib, "Song", "Album", "Artist");

        lib.vacuum().unwrap();

        let albums = lib.albums().unwrap();
        assert_eq!(albums.len(), 1);
        let tracks = lib.tracks_for_album(albums[0].id).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, track_id);
        assert_eq!(tracks[0].title, "Song");
    }

    #[test]
    fn test_delete_orphaned_albums_and_artists() {
        let (lib, _path) = create_test_db();
        let artist_id = lib.upsert_artist("Orphan Artist").unwrap();
        let album_id = lib.upsert_album("Orphan Album", None, None).unwrap();
        lib.set_album_artists(album_id, &[(artist_id, 0)]).unwrap();

        lib.delete_orphaned_albums_and_artists().unwrap();

        assert!(lib.album_title(album_id).unwrap().is_none());
        assert!(lib.album_artists(album_id).unwrap().is_empty());
    }

    #[test]
    fn test_save_and_retrieve_cover_art() {
        let (lib, _path) = create_test_db();
        let img = image::RgbImage::from_pixel(4, 4, image::Rgb([255, 0, 0]));
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Jpeg)
            .unwrap();
        let data = buf.into_inner();

        let id = lib.save_cover_art(&data, "/music/a.flac", true).unwrap();
        assert!(id > 0);

        let id2 = lib.save_cover_art(&data, "/music/a.flac", true).unwrap();
        assert_eq!(id, id2);

        let cover = lib.get_cover_art(id).unwrap().unwrap();
        assert!(!cover.small.is_empty());
        assert!(!cover.large.is_empty());
    }

    #[test]
    fn test_clear_then_reinsert() {
        let (lib, _path) = create_test_db();
        let artist_id = lib.upsert_artist("Artist").unwrap();
        let album_id = lib.upsert_album("Album", None, None).unwrap();
        lib.set_album_artists(album_id, &[(artist_id, 0)]).unwrap();
        let track = NewTrack {
            path: "/music/song.flac".into(),
            title: Some("Song".into()),
            album_title: Some("Album".into()),
            artist_names: vec!["Artist".into()],
            track_number: None,
            disc_number: None,
            year: None,
            duration_ms: None,
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        lib.upsert_track(&track, Some(album_id), &[(artist_id, 0)])
            .unwrap();
        assert!(lib.has_tracks().unwrap());

        lib.clear().unwrap();
        assert!(!lib.has_tracks().unwrap());

        let artist_id2 = lib.upsert_artist("Artist").unwrap();
        let album_id2 = lib.upsert_album("Album", None, None).unwrap();
        lib.set_album_artists(album_id2, &[(artist_id2, 0)])
            .unwrap();
        lib.upsert_track(&track, Some(album_id2), &[(artist_id2, 0)])
            .unwrap();
        assert!(lib.has_tracks().unwrap());
        let tracks = lib.tracks_for_album(album_id2).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "Song");
    }

    #[test]
    fn test_empty_album_artist_in_summary() {
        let (lib, _path) = create_test_db();
        let album_id = lib.upsert_album("Compilation", Some(2020), None).unwrap();
        let artist_id = lib.upsert_artist("Various").unwrap();
        let track = NewTrack {
            path: "/music/track01.flac".into(),
            title: Some("Track".into()),
            album_title: Some("Compilation".into()),
            artist_names: vec!["Various".into()],
            track_number: Some(1),
            disc_number: None,
            year: Some(2020),
            duration_ms: Some(180_000),
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        lib.upsert_track(&track, Some(album_id), &[(artist_id, 0)])
            .unwrap();

        let albums = lib.albums().unwrap();
        let album = albums.iter().find(|a| a.id == album_id).unwrap();
        assert_eq!(album.artist_name, "");
    }

    #[test]
    fn test_track_without_album() {
        let (lib, _path) = create_test_db();
        let artist_id = lib.upsert_artist("Artist").unwrap();
        let track = NewTrack {
            path: "/music/song.flac".into(),
            title: Some("Song".into()),
            album_title: None,
            artist_names: vec!["Artist".into()],
            track_number: None,
            disc_number: None,
            year: None,
            duration_ms: None,
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        let track_id = lib.upsert_track(&track, None, &[(artist_id, 0)]).unwrap();

        assert!(lib.has_tracks().unwrap());
        let artists = lib.track_artists(track_id).unwrap();
        assert_eq!(artists, vec!["Artist"]);
    }

    #[test]
    fn test_same_path_different_offset() {
        let (lib, _path) = create_test_db();
        let artist_id = lib.upsert_artist("Artist").unwrap();
        let album_id = lib.upsert_album("Album", None, None).unwrap();
        lib.set_album_artists(album_id, &[(artist_id, 0)]).unwrap();

        let track1 = NewTrack {
            path: "/music/track.flac".into(),
            title: Some("Track One".into()),
            album_title: Some("Album".into()),
            artist_names: vec!["Artist".into()],
            track_number: Some(1),
            disc_number: None,
            year: None,
            duration_ms: Some(300_000),
            cover_art_id: None,
            start_offset_ms: Some(0),
            bitrate: None,
        };
        let track2 = NewTrack {
            path: "/music/track.flac".into(),
            title: Some("Track Two".into()),
            album_title: Some("Album".into()),
            artist_names: vec!["Artist".into()],
            track_number: Some(2),
            disc_number: None,
            year: None,
            duration_ms: Some(300_000),
            cover_art_id: None,
            start_offset_ms: Some(300_000),
            bitrate: None,
        };

        let id1 = lib
            .upsert_track(&track1, Some(album_id), &[(artist_id, 0)])
            .unwrap();
        let id2 = lib
            .upsert_track(&track2, Some(album_id), &[(artist_id, 0)])
            .unwrap();

        assert_ne!(
            id1, id2,
            "same path with different offsets should create distinct tracks"
        );

        let tracks = lib.tracks_for_album(album_id).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].title, "Track One");
        assert_eq!(tracks[1].title, "Track Two");
    }

    #[test]
    fn test_has_tracks_empty_on_fresh_db() {
        let (lib, _path) = create_test_db();
        assert!(!lib.has_tracks().unwrap());
    }

    fn make_test_jpeg(bytes: &[u8]) -> Vec<u8> {
        let color = if bytes.is_empty() {
            image::Rgb([0, 0, 0])
        } else {
            image::Rgb([
                bytes[0],
                bytes.get(1).copied().unwrap_or(0),
                bytes.get(2).copied().unwrap_or(0),
            ])
        };
        let img = image::RgbImage::from_pixel(4, 4, color);
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut buf, image::ImageFormat::Jpeg)
            .unwrap();
        buf.into_inner()
    }

    #[test]
    fn test_cover_art_different_images_different_ids() {
        let (lib, _path) = create_test_db();
        let data1 = make_test_jpeg(&[255, 0, 0]);
        let data2 = make_test_jpeg(&[0, 255, 0]);

        let id1 = lib.save_cover_art(&data1, "/music/a.flac", true).unwrap();
        let id2 = lib.save_cover_art(&data2, "/music/a.flac", true).unwrap();
        assert_ne!(id1, id2, "different images must have different IDs");
    }

    #[test]
    fn test_cover_art_get_nonexistent() {
        let (lib, _path) = create_test_db();
        assert!(lib.get_cover_art(999).unwrap().is_none());
        assert!(lib.get_cover_art_small(999).unwrap().is_none());
        assert!(lib.get_cover_art_large(999).unwrap().is_none());
    }

    #[test]
    fn test_cover_art_thumbnail_sizes() {
        let (lib, _path) = create_test_db();
        let data = make_test_jpeg(&[255, 0, 0]);
        let id = lib.save_cover_art(&data, "/music/a.flac", true).unwrap();
        let cover = lib.get_cover_art(id).unwrap().unwrap();

        let small_img = image::load_from_memory(&cover.small).unwrap();
        let large_img = image::load_from_memory(&cover.large).unwrap();
        assert!(small_img.width() <= 128);
        assert!(small_img.height() <= 128);
        assert!(large_img.width() <= 320);
        assert!(large_img.height() <= 320);
    }

    #[test]
    fn test_cover_art_id_propagates_to_album_and_track() {
        let (lib, _path) = create_test_db();
        let data = make_test_jpeg(&[255, 0, 0]);
        let cover_id = lib.save_cover_art(&data, "/music/a.flac", true).unwrap();

        let artist_id = lib.upsert_artist("Artist").unwrap();
        let album_id = lib
            .upsert_album("Album", Some(2020), Some(cover_id))
            .unwrap();
        lib.set_album_artists(album_id, &[(artist_id, 0)]).unwrap();

        let track = NewTrack {
            path: "/music/song.flac".into(),
            title: Some("Song".into()),
            album_title: Some("Album".into()),
            artist_names: vec!["Artist".into()],
            track_number: Some(1),
            disc_number: Some(1),
            year: Some(2020),
            duration_ms: Some(200_000),
            cover_art_id: Some(cover_id),
            start_offset_ms: None,
            bitrate: None,
        };
        lib.upsert_track(&track, Some(album_id), &[(artist_id, 0)])
            .unwrap();

        let albums = lib.albums().unwrap();
        assert_eq!(albums[0].cover_art_id, Some(cover_id));

        let tracks = lib.tracks_for_album(album_id).unwrap();
        assert_eq!(tracks[0].cover_art_id, Some(cover_id));

        let retrieved = lib.get_cover_art(cover_id).unwrap().unwrap();
        assert_eq!(retrieved.id, cover_id);
        assert!(!retrieved.small.is_empty());
        assert!(!retrieved.large.is_empty());
    }

    /// The album takes the cover of its *lowest-numbered* track, not of whichever one
    /// was written first — the tracks here go in back to front to prove it, since the
    /// parallel scan gives no order guarantee at all.
    #[test]
    fn test_resolve_album_covers_picks_the_lowest_numbered_track() {
        let (lib, _path) = create_test_db();
        let artist = lib.upsert_artist("Artist").unwrap();
        let album = lib.upsert_album("Album", Some(2020), None).unwrap();

        let mut covers = Vec::new();
        for (n, shade) in [(3u32, 60u8), (2, 30), (1, 0)] {
            let cover = lib
                .save_cover_art(&make_test_jpeg(&[shade, 0, 0]), "/music/a.flac", true)
                .unwrap();
            covers.push((n, cover));
            let track = NewTrack {
                path: format!("/music/{n}.flac"),
                title: Some(format!("Track {n}")),
                album_title: Some("Album".into()),
                artist_names: vec!["Artist".into()],
                track_number: Some(n),
                disc_number: Some(1),
                year: Some(2020),
                duration_ms: Some(1000),
                cover_art_id: Some(cover),
                start_offset_ms: None,
                bitrate: None,
            };
            lib.upsert_track(&track, Some(album), &[(artist, 0)])
                .unwrap();
        }

        lib.resolve_album_covers().unwrap();
        let first_track_cover = covers.iter().find(|(n, _)| *n == 1).unwrap().1;
        assert_eq!(
            lib.albums().unwrap()[0].cover_art_id,
            Some(first_track_cover),
            "insert order was 3, 2, 1 — the result must not depend on it"
        );
    }

    #[test]
    fn test_resolve_album_covers_clears_an_album_whose_tracks_lost_their_art() {
        let (lib, _path) = create_test_db();
        let cover = lib
            .save_cover_art(&make_test_jpeg(&[255, 0, 0]), "/music/a.flac", true)
            .unwrap();
        let artist = lib.upsert_artist("Artist").unwrap();
        let album = lib.upsert_album("Album", None, Some(cover)).unwrap();
        let track = NewTrack {
            path: "/music/one.flac".into(),
            title: Some("One".into()),
            album_title: Some("Album".into()),
            artist_names: vec!["Artist".into()],
            track_number: Some(1),
            disc_number: Some(1),
            year: None,
            duration_ms: Some(1000),
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        lib.upsert_track(&track, Some(album), &[(artist, 0)])
            .unwrap();

        lib.resolve_album_covers().unwrap();
        assert_eq!(
            lib.albums().unwrap()[0].cover_art_id,
            None,
            "no track carries art, so the album must not keep a stale cover"
        );
    }

    #[test]
    fn test_cover_art_deduplication_across_albums() {
        let (lib, _path) = create_test_db();
        let data = make_test_jpeg(&[255, 0, 0]);

        let cover_id = lib.save_cover_art(&data, "/music/a.flac", true).unwrap();
        let cover_id2 = lib.save_cover_art(&data, "/music/a.flac", true).unwrap();
        assert_eq!(cover_id, cover_id2, "same bytes must return same ID");

        let artist_id = lib.upsert_artist("Artist").unwrap();
        let album1 = lib.upsert_album("Album 1", None, Some(cover_id)).unwrap();
        let album2 = lib.upsert_album("Album 2", None, Some(cover_id)).unwrap();

        let track1 = NewTrack {
            path: "/music/track1.flac".into(),
            title: Some("Track 1".into()),
            album_title: Some("Album 1".into()),
            artist_names: vec!["Artist".into()],
            track_number: Some(1),
            disc_number: Some(1),
            year: None,
            duration_ms: None,
            cover_art_id: Some(cover_id),
            start_offset_ms: None,
            bitrate: None,
        };
        let track2 = NewTrack {
            path: "/music/track2.flac".into(),
            title: Some("Track 2".into()),
            album_title: Some("Album 2".into()),
            artist_names: vec!["Artist".into()],
            track_number: Some(1),
            disc_number: Some(1),
            year: None,
            duration_ms: None,
            cover_art_id: Some(cover_id),
            start_offset_ms: None,
            bitrate: None,
        };

        lib.upsert_track(&track1, Some(album1), &[(artist_id, 0)])
            .unwrap();
        lib.upsert_track(&track2, Some(album2), &[(artist_id, 0)])
            .unwrap();

        let albums = lib.albums().unwrap();
        assert_eq!(albums.len(), 2);
        for album in &albums {
            assert_eq!(album.cover_art_id, Some(cover_id));
        }
    }

    #[test]
    fn test_cover_art_clear_removes_cover_art() {
        let (lib, _path) = create_test_db();
        let data = make_test_jpeg(&[255, 0, 0]);
        let cover_id = lib.save_cover_art(&data, "/music/a.flac", true).unwrap();

        assert!(lib.get_cover_art(cover_id).unwrap().is_some());

        // clear() keeps cover_art rows intact (hash→id mapping stays valid for
        // PlaybackQueue across rescans). Orphans are removed in the post-rescan
        // cleanup step.
        lib.clear().unwrap();
        lib.delete_orphaned_albums_and_artists().unwrap();

        assert!(lib.get_cover_art(cover_id).unwrap().is_none());
    }

    #[test]
    fn test_cover_art_full_scanner_flow() {
        let (lib, _path) = create_test_db();
        let cover_data = make_test_jpeg(&[100, 150, 200]);

        // Step 1: Scanner extracted raw cover bytes
        let cover_art_id = lib
            .save_cover_art(&cover_data, "/music/a.flac", true)
            .unwrap();

        // Step 2: Upsert artist
        let artist_id = lib.upsert_artist("Test Artist").unwrap();

        // Step 3: Upsert album with cover_art_id
        let album_id = lib
            .upsert_album("Test Album", Some(2024), Some(cover_art_id))
            .unwrap();
        lib.set_album_artists(album_id, &[(artist_id, 0)]).unwrap();

        // Step 4: Build NewTrack with cover_art_id (no raw bytes)
        let new_track = NewTrack {
            path: "/music/test.flac".into(),
            title: Some("Test Track".into()),
            album_title: Some("Test Album".into()),
            artist_names: vec!["Test Artist".into()],
            track_number: Some(1),
            disc_number: Some(1),
            year: Some(2024),
            duration_ms: Some(180_000),
            cover_art_id: Some(cover_art_id),
            start_offset_ms: None,
            bitrate: None,
        };
        lib.upsert_track(&new_track, Some(album_id), &[(artist_id, 0)])
            .unwrap();

        // Step 5: Verify album summary has cover_art_id
        let albums = lib.albums().unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].title, "Test Album");
        assert_eq!(albums[0].cover_art_id, Some(cover_art_id));
        assert_eq!(albums[0].artist_name, "Test Artist");

        // Step 6: Verify track has cover_art_id
        let tracks = lib.tracks_for_album(album_id).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].cover_art_id, Some(cover_art_id));

        // Step 7: Verify thumbnails exist and are non-empty
        let cover = lib.get_cover_art(cover_art_id).unwrap().unwrap();
        assert!(!cover.small.is_empty());
        assert!(!cover.large.is_empty());
        assert_eq!(cover.id, cover_art_id);

        // Step 8: Verify small and large can be retrieved independently
        let small = lib.get_cover_art_small(cover_art_id).unwrap().unwrap();
        let large = lib.get_cover_art_large(cover_art_id).unwrap().unwrap();
        assert_eq!(small, cover.small);
        assert_eq!(large, cover.large);
    }

    fn seed_track(lib: &SqliteLibrary, title: &str, album: &str, artist: &str) -> i64 {
        let artist_id = lib.upsert_artist(artist).unwrap();
        let album_id = lib.upsert_album(album, Some(2020), None).unwrap();
        lib.set_album_artists(album_id, &[(artist_id, 0)]).unwrap();
        let track = NewTrack {
            path: format!("/music/{}.flac", title),
            title: Some(title.into()),
            album_title: Some(album.into()),
            artist_names: vec![artist.into()],
            track_number: Some(1),
            disc_number: Some(1),
            year: Some(2020),
            duration_ms: Some(180_000),
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        lib.upsert_track(&track, Some(album_id), &[(artist_id, 0)])
            .unwrap()
    }

    #[test]
    fn test_track_defaults_to_not_liked() {
        let (lib, _path) = create_test_db();
        let _ = seed_track(&lib, "Song", "Album", "Artist");
        let album_id = lib.upsert_album("Album", Some(2020), None).unwrap();
        let tracks = lib.tracks_for_album(album_id).unwrap();
        assert!(!tracks[0].liked);
    }

    #[test]
    fn test_set_liked_toggles_flag() {
        let (lib, _path) = create_test_db();
        let track_id = seed_track(&lib, "Song", "Album", "Artist");

        lib.set_liked(track_id, true).unwrap();
        let liked = lib.liked_tracks().unwrap();
        assert_eq!(liked.len(), 1);
        assert!(liked[0].liked);
        assert_eq!(liked[0].title, "Song");

        lib.set_liked(track_id, false).unwrap();
        assert!(lib.liked_tracks().unwrap().is_empty());
    }

    #[test]
    fn test_liked_reorder_persists_and_hidden_playlist_stays_invisible() {
        let (lib, _path) = create_test_db();
        let a = seed_track(&lib, "A", "Album", "Artist");
        let b = seed_track(&lib, "B", "Album", "Artist");
        let c = seed_track(&lib, "C", "Album", "Artist");
        lib.set_liked(a, true).unwrap();
        lib.set_liked(b, true).unwrap();
        lib.set_liked(c, true).unwrap();

        let liked_ids = |lib: &SqliteLibrary| -> Vec<i64> {
            lib.liked_tracks().unwrap().iter().map(|t| t.id).collect()
        };
        assert_eq!(liked_ids(&lib), vec![a, b, c]);

        lib.move_liked_track(0, 2).unwrap();
        assert_eq!(liked_ids(&lib), vec![b, c, a]);
        assert!(lib.liked_tracks().unwrap().iter().all(|t| t.liked));

        let pl = lib.create_playlist("Mine").unwrap();
        lib.add_track_to_playlist(pl, b).unwrap();
        assert_eq!(lib.playlists().unwrap().len(), 1);
        assert_eq!(lib.playlists_containing_track(b).unwrap(), vec![pl]);

        lib.set_liked(c, false).unwrap();
        assert_eq!(liked_ids(&lib), vec![b, a]);
    }

    #[test]
    fn test_like_many_appends_in_order_and_skips_duplicates() {
        let (lib, _path) = create_test_db();
        let a = seed_track(&lib, "A", "Album", "Artist");
        let b = seed_track(&lib, "B", "Album", "Artist");
        let c = seed_track(&lib, "C", "Album", "Artist");
        lib.set_liked(a, true).unwrap();

        lib.like_many(&[b, c, a]).unwrap();

        let liked: Vec<i64> = lib.liked_tracks().unwrap().iter().map(|t| t.id).collect();
        assert_eq!(liked, vec![a, b, c]);
        assert!(lib.liked_tracks().unwrap().iter().all(|t| t.liked));

        lib.set_liked(b, false).unwrap();
        let liked: Vec<i64> = lib.liked_tracks().unwrap().iter().map(|t| t.id).collect();
        assert_eq!(liked, vec![a, c]);
    }

    #[test]
    fn test_like_many_on_an_empty_slice_is_a_no_op() {
        let (lib, _path) = create_test_db();
        let a = seed_track(&lib, "A", "Album", "Artist");
        lib.like_many(&[]).unwrap();
        assert!(lib.liked_tracks().unwrap().is_empty());
        assert!(!lib.track(a).unwrap().unwrap().liked);
    }

    #[test]
    fn test_artists_enumerates_with_track_counts() {
        let (lib, _path) = create_test_db();
        seed_track(&lib, "Song A", "Album X", "Artist Alpha");
        seed_track(&lib, "Song B", "Album X", "Artist Alpha");
        seed_track(&lib, "Song C", "Album Y", "Artist Beta");

        let artists = lib.artists(ArtistGrouping::TrackArtist).unwrap();
        let alpha = artists.iter().find(|a| a.name == "Artist Alpha").unwrap();
        let beta = artists.iter().find(|a| a.name == "Artist Beta").unwrap();
        assert_eq!(alpha.track_count, 2);
        assert_eq!(beta.track_count, 1);
        // Artists with zero tracks should not appear.
        lib.upsert_artist("Lonely Artist").unwrap();
        let artists2 = lib.artists(ArtistGrouping::TrackArtist).unwrap();
        assert!(artists2.iter().all(|a| a.name != "Lonely Artist"));
    }

    #[test]
    fn test_tracks_by_artist_returns_all_albums_for_artist() {
        let (lib, _path) = create_test_db();
        let radiohead = lib.upsert_artist("Radiohead").unwrap();
        let other = lib.upsert_artist("Other").unwrap();
        let ok_computer = lib.upsert_album("OK Computer", Some(1997), None).unwrap();
        let kid_a = lib.upsert_album("Kid A", Some(2000), None).unwrap();
        let other_album = lib.upsert_album("Other Album", Some(2001), None).unwrap();
        lib.set_album_artists(ok_computer, &[(radiohead, 0)])
            .unwrap();
        lib.set_album_artists(kid_a, &[(radiohead, 0)]).unwrap();
        lib.set_album_artists(other_album, &[(other, 0)]).unwrap();

        let mk_track = |path: &str, title: &str, _album_id: i64| NewTrack {
            path: path.into(),
            title: Some(title.into()),
            album_title: None,
            artist_names: vec![],
            track_number: Some(1),
            disc_number: Some(1),
            year: Some(2000),
            duration_ms: Some(180_000),
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        lib.upsert_track(
            &mk_track("/p/airbag.flac", "Airbag", ok_computer),
            Some(ok_computer),
            &[(radiohead, 0)],
        )
        .unwrap();
        lib.upsert_track(
            &mk_track("/p/everything.flac", "Everything In Its Right Place", kid_a),
            Some(kid_a),
            &[(radiohead, 0)],
        )
        .unwrap();
        lib.upsert_track(
            &mk_track("/p/other.flac", "Other Song", other_album),
            Some(other_album),
            &[(other, 0)],
        )
        .unwrap();

        let tracks = lib
            .tracks_by_artist(radiohead, ArtistGrouping::TrackArtist)
            .unwrap();
        assert_eq!(tracks.len(), 2);
        // Ordered by album year ASC: OK Computer (1997) first, Kid A (2000) second.
        assert_eq!(tracks[0].title, "Airbag");
        assert_eq!(tracks[1].title, "Everything In Its Right Place");
        // Other artist's track is not returned.
        assert!(tracks.iter().all(|t| t.title != "Other Song"));
    }

    #[test]
    fn test_track_artists_map_returns_names_in_position_order() {
        let (lib, _path) = create_test_db();
        let a1 = lib.upsert_artist("Lead").unwrap();
        let a2 = lib.upsert_artist("Feat").unwrap();
        let album = lib.upsert_album("Album", None, None).unwrap();
        let track = NewTrack {
            path: "/music/t.flac".into(),
            title: Some("Track".into()),
            album_title: Some("Album".into()),
            artist_names: vec!["Lead".into(), "Feat".into()],
            track_number: None,
            disc_number: None,
            year: None,
            duration_ms: None,
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        let id = lib
            .upsert_track(&track, Some(album), &[(a1, 0), (a2, 1)])
            .unwrap();
        let map = lib.track_artists_map(&[id]).unwrap();
        assert_eq!(
            map.get(&id).unwrap(),
            &vec!["Lead".to_string(), "Feat".to_string()]
        );
    }

    #[test]
    fn test_track_artists_map_empty_input() {
        let (lib, _path) = create_test_db();
        let map = lib.track_artists_map(&[]).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn test_delete_playlist_cascades_to_playlist_tracks() {
        // Verifies the FK pragma + ON DELETE CASCADE: dropping a playlist
        // must take its membership rows with it. Without `PRAGMA foreign_keys
        // = ON` this silently leaves orphaned playlist_tracks rows.
        let (lib, _path) = create_test_db();
        let track_id = seed_track(&lib, "Song", "Album", "Artist");
        let playlist_id = lib.create_playlist("My Playlist").unwrap();
        lib.add_track_to_playlist(playlist_id, track_id).unwrap();
        assert_eq!(lib.tracks_for_playlist(playlist_id).unwrap().len(), 1);

        lib.delete_playlist(playlist_id).unwrap();
        // The membership row is gone — playlists_containing_track must not
        // return the deleted playlist id.
        assert!(lib.playlists_containing_track(track_id).unwrap().is_empty());
    }

    #[test]
    fn test_add_track_to_playlist_is_idempotent() {
        // Double-click / stale `containing` UI checks should be harmless.
        let (lib, _path) = create_test_db();
        let track_id = seed_track(&lib, "Song", "Album", "Artist");
        let playlist_id = lib.create_playlist("My Playlist").unwrap();
        lib.add_track_to_playlist(playlist_id, track_id).unwrap();
        lib.add_track_to_playlist(playlist_id, track_id).unwrap();
        assert_eq!(lib.tracks_for_playlist(playlist_id).unwrap().len(), 1);
    }

    #[test]
    fn test_move_track_in_playlist_reorders_positions() {
        let (lib, _path) = create_test_db();
        let a = seed_track(&lib, "A", "Album", "Artist");
        let b = seed_track(&lib, "B", "Album", "Artist");
        let c = seed_track(&lib, "C", "Album", "Artist");
        let playlist_id = lib.create_playlist("Order").unwrap();
        lib.add_track_to_playlist(playlist_id, a).unwrap();
        lib.add_track_to_playlist(playlist_id, b).unwrap();
        lib.add_track_to_playlist(playlist_id, c).unwrap();

        let ids = |lib: &SqliteLibrary| -> Vec<i64> {
            lib.tracks_for_playlist(playlist_id)
                .unwrap()
                .iter()
                .map(|t| t.id)
                .collect()
        };

        lib.move_track_in_playlist(playlist_id, 0, 2).unwrap();
        assert_eq!(ids(&lib), vec![b, c, a]);

        lib.move_track_in_playlist(playlist_id, 2, 0).unwrap();
        assert_eq!(ids(&lib), vec![a, b, c]);

        lib.move_track_in_playlist(playlist_id, 1, 1).unwrap();
        assert_eq!(ids(&lib), vec![a, b, c]);

        lib.move_track_in_playlist(playlist_id, 0, 9).unwrap();
        assert_eq!(ids(&lib), vec![a, b, c]);
    }

    fn content_size(title: &str) -> u64 {
        title.bytes().fold(1_000_003u64, |acc, b| {
            acc.wrapping_mul(31).wrapping_add(b as u64)
        }) % 1_000_000_000
    }

    fn scan_track(path: &str, title: &str) -> ScanTrack {
        ScanTrack {
            path: path.into(),
            title: Some(title.into()),
            file_size: Some(content_size(title)),
            album_title: Some("Album".into()),
            artist_names: vec!["Artist".into()],
            album_artist_names: vec!["Artist".into()],
            track_number: Some(1),
            disc_number: Some(1),
            year: Some(2020),
            duration_ms: Some(180_000),
            ..Default::default()
        }
    }

    fn scan(lib: &SqliteLibrary, tracks: Vec<ScanTrack>) {
        let mut session = lib.open_scan_session().unwrap();
        session.clear().unwrap();
        for track in tracks {
            session.add_track(track).unwrap();
        }
        session.finish().unwrap();
    }

    fn folders(paths: &[&str]) -> Vec<LocalFolder> {
        paths
            .iter()
            .map(|path| LocalFolder {
                path: (*path).into(),
                available: true,
            })
            .collect()
    }

    fn id_of(lib: &SqliteLibrary, path: &str) -> i64 {
        lib.all_tracks()
            .unwrap()
            .into_iter()
            .find(|t| t.path == path)
            .unwrap_or_else(|| panic!("{path} is not in the library"))
            .id
    }

    #[test]
    fn a_playlist_keeps_its_track_across_a_rescan_under_the_same_id() {
        let (lib, _path) = create_test_db();
        scan(&lib, vec![scan_track("/m/keep.flac", "Keep")]);
        let track_id = id_of(&lib, "/m/keep.flac");
        let playlist_id = lib.create_playlist("Keepers").unwrap();
        lib.add_track_to_playlist(playlist_id, track_id).unwrap();

        scan(&lib, vec![scan_track("/m/keep.flac", "Keep")]);

        assert_eq!(id_of(&lib, "/m/keep.flac"), track_id);
        let tracks = lib.tracks_for_playlist(playlist_id).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, track_id);
        assert!(tracks[0].available);
    }

    #[test]
    fn a_track_missing_from_a_rescan_stays_in_its_playlist_and_comes_back() {
        let (lib, _path) = create_test_db();
        scan(
            &lib,
            vec![
                scan_track("/m/kept.flac", "Kept"),
                scan_track("/m/gone.flac", "Gone"),
            ],
        );
        let kept = id_of(&lib, "/m/kept.flac");
        let gone = id_of(&lib, "/m/gone.flac");
        let playlist_id = lib.create_playlist("Mixed").unwrap();
        lib.add_track_to_playlist(playlist_id, gone).unwrap();
        lib.add_track_to_playlist(playlist_id, kept).unwrap();

        scan(&lib, vec![scan_track("/m/kept.flac", "Kept")]);

        let tracks = lib.tracks_for_playlist(playlist_id).unwrap();
        let shape: Vec<(i64, &str, bool)> = tracks
            .iter()
            .map(|t| (t.id, t.title.as_str(), t.available))
            .collect();
        assert_eq!(shape, vec![(gone, "Gone", false), (kept, "Kept", true)]);
        assert_eq!(tracks[0].path, "/m/gone.flac");
        assert_eq!(tracks[0].duration_ms, Some(180_000));

        scan(
            &lib,
            vec![
                scan_track("/m/kept.flac", "Kept"),
                scan_track("/m/gone.flac", "Gone"),
            ],
        );
        assert_eq!(id_of(&lib, "/m/gone.flac"), gone);
        assert!(
            lib.tracks_for_playlist(playlist_id)
                .unwrap()
                .iter()
                .all(|t| t.available)
        );
    }

    #[test]
    fn fetched_lyrics_survive_a_rescan() {
        let (lib, _path) = create_test_db();
        scan(&lib, vec![scan_track("/m/fetched.flac", "Fetched")]);
        let track_id = id_of(&lib, "/m/fetched.flac");
        lib.upsert_lyrics(track_id, "la la la", "lrclib", false)
            .unwrap();

        scan(&lib, vec![scan_track("/m/fetched.flac", "Fetched")]);

        let kept = lib.lyrics_for_track(track_id).unwrap().unwrap();
        assert_eq!(kept.text, "la la la");
        assert_eq!(kept.source, "lrclib");
    }

    #[test]
    fn clear_drops_disk_lyrics_but_keeps_fetched_ones() {
        let (lib, _path) = create_test_db();
        let lrc = seed_track(&lib, "Sidecar", "Album", "Artist");
        let embedded = seed_track(&lib, "Tagged", "Album", "Artist");
        let fetched = seed_track(&lib, "Fetched", "Album", "Artist");
        lib.upsert_lyrics(lrc, "from sidecar", "lrc", false)
            .unwrap();
        lib.upsert_lyrics(embedded, "from tag", "embedded", false)
            .unwrap();
        lib.upsert_lyrics(fetched, "from net", "lrclib", false)
            .unwrap();

        lib.clear().unwrap();

        assert!(lib.lyrics_for_track(lrc).unwrap().is_none());
        assert!(lib.lyrics_for_track(embedded).unwrap().is_none());
        assert_eq!(
            lib.lyrics_for_track(fetched).unwrap().unwrap().text,
            "from net"
        );
    }

    #[test]
    fn fresh_disk_lyrics_win_over_fetched_ones() {
        let (lib, _path) = create_test_db();
        scan(&lib, vec![scan_track("/m/song.flac", "Song")]);
        let track_id = id_of(&lib, "/m/song.flac");
        lib.upsert_lyrics(track_id, "stale fetched", "lrclib", false)
            .unwrap();

        let mut with_sidecar = scan_track("/m/song.flac", "Song");
        with_sidecar.lyrics = Some(ScanLyrics {
            text: "fresh from disk".into(),
            source: "lrc".into(),
        });
        scan(&lib, vec![with_sidecar]);

        let kept = lib.lyrics_for_track(track_id).unwrap().unwrap();
        assert_eq!(kept.text, "fresh from disk");
        assert_eq!(kept.source, "lrc");
    }

    #[test]
    fn a_rescan_sweeps_items_nothing_references() {
        let (lib, path) = create_test_db();
        scan(
            &lib,
            vec![
                scan_track("/m/liked.flac", "Liked"),
                scan_track("/m/plain.flac", "Plain"),
            ],
        );
        lib.set_liked(id_of(&lib, "/m/liked.flac"), true).unwrap();

        scan(&lib, vec![]);

        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 1);
        let liked = lib.liked_tracks().unwrap();
        assert_eq!(liked.len(), 1);
        assert_eq!(liked[0].title, "Liked");
        assert!(!liked[0].available);
    }

    #[test]
    fn a_swept_id_is_never_handed_to_another_track() {
        let (lib, _path) = create_test_db();
        scan(
            &lib,
            vec![scan_track("/m/a.flac", "A"), scan_track("/m/b.flac", "B")],
        );
        let swept = id_of(&lib, "/m/b.flac");

        scan(&lib, vec![scan_track("/m/a.flac", "A")]);
        scan(
            &lib,
            vec![scan_track("/m/a.flac", "A"), scan_track("/m/c.flac", "C")],
        );

        assert!(id_of(&lib, "/m/c.flac") > swept);
    }

    #[test]
    fn track_artists_map_has_no_parameter_ceiling_and_names_unavailable_tracks() {
        let (lib, _path) = create_test_db();
        scan(
            &lib,
            vec![
                scan_track("/m/kept.flac", "Kept"),
                scan_track("/m/gone.flac", "Gone"),
            ],
        );
        let kept = id_of(&lib, "/m/kept.flac");
        let gone = id_of(&lib, "/m/gone.flac");
        lib.set_liked(gone, true).unwrap();
        scan(&lib, vec![scan_track("/m/kept.flac", "Kept")]);

        let mut ids: Vec<i64> = (1_000_000..1_040_000).collect();
        ids.push(kept);
        ids.push(gone);
        let map = lib.track_artists_map(&ids).unwrap();

        assert_eq!(map.get(&kept), Some(&vec!["Artist".to_string()]));
        assert_eq!(map.get(&gone), Some(&vec!["Artist".to_string()]));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn the_guard_refuses_to_delete_an_item_that_carries_user_data() {
        let (lib, path) = create_test_db();
        scan(&lib, vec![scan_track("/m/liked.flac", "Liked")]);
        let track_id = id_of(&lib, "/m/liked.flac");
        lib.set_liked(track_id, true).unwrap();

        let conn = rusqlite::Connection::open(&path).unwrap();
        let err = conn
            .execute("DELETE FROM media_items WHERE id = ?1", [track_id])
            .unwrap_err();
        assert!(err.to_string().contains("referenced by user data"));
    }

    #[test]
    fn removing_a_folder_keeps_its_likes_and_re_adding_it_revives_them() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/a.flac", "A")]);
        let track_id = id_of(&lib, "/music/a.flac");
        lib.set_liked(track_id, true).unwrap();

        lib.reconcile_local_sources(&folders(&[])).unwrap();
        scan(&lib, vec![]);
        let liked = lib.liked_tracks().unwrap();
        assert_eq!(liked.len(), 1);
        assert!(!liked[0].available);

        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/a.flac", "A")]);
        assert_eq!(id_of(&lib, "/music/a.flac"), track_id);
        let liked = lib.liked_tracks().unwrap();
        assert_eq!(liked.len(), 1);
        assert!(liked[0].available);
        assert!(liked[0].liked);
    }

    fn untagged(path: &str, title: &str) -> ScanTrack {
        ScanTrack {
            path: path.into(),
            title: Some(title.into()),
            file_size: Some(content_size(title)),
            duration_ms: Some(120_000),
            ..Default::default()
        }
    }

    #[test]
    fn a_moved_file_keeps_its_id_likes_and_playlist_entries() {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/old/a.flac", "A")]);
        let track_id = id_of(&lib, "/music/old/a.flac");
        lib.set_liked(track_id, true).unwrap();
        let playlist_id = lib.create_playlist("Mix").unwrap();
        lib.add_track_to_playlist(playlist_id, track_id).unwrap();

        scan(&lib, vec![scan_track("/music/new/a.flac", "A")]);

        assert_eq!(id_of(&lib, "/music/new/a.flac"), track_id);
        let playlist = lib.tracks_for_playlist(playlist_id).unwrap();
        assert_eq!(playlist.len(), 1);
        assert!(playlist[0].available);
        assert!(playlist[0].liked);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 1);
        assert_eq!(
            count_rows(&path, "SELECT COUNT(*) FROM adoptions WHERE tier = 'file'"),
            1
        );
    }

    #[test]
    fn a_folder_re_added_from_a_new_mount_point_is_re_attached_by_relative_path() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/Volumes/Old"]))
            .unwrap();
        scan(&lib, vec![untagged("/Volumes/Old/x/01.flac", "01")]);
        let track_id = id_of(&lib, "/Volumes/Old/x/01.flac");
        lib.set_liked(track_id, true).unwrap();

        lib.reconcile_local_sources(&folders(&["/Volumes/New"]))
            .unwrap();
        scan(&lib, vec![untagged("/Volumes/New/x/01.flac", "01")]);

        assert_eq!(id_of(&lib, "/Volumes/New/x/01.flac"), track_id);
        assert!(lib.liked_tracks().unwrap()[0].available);
    }

    #[test]
    fn a_copy_next_to_its_original_is_a_separate_track() {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/a.flac", "A")]);
        let original = id_of(&lib, "/music/a.flac");
        lib.set_liked(original, true).unwrap();

        scan(
            &lib,
            vec![
                scan_track("/music/a.flac", "A"),
                scan_track("/music/copy/a.flac", "A"),
            ],
        );

        assert_eq!(id_of(&lib, "/music/a.flac"), original);
        assert_ne!(id_of(&lib, "/music/copy/a.flac"), original);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM adoptions"), 0);
    }

    #[test]
    fn a_copy_elsewhere_joins_the_track_of_an_offline_folder_which_is_never_swept() {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/a", "/b"]))
            .unwrap();
        scan(
            &lib,
            vec![
                scan_track("/a/one.flac", "One"),
                scan_track("/b/two.flac", "Two"),
                scan_track("/b/plain.flac", "Plain"),
            ],
        );
        let two = id_of(&lib, "/b/two.flac");
        let plain = id_of(&lib, "/b/plain.flac");
        lib.set_liked(two, true).unwrap();

        lib.reconcile_local_sources(&[
            LocalFolder {
                path: "/a".into(),
                available: true,
            },
            LocalFolder {
                path: "/b".into(),
                available: false,
            },
        ])
        .unwrap();
        scan(
            &lib,
            vec![
                scan_track("/a/one.flac", "One"),
                scan_track("/a/two-copy.flac", "Two"),
            ],
        );

        assert_eq!(id_of(&lib, "/a/two-copy.flac"), two);
        assert!(!lib.all_tracks().unwrap().iter().any(|t| t.id == plain));
        assert!(lib.liked_tracks().unwrap()[0].available);
        assert_eq!(
            count_rows(
                &path,
                "SELECT COUNT(*) FROM media_bindings WHERE present = 1 AND source_key LIKE '/b/%'"
            ),
            2
        );

        lib.reconcile_local_sources(&folders(&["/a", "/b"]))
            .unwrap();
        scan(
            &lib,
            vec![
                scan_track("/a/one.flac", "One"),
                scan_track("/a/two-copy.flac", "Two"),
                scan_track("/b/two.flac", "Two"),
                scan_track("/b/plain.flac", "Plain"),
            ],
        );
        assert_eq!(id_of(&lib, "/a/two-copy.flac"), two);
        assert_eq!(id_of(&lib, "/b/plain.flac"), plain);
        assert_eq!(paths(&lib).len(), 3);
        assert!(lib.liked_tracks().unwrap()[0].available);
    }

    #[test]
    fn a_copy_made_before_the_original_was_deleted_takes_over_its_like() {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/a.flac", "A")]);
        let original = id_of(&lib, "/music/a.flac");
        lib.set_liked(original, true).unwrap();
        let playlist_id = lib.create_playlist("Mix").unwrap();
        lib.add_track_to_playlist(playlist_id, original).unwrap();
        let mut copy = scan_track("/music/copy/a.flac", "A");
        copy.lyrics = Some(ScanLyrics {
            text: "from disk".into(),
            source: "lrc".into(),
        });
        scan(&lib, vec![scan_track("/music/a.flac", "A"), copy.clone()]);
        let copy_id = id_of(&lib, "/music/copy/a.flac");
        assert_ne!(copy_id, original);

        scan(&lib, vec![copy]);

        assert_eq!(id_of(&lib, "/music/copy/a.flac"), original);
        let liked = lib.liked_tracks().unwrap();
        assert!(liked[0].available);
        assert_eq!(liked[0].path, "/music/copy/a.flac");
        assert!(lib.tracks_for_playlist(playlist_id).unwrap()[0].available);
        assert_eq!(
            lib.track_artists(original).unwrap(),
            vec!["Artist".to_string()]
        );
        assert_eq!(
            lib.lyrics_for_track(original).unwrap().unwrap().text,
            "from disk"
        );
        assert_eq!(
            count_rows(
                &path,
                &format!("SELECT COUNT(*) FROM media_items WHERE id = {copy_id}")
            ),
            0
        );
        assert_eq!(
            count_rows(&path, "SELECT COUNT(*) FROM pragma_foreign_key_check"),
            0
        );
    }

    #[test]
    fn a_moved_file_restored_to_its_old_path_stays_in_the_library_beside_the_move() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/a.flac", "A")]);
        let original = id_of(&lib, "/music/a.flac");
        lib.set_liked(original, true).unwrap();
        scan(&lib, vec![scan_track("/music/b.flac", "A")]);
        assert_eq!(id_of(&lib, "/music/b.flac"), original);

        for _ in 0..2 {
            scan(
                &lib,
                vec![
                    scan_track("/music/a.flac", "A"),
                    scan_track("/music/b.flac", "A"),
                ],
            );
            let a = id_of(&lib, "/music/a.flac");
            let b = id_of(&lib, "/music/b.flac");
            assert_ne!(a, b);
            assert!(a == original || b == original);
            assert_eq!(lib.liked_tracks().unwrap().len(), 1);
            assert!(lib.liked_tracks().unwrap()[0].available);
        }
    }

    #[test]
    fn the_same_file_in_two_folders_is_one_track_whatever_happens_to_either_folder() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/a", "/b"]))
            .unwrap();
        scan(
            &lib,
            vec![scan_track("/a/y.flac", "X"), scan_track("/b/x.flac", "X")],
        );
        assert_eq!(paths(&lib), vec!["/a/y.flac".to_string()]);
        let liked = id_of(&lib, "/a/y.flac");
        lib.set_liked(liked, true).unwrap();

        lib.reconcile_local_sources(&folders(&["/b"])).unwrap();
        scan(&lib, vec![scan_track("/b/x.flac", "X")]);
        assert_eq!(id_of(&lib, "/b/x.flac"), liked);

        lib.reconcile_local_sources(&folders(&["/a", "/b"]))
            .unwrap();
        scan(
            &lib,
            vec![
                scan_track("/a/y.flac", "X"),
                scan_track("/a/new.flac", "New"),
                scan_track("/b/x.flac", "X"),
            ],
        );
        assert_eq!(
            paths(&lib),
            vec!["/a/new.flac".to_string(), "/a/y.flac".to_string()]
        );
        assert_eq!(id_of(&lib, "/a/y.flac"), liked);
        assert_eq!(lib.liked_tracks().unwrap().len(), 1);
        assert!(lib.liked_tracks().unwrap()[0].available);
    }

    #[test]
    fn a_revived_like_keeps_its_fetched_lyrics_when_the_file_has_none() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/a.flac", "A")]);
        let original = id_of(&lib, "/music/a.flac");
        lib.set_liked(original, true).unwrap();
        lib.upsert_lyrics(original, "from the net", "lrclib", false)
            .unwrap();
        let copy = scan_track("/music/copy/a.flac", "A");
        scan(&lib, vec![scan_track("/music/a.flac", "A"), copy.clone()]);
        scan(
            &lib,
            vec![copy.clone(), scan_track("/music/new.flac", "New")],
        );

        assert_eq!(id_of(&lib, "/music/copy/a.flac"), original);
        let lyrics = lib.lyrics_for_track(original).unwrap().unwrap();
        assert_eq!(lyrics.text, "from the net");
        assert_eq!(lyrics.source, "lrclib");
    }

    #[test]
    fn sources_report_availability_and_the_tracks_each_folder_contributes() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/a", "/b"]))
            .unwrap();
        scan(
            &lib,
            vec![
                scan_track("/a/1.flac", "One"),
                scan_track("/a/2.flac", "Two"),
                scan_track("/b/3.flac", "Three"),
            ],
        );
        let shape = |lib: &SqliteLibrary| -> Vec<(String, bool, bool, i64)> {
            lib.sources()
                .unwrap()
                .into_iter()
                .map(|s| (s.uri, s.enabled, s.available, s.track_count))
                .collect()
        };
        assert_eq!(
            shape(&lib),
            vec![("/a".into(), true, true, 2), ("/b".into(), true, true, 1)]
        );

        lib.reconcile_local_sources(&[
            LocalFolder {
                path: "/a".into(),
                available: true,
            },
            LocalFolder {
                path: "/b".into(),
                available: false,
            },
        ])
        .unwrap();
        scan(
            &lib,
            vec![
                scan_track("/a/1.flac", "One"),
                scan_track("/a/2.flac", "Two"),
            ],
        );
        assert_eq!(
            shape(&lib),
            vec![("/a".into(), true, true, 2), ("/b".into(), true, false, 1)]
        );

        lib.reconcile_local_sources(&folders(&["/a"])).unwrap();
        assert!(!shape(&lib)[1].1);
    }

    fn remote_song(key: &str, title: &str) -> RemoteSong {
        RemoteSong {
            key: key.into(),
            title: title.into(),
            size: Some(content_size(title) as i64),
            artist: Some("Artist".into()),
            album: Some("Album".into()),
            track_number: Some(1),
            disc_number: Some(1),
            year: Some(2020),
            duration_ms: Some(180_000),
            suffix: Some("flac".into()),
            ..Default::default()
        }
    }

    fn server(lib: &SqliteLibrary) -> i64 {
        lib.reconcile_remote_sources(
            "subsonic",
            &[RemoteSource {
                uri: "me@http://nas".into(),
                name: "nas".into(),
            }],
        )
        .unwrap();
        lib.sources()
            .unwrap()
            .into_iter()
            .find(|s| s.kind == "subsonic")
            .unwrap()
            .id
    }

    #[test]
    fn a_scan_holds_the_write_lock_only_until_it_catches_up() {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/a.flac", "A")]);
        let other = rusqlite::Connection::open(&path).unwrap();
        other.busy_timeout(std::time::Duration::ZERO).unwrap();
        let can_write = || other.execute_batch("BEGIN IMMEDIATE; COMMIT").is_ok();

        let mut session = lib.open_scan_session().unwrap();
        assert!(can_write());
        session.clear().unwrap();
        assert!(!can_write());
        session.flush().unwrap();
        assert!(can_write());
        session
            .add_track(scan_track("/music/new.flac", "New"))
            .unwrap();
        assert!(can_write());
        session.add_track(scan_track("/music/a.flac", "A")).unwrap();
        assert!(!can_write());
        session.flush().unwrap();
        assert!(can_write());
        session.finish().unwrap();
        assert!(can_write());
        assert_eq!(paths(&lib).len(), 2);
    }

    #[test]
    fn a_file_listed_twice_in_one_scan_keeps_its_id_and_playlist_entry() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music", "/music/rock"]))
            .unwrap();
        let track = untagged("/music/rock/a.flac", "A");
        scan(&lib, vec![track.clone(), track.clone()]);
        let track_id = id_of(&lib, "/music/rock/a.flac");
        let playlist_id = lib.create_playlist("Mix").unwrap();
        lib.add_track_to_playlist(playlist_id, track_id).unwrap();

        for _ in 0..3 {
            scan(&lib, vec![track.clone(), track.clone()]);
        }

        assert_eq!(id_of(&lib, "/music/rock/a.flac"), track_id);
        assert_eq!(
            lib.playback_locators(track_id).unwrap(),
            vec![("/music/rock/a.flac".to_string(), 0)]
        );
        let playlist = lib.tracks_for_playlist(playlist_id).unwrap();
        assert_eq!(playlist.len(), 1);
        assert!(playlist[0].available);
    }

    #[test]
    fn a_like_waits_for_the_scan_batch_instead_of_failing() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/a.flac", "A")]);
        let track_id = id_of(&lib, "/music/a.flac");
        let playlist_id = lib.create_playlist("Mix").unwrap();
        let lib = std::sync::Arc::new(lib);
        let mut session = lib.open_scan_session().unwrap();
        session.clear().unwrap();

        let writer = lib.clone();
        let edits = std::thread::spawn(move || {
            (
                writer.set_liked(track_id, true).is_ok(),
                writer.add_track_to_playlist(playlist_id, track_id).is_ok(),
            )
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        session.flush().unwrap();

        assert_eq!(edits.join().unwrap(), (true, true));
        drop(session);
    }

    #[test]
    fn a_migrated_file_that_never_shows_up_again_stops_counting_as_unplaced() {
        let path = fresh_db_path();
        build_v8_db(
            &path,
            &format!(
                "{V8_CATALOG}
                INSERT INTO tracks (id, path, title, duration_ms, start_offset_ms, liked, is_cue)
                    VALUES (13, '/m/deleted.flac', 'Deleted', 1000, 0, 1, 0),
                           (14, '/offline/kept.flac', 'Kept', 1000, 0, 1, 0);"
            ),
        );
        let lib = SqliteLibrary::open_at(&path).unwrap();
        lib.reconcile_local_sources(&[online("/m"), offline("/offline")])
            .unwrap();
        let mut cue_b = scan_track("/m/cue.flac", "Cue B");
        cue_b.start_offset_ms = Some(2000);
        scan(
            &lib,
            vec![
                scan_track("/m/one.flac", "One"),
                scan_track("/m/cue.flac", "Cue A"),
                cue_b,
            ],
        );
        assert!(lib.has_unplaced_media().unwrap());

        lib.reconcile_local_sources(&[online("/m")]).unwrap();
        scan(
            &lib,
            vec![
                scan_track("/m/one.flac", "One"),
                scan_track("/m/cue.flac", "Cue A"),
            ],
        );
        assert!(!lib.has_unplaced_media().unwrap());
        let liked: Vec<i64> = lib.liked_tracks().unwrap().iter().map(|t| t.id).collect();
        assert!(liked.contains(&13) && liked.contains(&14));
    }

    #[test]
    fn the_catalog_row_comes_from_the_file_in_the_first_folder_whatever_the_scan_order() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/a", "/b"]))
            .unwrap();
        let mut first = scan_track("/a/x.flac", "X");
        first.year = Some(2000);
        let mut second = scan_track("/b/x.flac", "X");
        second.year = Some(1999);
        second.is_cue = true;
        second.start_offset_ms = Some(0);
        for order in [
            vec![first.clone(), second.clone()],
            vec![second.clone(), first.clone()],
        ] {
            scan(&lib, order);
            let tracks = lib.all_tracks().unwrap();
            assert_eq!(tracks.len(), 1);
            assert_eq!(tracks[0].path, "/a/x.flac");
            assert_eq!(tracks[0].year, Some(2000));
            assert!(!tracks[0].is_cue);
        }
    }

    #[test]
    fn a_file_restored_beside_its_move_splits_off_even_when_another_folder_holds_the_track() {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/a", "/b"]))
            .unwrap();
        scan(
            &lib,
            vec![scan_track("/a/s.flac", "S"), scan_track("/b/s.flac", "S")],
        );
        let id = id_of(&lib, "/a/s.flac");
        scan(
            &lib,
            vec![scan_track("/a/s.flac", "S"), scan_track("/b/s2.flac", "S")],
        );
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 1);

        scan(
            &lib,
            vec![
                scan_track("/a/s.flac", "S"),
                scan_track("/b/s2.flac", "S"),
                scan_track("/b/s.flac", "S"),
            ],
        );
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 2);
        assert_eq!(
            count_rows(
                &path,
                &format!(
                    "SELECT COUNT(*) FROM media_bindings b JOIN sources s ON s.id = b.source_id \
                     WHERE b.item_id = {id} AND b.present = 1 AND s.uri = '/b'"
                )
            ),
            1
        );
        assert_eq!(paths(&lib).len(), 2);
    }

    #[test]
    fn a_retagged_copy_in_another_folder_joins_by_tags_when_its_size_differs() {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/a"])).unwrap();
        scan(&lib, vec![scan_track("/a/x.flac", "X")]);
        let id = id_of(&lib, "/a/x.flac");
        lib.reconcile_local_sources(&[offline("/a"), online("/b")])
            .unwrap();
        let mut copy = scan_track("/b/x.flac", "X");
        copy.file_size = Some(1);
        copy.duration_ms = Some(181_000);
        scan(&lib, vec![copy]);
        assert_eq!(id_of(&lib, "/b/x.flac"), id);
        assert_eq!(
            count_rows(&path, "SELECT COUNT(*) FROM adoptions WHERE tier = 'tags'"),
            1
        );
    }

    fn servers(lib: &SqliteLibrary, uris: &[&str]) -> Vec<i64> {
        let sources: Vec<RemoteSource> = uris
            .iter()
            .map(|uri| RemoteSource {
                uri: (*uri).into(),
                name: (*uri).into(),
            })
            .collect();
        lib.reconcile_remote_sources("subsonic", &sources).unwrap();
        let mut ids: Vec<i64> = lib
            .sources()
            .unwrap()
            .into_iter()
            .filter(|s| s.kind == "subsonic" && uris.contains(&s.uri.as_str()))
            .map(|s| s.id)
            .collect();
        ids.sort_unstable();
        ids
    }

    fn offline(path: &str) -> LocalFolder {
        LocalFolder {
            path: path.into(),
            available: false,
        }
    }

    fn online(path: &str) -> LocalFolder {
        LocalFolder {
            path: path.into(),
            available: true,
        }
    }

    #[test]
    fn a_folder_renamed_on_disk_and_added_again_while_the_old_path_stays_configured_keeps_everything()
     {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        let before = vec![
            scan_track("/music/x/a.flac", "A"),
            scan_track("/music/x/b.flac", "B"),
            scan_track("/music/x/c.flac", "C"),
            untagged("/music/loose/01.flac", "01"),
        ];
        scan(&lib, before);
        let a = id_of(&lib, "/music/x/a.flac");
        let b = id_of(&lib, "/music/x/b.flac");
        let c = id_of(&lib, "/music/x/c.flac");
        let loose = id_of(&lib, "/music/loose/01.flac");
        lib.set_liked(a, true).unwrap();
        lib.set_liked(loose, true).unwrap();
        let playlist = lib.create_playlist("Mix").unwrap();
        lib.add_track_to_playlist(playlist, b).unwrap();

        lib.reconcile_local_sources(&[offline("/music"), online("/music 2")])
            .unwrap();
        let after = vec![
            scan_track("/music 2/x/a.flac", "A"),
            scan_track("/music 2/x/b.flac", "B"),
            scan_track("/music 2/x/c.flac", "C"),
            untagged("/music 2/loose/01.flac", "01"),
        ];
        scan(&lib, after.clone());

        assert_eq!(id_of(&lib, "/music 2/x/a.flac"), a);
        assert_eq!(id_of(&lib, "/music 2/x/b.flac"), b);
        assert_eq!(id_of(&lib, "/music 2/x/c.flac"), c);
        assert_eq!(id_of(&lib, "/music 2/loose/01.flac"), loose);
        assert_eq!(paths(&lib).len(), 4);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 4);
        assert!(lib.liked_tracks().unwrap().iter().all(|t| t.available));
        assert!(lib.tracks_for_playlist(playlist).unwrap()[0].available);

        lib.reconcile_local_sources(&folders(&["/music 2"]))
            .unwrap();
        scan(&lib, after);
        assert_eq!(paths(&lib).len(), 4);
        assert_eq!(id_of(&lib, "/music 2/x/a.flac"), a);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 4);
    }

    #[test]
    fn folders_added_together_to_an_existing_library_all_join_the_same_track() {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/a"])).unwrap();
        scan(&lib, vec![scan_track("/a/x.flac", "X")]);
        let x = id_of(&lib, "/a/x.flac");

        lib.reconcile_local_sources(&folders(&["/a", "/b", "/c"]))
            .unwrap();
        scan(
            &lib,
            vec![
                scan_track("/a/x.flac", "X"),
                scan_track("/b/x.flac", "X"),
                scan_track("/c/copy of x.flac", "X"),
            ],
        );
        assert_eq!(paths(&lib), vec!["/a/x.flac".to_string()]);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 1);
        assert_eq!(
            count_rows(
                &path,
                &format!("SELECT COUNT(*) FROM media_bindings WHERE item_id = {x} AND present = 1")
            ),
            3
        );

        lib.reconcile_local_sources(&[offline("/a"), online("/b"), online("/c")])
            .unwrap();
        scan(
            &lib,
            vec![
                scan_track("/b/x.flac", "X"),
                scan_track("/c/copy of x.flac", "X"),
            ],
        );
        assert_eq!(paths(&lib), vec!["/b/x.flac".to_string()]);
        assert_eq!(id_of(&lib, "/b/x.flac"), x);
    }

    #[test]
    fn copies_inside_one_folder_stay_separate_tracks() {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/a", "/b"]))
            .unwrap();
        scan(
            &lib,
            vec![
                scan_track("/a/x.flac", "X"),
                scan_track("/a/x copy.flac", "X"),
                scan_track("/b/x.flac", "X"),
            ],
        );
        assert_eq!(paths(&lib).len(), 2);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 2);
        scan(
            &lib,
            vec![
                scan_track("/a/x.flac", "X"),
                scan_track("/a/x copy.flac", "X"),
                scan_track("/b/x.flac", "X"),
            ],
        );
        assert_eq!(paths(&lib).len(), 2);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 2);
    }

    #[test]
    fn cue_tracks_of_one_image_in_two_folders_join_track_by_track() {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/a", "/b"]))
            .unwrap();
        let cue = |root: &str, offset: u64, title: &str| {
            let mut track = untagged(&format!("{root}/image.ape"), title);
            track.start_offset_ms = Some(offset);
            track.is_cue = true;
            track.file_size = Some(777_000);
            track.duration_ms = Some(if offset == 0 { 60_000 } else { 90_000 });
            track
        };
        scan(
            &lib,
            vec![
                cue("/a", 0, "One"),
                cue("/a", 60_000, "Two"),
                cue("/b", 0, "Uno"),
                cue("/b", 60_000, "Dos"),
            ],
        );
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 2);
        assert_eq!(paths(&lib).len(), 2);
    }

    fn id_of_title(lib: &SqliteLibrary, title: &str) -> i64 {
        lib.all_tracks()
            .unwrap()
            .into_iter()
            .find(|t| t.title == title)
            .unwrap_or_else(|| panic!("{title} is not in the library"))
            .id
    }

    #[test]
    fn a_server_that_splits_cues_joins_each_local_cue_track_by_tags_not_by_the_image_size() {
        for server_first in [false, true] {
            let (lib, path) = create_test_db();
            let source = server(&lib);
            let cue = |offset: u64, title: &str, duration_ms: u64| {
                let mut track = scan_track("/music/image.flac", title);
                track.start_offset_ms = Some(offset);
                track.is_cue = true;
                track.file_size = Some(900_000);
                track.duration_ms = Some(duration_ms);
                track
            };
            let tracks = vec![
                cue(0, "One", 241_300),
                cue(241_300, "Two", 200_000),
                cue(441_300, "Three", 242_000),
            ];
            let song = |key: &str, title: &str, duration_ms: i64| {
                let mut song = remote_song(key, title);
                song.size = Some(900_000);
                song.duration_ms = Some(duration_ms);
                song
            };
            let songs = vec![
                song("one", "One", 240_000),
                song("two", "Two", 200_000),
                song("three", "Three", 241_000),
            ];
            if server_first {
                lib.apply_remote_listing(source, &songs, &[]).unwrap();
                scan(&lib, vec![]);
                lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
                scan(&lib, tracks);
            } else {
                lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
                scan(&lib, tracks.clone());
                lib.apply_remote_listing(source, &songs, &[]).unwrap();
                scan(&lib, tracks);
            }
            assert_eq!(
                count_rows(&path, "SELECT COUNT(*) FROM media_items"),
                3,
                "server first: {server_first}"
            );
            for (key, title) in [("one", "One"), ("two", "Two"), ("three", "Three")] {
                assert_eq!(
                    lib.items_for_remote_keys(source, &[key.into()]).unwrap(),
                    vec![id_of_title(&lib, title)],
                    "{key}, server first: {server_first}"
                );
            }
        }
    }

    #[test]
    fn a_server_copy_of_a_local_cue_image_stays_out_of_the_catalog_while_the_folder_is_there() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        let cue = |offset: u64, title: &str| {
            let mut track = scan_track("/music/image.flac", title);
            track.start_offset_ms = Some(offset);
            track.is_cue = true;
            track.file_size = Some(777_000);
            track.duration_ms = Some(60_000);
            track
        };
        let tracks = vec![cue(0, "One"), cue(60_000, "Two")];
        scan(&lib, tracks.clone());
        let source = server(&lib);
        let mut image = remote_song("img", "Image");
        image.size = Some(777_000);
        image.duration_ms = Some(120_000);
        let mut split = remote_song("two", "Two");
        split.size = Some(777_000);
        split.duration_ms = Some(60_000);
        lib.apply_remote_listing(source, &[image, split], &[])
            .unwrap();
        scan(&lib, tracks);
        assert_eq!(
            paths(&lib),
            vec!["/music/image.flac".to_string(), "/music/image.flac".into()]
        );
        assert_eq!(
            lib.items_for_remote_keys(source, &["two".into()]).unwrap(),
            vec![id_of_title(&lib, "Two")]
        );

        lib.reconcile_local_sources(&[LocalFolder {
            path: "/music".into(),
            available: false,
        }])
        .unwrap();
        scan(&lib, vec![]);
        let mut offline = paths(&lib);
        offline.sort();
        assert_eq!(
            offline,
            vec![
                remote::locator(source, "img", "flac"),
                remote::locator(source, "two", "flac")
            ]
        );

        lib.reconcile_local_sources(&folders(&[])).unwrap();
        scan(&lib, vec![]);
        let mut remaining = paths(&lib);
        remaining.sort();
        assert_eq!(
            remaining,
            vec![
                remote::locator(source, "img", "flac"),
                remote::locator(source, "two", "flac")
            ]
        );
    }

    #[test]
    fn a_local_file_and_its_server_copy_are_one_track_in_either_order_even_untagged() {
        for server_first in [false, true] {
            let (lib, path) = create_test_db();
            let source = server(&lib);
            let mut song = remote_song("s1", "01");
            song.artist = None;
            song.album = None;
            song.duration_ms = Some(120_000);
            let local = untagged("/music/01.flac", "01");
            if server_first {
                lib.apply_remote_listing(source, std::slice::from_ref(&song), &[])
                    .unwrap();
                scan(&lib, vec![]);
                lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
                scan(&lib, vec![local]);
            } else {
                lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
                scan(&lib, vec![local.clone()]);
                lib.apply_remote_listing(source, std::slice::from_ref(&song), &[])
                    .unwrap();
                scan(&lib, vec![local]);
            }
            assert_eq!(
                paths(&lib),
                vec!["/music/01.flac".to_string()],
                "server first: {server_first}"
            );
            assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 1);
        }
    }

    #[test]
    fn servers_of_different_kinds_share_an_item_and_rank_by_source_id() {
        let (lib, path) = create_test_db();
        let source = |kind: &str| RemoteSource {
            uri: format!("me@http://{kind}"),
            name: kind.into(),
        };
        lib.reconcile_remote_sources("jellyfin", &[source("jellyfin")])
            .unwrap();
        lib.reconcile_remote_sources("subsonic", &[source("subsonic")])
            .unwrap();
        let id_of_kind = |kind: &str| {
            lib.sources()
                .unwrap()
                .into_iter()
                .find(|s| s.kind == kind)
                .unwrap()
                .id
        };
        let (jellyfin, subsonic) = (id_of_kind("jellyfin"), id_of_kind("subsonic"));
        assert!(jellyfin < subsonic);

        lib.apply_remote_listing(subsonic, &[remote_song("s1", "A")], &[])
            .unwrap();
        let report = lib
            .apply_remote_listing(jellyfin, &[remote_song("j1", "A")], &[])
            .unwrap();
        assert_eq!((report.added, report.adopted), (0, 1));
        scan(&lib, vec![]);
        let from_jellyfin = remote::locator(jellyfin, "j1", "flac");
        let from_subsonic = remote::locator(subsonic, "s1", "flac");
        assert_eq!(paths(&lib), vec![from_jellyfin.clone()]);
        let item = id_of(&lib, &from_jellyfin);
        assert_eq!(
            lib.playback_locators(item).unwrap(),
            vec![(from_jellyfin, 0), (from_subsonic.clone(), 0)]
        );

        lib.reconcile_remote_sources("jellyfin", &[]).unwrap();
        assert!(
            lib.sources()
                .unwrap()
                .into_iter()
                .any(|s| s.kind == "subsonic" && s.enabled)
        );
        scan(&lib, vec![]);
        assert_eq!(paths(&lib), vec![from_subsonic.clone()]);
        assert_eq!(id_of(&lib, &from_subsonic), item);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 1);
    }

    #[test]
    fn a_second_server_joins_the_tracks_of_a_first_one_that_is_offline() {
        let (lib, path) = create_test_db();
        let ids = servers(&lib, &["me@http://one", "me@http://two"]);
        let (one, two) = (ids[0], ids[1]);
        lib.apply_remote_listing(one, &[remote_song("a1", "A")], &[])
            .unwrap();
        scan(&lib, vec![]);
        let a = id_of(&lib, &remote::locator(one, "a1", "flac"));
        lib.set_liked(a, true).unwrap();

        lib.set_source_available(one, false).unwrap();
        let report = lib
            .apply_remote_listing(two, &[remote_song("b7", "A")], &[])
            .unwrap();
        assert_eq!((report.added, report.adopted), (0, 1));
        scan(&lib, vec![]);
        assert_eq!(paths(&lib), vec![remote::locator(two, "b7", "flac")]);
        assert_eq!(id_of(&lib, &remote::locator(two, "b7", "flac")), a);

        lib.set_source_available(one, true).unwrap();
        scan(&lib, vec![]);
        assert_eq!(paths(&lib), vec![remote::locator(one, "a1", "flac")]);
        assert_eq!(
            lib.playback_locators(a).unwrap(),
            vec![
                (remote::locator(one, "a1", "flac"), 0),
                (remote::locator(two, "b7", "flac"), 0)
            ]
        );
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 1);
    }

    #[test]
    fn a_liked_track_whose_file_is_gone_takes_over_a_differently_tagged_server_copy() {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/a.flac", "A")]);
        let liked = id_of(&lib, "/music/a.flac");
        lib.set_liked(liked, true).unwrap();
        let source = server(&lib);
        let mut transcoded = remote_song("s1", "A");
        transcoded.album = Some("Album (Deluxe)".into());
        transcoded.size = Some(1);
        transcoded.duration_ms = Some(181_000);
        lib.apply_remote_listing(source, &[transcoded], &[])
            .unwrap();
        scan(&lib, vec![scan_track("/music/a.flac", "A")]);
        assert_eq!(paths(&lib).len(), 2);

        scan(&lib, vec![]);

        assert_eq!(paths(&lib), vec![remote::locator(source, "s1", "flac")]);
        assert_eq!(id_of(&lib, &remote::locator(source, "s1", "flac")), liked);
        assert!(lib.liked_tracks().unwrap()[0].available);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 1);
    }

    #[test]
    fn removing_a_folder_hands_its_liked_track_to_a_copy_in_another_folder() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/a", "/b"]))
            .unwrap();
        let mut other = scan_track("/b/a.mp3", "A");
        other.album_title = Some("Best Of".into());
        other.file_size = Some(1);
        scan(&lib, vec![scan_track("/a/a.flac", "A"), other.clone()]);
        let liked = id_of(&lib, "/a/a.flac");
        lib.set_liked(liked, true).unwrap();
        assert_eq!(paths(&lib).len(), 2);

        lib.reconcile_local_sources(&folders(&["/b"])).unwrap();
        scan(&lib, vec![other]);

        assert_eq!(id_of(&lib, "/b/a.mp3"), liked);
        assert!(lib.liked_tracks().unwrap()[0].available);
    }

    #[test]
    fn an_offline_track_is_not_merged_with_another_recording_on_title_alone() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/a", "/b"]))
            .unwrap();
        scan(&lib, vec![scan_track("/a/a.flac", "A")]);
        let liked = id_of(&lib, "/a/a.flac");
        lib.set_liked(liked, true).unwrap();

        lib.reconcile_local_sources(&[offline("/a"), online("/b")])
            .unwrap();
        let mut live = scan_track("/b/a-live.flac", "A");
        live.album_title = Some("Live".into());
        live.file_size = Some(1);
        scan(&lib, vec![live]);

        assert_ne!(id_of(&lib, "/b/a-live.flac"), liked);
        assert!(!lib.liked_tracks().unwrap()[0].available);
    }

    fn paths(lib: &SqliteLibrary) -> Vec<String> {
        let mut paths: Vec<String> = lib
            .all_tracks()
            .unwrap()
            .into_iter()
            .map(|t| t.path)
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn server_songs_join_the_catalog_and_hide_while_the_server_is_away() {
        let (lib, _path) = create_test_db();
        let source = server(&lib);
        let report = lib
            .apply_remote_listing(source, &[remote_song("s1", "Remote")], &[])
            .unwrap();
        assert_eq!((report.added, report.adopted), (1, 0));
        scan(&lib, vec![]);
        let locator = remote::locator(source, "s1", "flac");
        assert_eq!(paths(&lib), vec![locator.clone()]);
        let track_id = id_of(&lib, &locator);
        lib.set_liked(track_id, true).unwrap();

        lib.set_source_available(source, false).unwrap();
        scan(&lib, vec![]);
        assert!(paths(&lib).is_empty());
        assert!(!lib.liked_tracks().unwrap()[0].available);

        lib.set_source_available(source, true).unwrap();
        scan(&lib, vec![]);
        assert_eq!(id_of(&lib, &locator), track_id);
        assert_eq!(
            lib.items_for_remote_keys(source, &["s1".into()]).unwrap(),
            vec![track_id]
        );
    }

    #[test]
    fn a_server_copy_of_a_local_file_is_one_track_not_two() {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/x/a.flac", "A")]);
        let local = id_of(&lib, "/music/x/a.flac");
        let source = server(&lib);

        let report = lib
            .apply_remote_listing(source, &[remote_song("s1", "A")], &[])
            .unwrap();
        assert_eq!((report.added, report.adopted), (0, 1));
        scan(&lib, vec![scan_track("/music/x/a.flac", "A")]);

        assert_eq!(paths(&lib), vec!["/music/x/a.flac".to_string()]);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 1);
        assert_eq!(
            lib.items_for_remote_keys(source, &["s1".into()]).unwrap(),
            vec![local]
        );

        lib.reconcile_local_sources(&folders(&[])).unwrap();
        scan(&lib, vec![]);
        assert_eq!(paths(&lib), vec![remote::locator(source, "s1", "flac")]);
        assert_eq!(id_of(&lib, &remote::locator(source, "s1", "flac")), local);
    }

    #[test]
    fn playback_locators_list_the_local_copy_first_and_skip_offline_sources() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/x/a.flac", "A")]);
        let id = id_of(&lib, "/music/x/a.flac");
        let source = server(&lib);
        lib.apply_remote_listing(source, &[remote_song("s1", "A")], &[])
            .unwrap();

        let remote = remote::locator(source, "s1", "flac");
        assert_eq!(
            lib.playback_locators(id).unwrap(),
            vec![("/music/x/a.flac".to_string(), 0), (remote.clone(), 0)]
        );
        lib.set_source_available(source, false).unwrap();
        assert_eq!(
            lib.playback_locators(id).unwrap(),
            vec![("/music/x/a.flac".to_string(), 0)]
        );
        lib.set_source_available(source, true).unwrap();
        scan(&lib, vec![]);
        assert_eq!(lib.playback_locators(id).unwrap(), vec![(remote, 0)]);
    }

    #[test]
    fn a_local_file_joins_the_server_track_it_copies() {
        let (lib, _path) = create_test_db();
        let source = server(&lib);
        lib.apply_remote_listing(source, &[remote_song("s1", "A")], &[])
            .unwrap();
        scan(&lib, vec![]);
        let remote_id = id_of(&lib, &remote::locator(source, "s1", "flac"));
        lib.set_liked(remote_id, true).unwrap();

        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/elsewhere/a.flac", "A")]);

        assert_eq!(paths(&lib), vec!["/music/elsewhere/a.flac".to_string()]);
        assert_eq!(id_of(&lib, "/music/elsewhere/a.flac"), remote_id);
        assert!(lib.liked_tracks().unwrap()[0].available);
    }

    #[test]
    fn a_song_dropped_from_the_server_listing_is_retired_but_keeps_its_like() {
        let (lib, path) = create_test_db();
        let source = server(&lib);
        lib.apply_remote_listing(
            source,
            &[
                remote_song("s1", "Keep"),
                remote_song("s2", "Gone"),
                remote_song("s3", "Plain"),
            ],
            &[],
        )
        .unwrap();
        scan(&lib, vec![]);
        let gone = id_of(&lib, &remote::locator(source, "s2", "flac"));
        lib.set_liked(gone, true).unwrap();

        let report = lib
            .apply_remote_listing(source, &[remote_song("s1", "Keep")], &[])
            .unwrap();
        assert_eq!(report.retired, 2);
        scan(&lib, vec![]);

        assert_eq!(paths(&lib), vec![remote::locator(source, "s1", "flac")]);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 2);
        assert!(!lib.liked_tracks().unwrap()[0].available);

        lib.apply_remote_listing(
            source,
            &[remote_song("s1", "Keep"), remote_song("s2", "Gone")],
            &[],
        )
        .unwrap();
        scan(&lib, vec![]);
        assert_eq!(id_of(&lib, &remote::locator(source, "s2", "flac")), gone);
    }

    #[test]
    fn an_empty_listing_changes_nothing_while_the_server_had_songs() {
        let (lib, _path) = create_test_db();
        let source = server(&lib);
        lib.apply_remote_listing(source, &[remote_song("s1", "A")], &[])
            .unwrap();
        assert!(lib.apply_remote_listing(source, &[], &[]).is_err());
        scan(&lib, vec![]);
        assert_eq!(paths(&lib), vec![remote::locator(source, "s1", "flac")]);
    }

    #[test]
    fn a_song_renamed_on_the_server_keeps_its_like() {
        let (lib, _path) = create_test_db();
        let source = server(&lib);
        lib.apply_remote_listing(source, &[remote_song("old-id", "A")], &[])
            .unwrap();
        scan(&lib, vec![]);
        let id = id_of(&lib, &remote::locator(source, "old-id", "flac"));
        lib.set_liked(id, true).unwrap();

        let report = lib
            .apply_remote_listing(source, &[remote_song("new-id", "A")], &[])
            .unwrap();
        assert_eq!((report.adopted, report.retired), (1, 1));
        scan(&lib, vec![]);
        assert_eq!(id_of(&lib, &remote::locator(source, "new-id", "flac")), id);
        assert!(lib.liked_tracks().unwrap()[0].available);
    }

    #[test]
    fn a_repeated_listing_reports_no_change() {
        let (lib, _path) = create_test_db();
        let source = server(&lib);
        let songs = [remote_song("s1", "A")];
        assert!(
            lib.apply_remote_listing(source, &songs, &[])
                .unwrap()
                .changed()
        );
        assert!(
            !lib.apply_remote_listing(source, &songs, &[])
                .unwrap()
                .changed()
        );
        let mut retagged = songs.clone();
        retagged[0].title = "A (Remastered)".into();
        assert!(
            lib.apply_remote_listing(source, &retagged, &[])
                .unwrap()
                .changed()
        );
    }

    #[test]
    fn a_server_song_does_not_claim_a_different_live_recording_by_title() {
        let (lib, path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/studio/a.flac", "A")]);
        let source = server(&lib);
        let mut live = remote_song("s1", "A");
        live.album = Some("Live at Somewhere".into());
        live.duration_ms = Some(181_000);
        live.size = Some(1);
        let report = lib.apply_remote_listing(source, &[live], &[]).unwrap();
        assert_eq!((report.added, report.adopted), (1, 0));
        scan(&lib, vec![scan_track("/music/studio/a.flac", "A")]);
        assert_eq!(paths(&lib).len(), 2);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM media_items"), 2);
    }

    #[test]
    fn server_covers_arrive_with_the_listing() {
        let (lib, _path) = create_test_db();
        let source = server(&lib);
        let jpeg = make_test_jpeg(&[1, 2, 3]);
        let thumbs = crate::thumbnail::generate_thumbnails(&jpeg).unwrap();
        let hash = sha256_hex(&jpeg);
        let mut song = remote_song("s1", "A");
        song.cover_hash = Some(hash.clone());
        lib.apply_remote_listing(
            source,
            &[song],
            &[RemoteCover {
                hash: hash.clone(),
                small: thumbs.small,
                large: thumbs.large,
                source_path: "subsonic-cover://1/c".into(),
            }],
        )
        .unwrap();
        scan(&lib, vec![]);
        lib.delete_orphaned_albums_and_artists().unwrap();
        assert!(lib.all_tracks().unwrap()[0].cover_art_id.is_some());
    }

    #[test]
    fn removing_the_server_hides_its_tracks_and_keeps_user_data() {
        let (lib, _path) = create_test_db();
        let source = server(&lib);
        lib.apply_remote_listing(source, &[remote_song("s1", "A")], &[])
            .unwrap();
        scan(&lib, vec![]);
        let id = id_of(&lib, &remote::locator(source, "s1", "flac"));
        let playlist = lib.create_playlist("Mix").unwrap();
        lib.add_track_to_playlist(playlist, id).unwrap();

        lib.reconcile_remote_sources("subsonic", &[]).unwrap();
        scan(&lib, vec![]);
        assert!(paths(&lib).is_empty());
        assert!(!lib.tracks_for_playlist(playlist).unwrap()[0].available);

        server(&lib);
        scan(&lib, vec![]);
        assert!(lib.tracks_for_playlist(playlist).unwrap()[0].available);
    }

    #[test]
    fn a_like_moves_to_its_copy_on_the_scan_where_the_original_disappears() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/a.flac", "A")]);
        let original = id_of(&lib, "/music/a.flac");
        lib.set_liked(original, true).unwrap();
        let copy = scan_track("/music/copy/a.flac", "A");
        scan(&lib, vec![scan_track("/music/a.flac", "A"), copy.clone()]);
        assert_ne!(id_of(&lib, "/music/copy/a.flac"), original);

        scan(&lib, vec![copy]);

        assert_eq!(id_of(&lib, "/music/copy/a.flac"), original);
        assert!(lib.liked_tracks().unwrap()[0].available);
    }

    #[test]
    fn a_deleted_file_is_adopted_by_its_retagged_replacement_only_within_tolerance() {
        let (lib, _path) = create_test_db();
        lib.reconcile_local_sources(&folders(&["/music"])).unwrap();
        scan(&lib, vec![scan_track("/music/a.flac", "A")]);
        let track_id = id_of(&lib, "/music/a.flac");
        lib.set_liked(track_id, true).unwrap();

        let mut other_length = scan_track("/music/a-live.flac", "A");
        other_length.album_title = Some("Live".into());
        other_length.duration_ms = Some(240_000);
        scan(&lib, vec![other_length]);

        assert_ne!(id_of(&lib, "/music/a-live.flac"), track_id);
        assert!(!lib.liked_tracks().unwrap()[0].available);
    }

    #[test]
    fn test_scan_session_buffers_track_until_cover_arrives() {
        // A track may reach the writer before its cover thumbnail; it must be
        // buffered and linked once add_cover lands.
        let (lib, _path) = create_test_db();
        let cover = make_test_jpeg(&[10, 20, 30]);
        let hash = sha256_hex(&cover);
        let thumbs = crate::thumbnail::generate_thumbnails(&cover).unwrap();

        let mut session = lib.open_scan_session().unwrap();
        session.clear().unwrap();
        session
            .add_track(ScanTrack {
                is_cue: false,
                path: "/music/a.flac".into(),
                title: Some("A".into()),
                album_title: Some("Album".into()),
                artist_names: vec!["Artist".into()],
                album_artist_names: vec!["Artist".into()],
                track_number: Some(1),
                disc_number: Some(1),
                year: Some(2020),
                genres: vec![],
                duration_ms: Some(1000),
                cover_hash: Some(hash.clone()),
                start_offset_ms: None,
                bitrate: None,
                lyrics: None,
                file_size: None,
            })
            .unwrap();
        session
            .add_cover(&hash, thumbs.small, thumbs.large, "/music/a.flac", true)
            .unwrap();
        session.finish().unwrap();
        // The album's cover and artists are settled after the scan, not during
        // it — see `resolve_album_covers` / `resolve_album_artists`.
        lib.resolve_album_covers().unwrap();
        lib.resolve_album_artists().unwrap();

        let albums = lib.albums().unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].title, "Album");
        assert_eq!(albums[0].artist_name, "Artist");
        assert!(albums[0].cover_art_id.is_some());
        let tracks = lib.tracks_for_album(albums[0].id).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "A");
        assert_eq!(tracks[0].cover_art_id, albums[0].cover_art_id);
        assert_eq!(
            lib.track_album_artists(tracks[0].id).unwrap(),
            vec!["Artist".to_string()]
        );
    }

    #[test]
    fn test_cover_art_source_roundtrip() {
        let (lib, _path) = create_test_db();
        let cover = make_test_jpeg(&[7, 8, 9]);
        let hash = sha256_hex(&cover);
        let thumbs = crate::thumbnail::generate_thumbnails(&cover).unwrap();

        let mut session = lib.open_scan_session().unwrap();
        session.clear().unwrap();
        session
            .add_cover(&hash, thumbs.small, thumbs.large, "/m/art/cover.jpg", false)
            .unwrap();
        session.finish().unwrap();

        let (_, id) = lib
            .cover_art_hashes()
            .unwrap()
            .into_iter()
            .find(|(h, _)| *h == hash)
            .unwrap();
        assert_eq!(
            lib.get_cover_art_source(id).unwrap(),
            Some(("/m/art/cover.jpg".to_string(), false))
        );
    }

    /// A cover saved outside a scan records its origin just like one saved during a
    /// scan, so the "open the original" paths reach the full-size image instead of
    /// falling back to re-reading the track. Rows left over from the migration that
    /// added these columns still have no source; that fallback lives in
    /// `music_indexer::metadata::load_cover_from_source`.
    #[test]
    fn test_save_cover_art_records_where_it_came_from() {
        let (lib, _path) = create_test_db();
        let embedded = lib
            .save_cover_art(&make_test_jpeg(&[4, 5, 6]), "/music/a.flac", true)
            .unwrap();
        let external = lib
            .save_cover_art(&make_test_jpeg(&[7, 8, 9]), "/music/cover.jpg", false)
            .unwrap();

        assert_eq!(
            lib.get_cover_art_source(embedded).unwrap(),
            Some(("/music/a.flac".to_string(), true))
        );
        assert_eq!(
            lib.get_cover_art_source(external).unwrap(),
            Some(("/music/cover.jpg".to_string(), false))
        );
    }

    #[test]
    fn test_add_cover_keeps_first_source_across_rescans() {
        let (lib, _path) = create_test_db();
        let cover = make_test_jpeg(&[11, 12, 13]);
        let hash = sha256_hex(&cover);
        let thumbs = crate::thumbnail::generate_thumbnails(&cover).unwrap();

        let mut session = lib.open_scan_session().unwrap();
        session.clear().unwrap();
        session
            .add_cover(
                &hash,
                thumbs.small.clone(),
                thumbs.large.clone(),
                "/first/cover.jpg",
                false,
            )
            .unwrap();
        session.finish().unwrap();

        let mut session = lib.open_scan_session().unwrap();
        session.clear().unwrap();
        session
            .add_cover(&hash, thumbs.small, thumbs.large, "/second/cover.jpg", true)
            .unwrap();
        session.finish().unwrap();

        let (_, id) = lib
            .cover_art_hashes()
            .unwrap()
            .into_iter()
            .find(|(h, _)| *h == hash)
            .unwrap();
        assert_eq!(
            lib.get_cover_art_source(id).unwrap(),
            Some(("/first/cover.jpg".to_string(), false))
        );
    }

    #[test]
    fn test_scan_session_reuses_existing_cover_without_add_cover() {
        // A cover already in the DB (survives clear) resolves by hash from the
        // seeded cache — no add_cover needed, no duplicate row.
        let (lib, _path) = create_test_db();
        let cover = make_test_jpeg(&[1, 2, 3]);
        let existing_id = lib.save_cover_art(&cover, "/music/a.flac", true).unwrap();
        let hash = sha256_hex(&cover);

        let mut session = lib.open_scan_session().unwrap();
        session.clear().unwrap();
        session
            .add_track(ScanTrack {
                is_cue: false,
                path: "/m/x.flac".into(),
                title: Some("X".into()),
                album_title: Some("Al".into()),
                artist_names: vec!["Ar".into()],
                album_artist_names: vec![],
                track_number: Some(1),
                disc_number: Some(1),
                year: None,
                genres: vec![],
                duration_ms: None,
                cover_hash: Some(hash),
                start_offset_ms: None,
                bitrate: None,
                lyrics: None,
                file_size: None,
            })
            .unwrap();
        session.finish().unwrap();

        let albums = lib.albums().unwrap();
        let tracks = lib.tracks_for_album(albums[0].id).unwrap();
        assert_eq!(tracks[0].cover_art_id, Some(existing_id));
    }

    #[test]
    fn test_scan_session_commits_across_batch_boundary() {
        // Exercise the COMMIT/BEGIN cycle: insert well over SCAN_BATCH_SIZE.
        let (lib, _path) = create_test_db();
        let mut session = lib.open_scan_session().unwrap();
        session.clear().unwrap();
        for i in 0..600 {
            session
                .add_track(ScanTrack {
                    is_cue: false,
                    path: format!("/m/{i}.flac"),
                    title: Some(format!("T{i}")),
                    album_title: Some("Big".into()),
                    artist_names: vec!["Ar".into()],
                    album_artist_names: vec!["Ar".into()],
                    track_number: Some(i as u32),
                    disc_number: Some(1),
                    year: Some(2020),
                    genres: vec![],
                    duration_ms: Some(1000),
                    cover_hash: None,
                    start_offset_ms: None,
                    bitrate: None,
                    lyrics: None,
                    file_size: None,
                })
                .unwrap();
        }
        session.finish().unwrap();

        let albums = lib.albums().unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(lib.tracks_for_album(albums[0].id).unwrap().len(), 600);
    }

    #[test]
    fn test_scan_session_writes_lyrics() {
        let (lib, _path) = create_test_db();
        let mut session = lib.open_scan_session().unwrap();
        session.clear().unwrap();
        session
            .add_track(ScanTrack {
                is_cue: false,
                path: "/m/lyric.flac".into(),
                title: Some("Lyric".into()),
                album_title: Some("Al".into()),
                artist_names: vec!["Ar".into()],
                album_artist_names: vec!["Ar".into()],
                track_number: Some(1),
                disc_number: Some(1),
                year: Some(2020),
                genres: vec![],
                duration_ms: Some(1000),
                cover_hash: None,
                start_offset_ms: None,
                bitrate: None,
                lyrics: Some(ScanLyrics {
                    text: "[00:01.00] hello\n[00:02.00] world".into(),
                    source: "lrclib".into(),
                }),
                file_size: None,
            })
            .unwrap();
        session.finish().unwrap();

        let tracks = lib.all_tracks().unwrap();
        assert_eq!(tracks.len(), 1);
        let stored = lib.lyrics_for_track(tracks[0].id).unwrap().unwrap();
        assert!(!stored.not_found);
        assert_eq!(stored.source, "lrclib");
        assert_eq!(stored.text, "[00:01.00] hello\n[00:02.00] world");
    }

    #[test]
    fn test_lyrics_cascade_on_clear() {
        let (lib, _path) = create_test_db();
        let mut session = lib.open_scan_session().unwrap();
        session.clear().unwrap();
        session
            .add_track(ScanTrack {
                is_cue: false,
                path: "/m/c.flac".into(),
                title: Some("C".into()),
                album_title: Some("Al".into()),
                artist_names: vec!["Ar".into()],
                album_artist_names: vec!["Ar".into()],
                track_number: Some(1),
                disc_number: Some(1),
                year: Some(2020),
                genres: vec![],
                duration_ms: Some(1000),
                cover_hash: None,
                start_offset_ms: None,
                bitrate: None,
                lyrics: Some(ScanLyrics {
                    text: "to be wiped".into(),
                    source: "embedded".into(),
                }),
                file_size: None,
            })
            .unwrap();
        session.finish().unwrap();

        let track_id = lib.all_tracks().unwrap()[0].id;
        assert!(lib.lyrics_for_track(track_id).unwrap().is_some());

        lib.clear().unwrap();
        assert!(lib.lyrics_for_track(track_id).unwrap().is_none());
    }

    #[test]
    fn test_scan_meta_roundtrip() {
        let (lib, _path) = create_test_db();
        assert!(lib.scan_fingerprint().unwrap().is_none());
        assert!(lib.scan_folders().unwrap().is_none());
        lib.set_scan_meta("fp123", "/a\n/b").unwrap();
        assert_eq!(lib.scan_fingerprint().unwrap(), Some("fp123".into()));
        assert_eq!(lib.scan_folders().unwrap(), Some("/a\n/b".into()));
        lib.set_scan_meta("fp456", "/c").unwrap();
        assert_eq!(lib.scan_fingerprint().unwrap(), Some("fp456".into()));
        assert_eq!(lib.scan_folders().unwrap(), Some("/c".into()));
    }

    #[test]
    fn test_artist_album_covers_oldest_first_capped_at_three() {
        let (lib, _path) = create_test_db();

        let cover1 = lib
            .save_cover_art(&make_test_jpeg(&[255, 0, 0]), "/music/a.flac", true)
            .unwrap();
        let cover2 = lib
            .save_cover_art(&make_test_jpeg(&[0, 255, 0]), "/music/a.flac", true)
            .unwrap();
        let cover3 = lib
            .save_cover_art(&make_test_jpeg(&[0, 0, 255]), "/music/a.flac", true)
            .unwrap();
        let cover4 = lib
            .save_cover_art(&make_test_jpeg(&[128, 128, 0]), "/music/a.flac", true)
            .unwrap();

        let artist = lib.upsert_artist("Radiohead").unwrap();
        let other = lib.upsert_artist("Other").unwrap();

        let mk_album = |title: &str, year: Option<i32>, cover: Option<i64>| {
            let id = lib.upsert_album(title, year, cover).unwrap();
            lib.set_album_artists(id, &[(artist, 0)]).unwrap();
            id
        };
        let album1990 = mk_album("Old Album", Some(1990), Some(cover1));
        let album2000 = mk_album("Mid Album", Some(2000), Some(cover2));
        let album2010 = mk_album("New Album", Some(2010), Some(cover3));
        let album2020 = mk_album("Newest Album", Some(2020), Some(cover4));

        let mk_track = |path: &str, _album_id: i64| NewTrack {
            path: path.into(),
            title: Some("T".into()),
            album_title: None,
            artist_names: vec![],
            track_number: Some(1),
            disc_number: Some(1),
            year: None,
            duration_ms: Some(120_000),
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        lib.upsert_track(
            &mk_track("/p/t1.flac", album1990),
            Some(album1990),
            &[(artist, 0)],
        )
        .unwrap();
        lib.upsert_track(
            &mk_track("/p/t2.flac", album2000),
            Some(album2000),
            &[(artist, 0)],
        )
        .unwrap();
        lib.upsert_track(
            &mk_track("/p/t3.flac", album2010),
            Some(album2010),
            &[(artist, 0)],
        )
        .unwrap();
        lib.upsert_track(
            &mk_track("/p/t4.flac", album2020),
            Some(album2020),
            &[(artist, 0)],
        )
        .unwrap();

        // Other artist — should not bleed into result.
        let other_album = lib
            .upsert_album("Other Album", Some(2005), Some(cover2))
            .unwrap();
        lib.set_album_artists(other_album, &[(other, 0)]).unwrap();
        lib.upsert_track(
            &mk_track("/p/t5.flac", other_album),
            Some(other_album),
            &[(other, 0)],
        )
        .unwrap();

        let covers = lib
            .artist_album_covers(ArtistGrouping::TrackArtist)
            .unwrap();

        let artist_covers = covers.get(&artist).unwrap();
        // At most 3, oldest-first: cover1 (1990), cover2 (2000), cover3 (2010).
        assert_eq!(artist_covers.len(), 3);
        assert_eq!(artist_covers[0], cover1);
        assert_eq!(artist_covers[1], cover2);
        assert_eq!(artist_covers[2], cover3);

        // Other artist is present separately.
        assert!(covers.contains_key(&other));
        // Artist without covers is absent.
        let no_cover_artist = lib.upsert_artist("Silent").unwrap();
        let bare_album = lib.upsert_album("Bare", Some(2000), None).unwrap();
        lib.upsert_track(
            &mk_track("/p/t6.flac", bare_album),
            Some(bare_album),
            &[(no_cover_artist, 0)],
        )
        .unwrap();
        let covers2 = lib
            .artist_album_covers(ArtistGrouping::TrackArtist)
            .unwrap();
        assert!(!covers2.contains_key(&no_cover_artist));
    }

    fn sorted_album_genres(lib: &SqliteLibrary, album_id: i64) -> Vec<String> {
        let mut genres = lib.album_genres(album_id).unwrap();
        genres.sort();
        genres
    }

    #[test]
    fn test_album_artists_in_position_order() {
        let (lib, _path) = create_test_db();
        let second = lib.upsert_artist("Bravo").unwrap();
        let first = lib.upsert_artist("Alpha").unwrap();
        let album_id = lib.upsert_album("Split", Some(2001), None).unwrap();
        lib.set_album_artists(album_id, &[(second, 1), (first, 0)])
            .unwrap();

        assert_eq!(
            lib.album_artists(album_id).unwrap(),
            vec!["Alpha".to_string(), "Bravo".to_string()]
        );
        assert!(lib.album_artists(99_999).unwrap().is_empty());
    }

    #[test]
    fn test_set_track_genres_replaces_links() {
        let (lib, _path) = create_test_db();
        let track_id = seed_track(&lib, "Song", "Album", "Artist");
        let album_id = lib.upsert_album("Album", Some(2020), None).unwrap();

        lib.set_track_genres(track_id, &["Ambient".into(), "Techno".into()])
            .unwrap();
        assert_eq!(
            sorted_album_genres(&lib, album_id),
            vec!["Ambient".to_string(), "Techno".to_string()]
        );

        lib.set_track_genres(track_id, &["Jazz".into()]).unwrap();
        assert_eq!(
            sorted_album_genres(&lib, album_id),
            vec!["Jazz".to_string()],
            "old genre links must be gone, not merged"
        );

        lib.set_track_genres(track_id, &[]).unwrap();
        assert!(sorted_album_genres(&lib, album_id).is_empty());
    }

    #[test]
    fn test_set_track_genres_dedups_case_insensitively() {
        let (lib, _path) = create_test_db();
        let track_id = seed_track(&lib, "Song", "Album", "Artist");
        let album_id = lib.upsert_album("Album", Some(2020), None).unwrap();

        lib.set_track_genres(
            track_id,
            &["Ambient".into(), "ambient".into(), "  Techno  ".into()],
        )
        .unwrap();

        assert_eq!(
            sorted_album_genres(&lib, album_id),
            vec!["Ambient".to_string(), "Techno".to_string()]
        );
    }

    #[test]
    fn test_set_track_genres_drops_orphaned_genres() {
        let (lib, _path) = create_test_db();
        let track_id = seed_track(&lib, "Song", "Album", "Artist");

        lib.set_track_genres(track_id, &["Ambient".into()]).unwrap();
        lib.set_track_genres(track_id, &["Techno".into()]).unwrap();

        let orphans = lib
            .album_genres_map()
            .unwrap()
            .into_values()
            .flatten()
            .filter(|g| g == "Ambient")
            .count();
        assert_eq!(orphans, 0);
    }

    #[test]
    fn test_upsert_track_preserves_id_liked_is_cue_and_cover() {
        let (lib, _path) = create_test_db();
        let jpeg = make_test_jpeg(&[10, 20, 30]);

        let mut session = lib.open_scan_session().unwrap();
        session.clear().unwrap();
        session
            .add_cover("cover-hash", jpeg.clone(), jpeg, "/m/cover.jpg", false)
            .unwrap();
        session
            .add_track(ScanTrack {
                is_cue: true,
                path: "/m/whole_album.flac".into(),
                title: Some("Cue Track".into()),
                album_title: Some("Cue Album".into()),
                artist_names: vec!["Old Artist".into()],
                album_artist_names: vec!["Old Artist".into()],
                track_number: Some(1),
                disc_number: Some(1),
                year: Some(1999),
                genres: vec!["Ambient".into()],
                duration_ms: Some(1000),
                cover_hash: Some("cover-hash".into()),
                start_offset_ms: Some(5000),
                bitrate: None,
                lyrics: None,
                file_size: None,
            })
            .unwrap();
        session.finish().unwrap();

        let before = lib.all_tracks().unwrap().remove(0);
        let cover_id = before.cover_art_id.expect("scan attached the cover");
        lib.set_liked(before.id, true).unwrap();

        let album_id = lib.upsert_album("Cue Album", Some(1999), None).unwrap();
        let new_artist = lib.upsert_artist("New Artist").unwrap();
        let updated = NewTrack {
            path: "/m/whole_album.flac".into(),
            title: Some("Renamed".into()),
            album_title: Some("Cue Album".into()),
            artist_names: vec!["New Artist".into()],
            track_number: Some(4),
            disc_number: Some(2),
            year: Some(2001),
            duration_ms: Some(1000),
            cover_art_id: None,
            start_offset_ms: Some(5000),
            bitrate: None,
        };
        let same_id = lib
            .upsert_track(&updated, Some(album_id), &[(new_artist, 0)])
            .unwrap();

        assert_eq!(same_id, before.id, "content key must keep the row id");

        let after = lib.track(before.id).unwrap().unwrap();
        assert_eq!(after.title, "Renamed");
        assert_eq!(after.track_number, Some(4));
        assert_eq!(after.disc_number, 2);
        assert_eq!(after.year, Some(2001));
        assert!(after.liked, "liked must survive a tag edit");
        assert!(after.is_cue, "is_cue must survive a tag edit");
        assert_eq!(
            after.cover_art_id,
            Some(cover_id),
            "a None cover must not wipe the existing one"
        );
        assert_eq!(
            lib.track_artists(before.id).unwrap(),
            vec!["New Artist".to_string()]
        );
    }

    #[test]
    fn test_set_track_genres_skips_blank_names() {
        let (lib, _path) = create_test_db();
        let track_id = seed_track(&lib, "Song", "Album", "Artist");
        let album_id = lib.upsert_album("Album", Some(2020), None).unwrap();

        lib.set_track_genres(
            track_id,
            &["".into(), "   ".into(), "Jazz".into(), "\t\n".into()],
        )
        .unwrap();

        assert_eq!(
            sorted_album_genres(&lib, album_id),
            vec!["Jazz".to_string()],
            "a blank name must not become a genre row"
        );
    }

    #[test]
    fn test_set_track_genres_leaves_other_tracks_alone() {
        let (lib, _path) = create_test_db();
        let first = seed_track(&lib, "One", "Album", "Artist");
        let second = seed_track(&lib, "Two", "Album", "Artist");
        let album_id = lib.upsert_album("Album", Some(2020), None).unwrap();

        lib.set_track_genres(first, &["Jazz".into()]).unwrap();
        lib.set_track_genres(second, &["Techno".into()]).unwrap();
        lib.set_track_genres(first, &["Rock".into()]).unwrap();

        assert_eq!(
            sorted_album_genres(&lib, album_id),
            vec!["Rock".to_string(), "Techno".to_string()],
            "replacing one track's genres must not touch its siblings"
        );
    }

    #[test]
    fn test_set_track_genres_keeps_a_genre_another_track_still_uses() {
        let (lib, _path) = create_test_db();
        let first = seed_track(&lib, "One", "Album", "Artist");
        let second = seed_track(&lib, "Two", "Album", "Artist");
        let album_id = lib.upsert_album("Album", Some(2020), None).unwrap();

        lib.set_track_genres(first, &["Ambient".into()]).unwrap();
        lib.set_track_genres(second, &["Ambient".into()]).unwrap();
        lib.set_track_genres(first, &["Techno".into()]).unwrap();

        assert_eq!(
            sorted_album_genres(&lib, album_id),
            vec!["Ambient".to_string(), "Techno".to_string()],
            "the orphan sweep must only take genres nothing links to"
        );
    }

    #[test]
    fn test_set_track_genres_reuses_an_existing_row_for_a_different_casing() {
        let (lib, _path) = create_test_db();
        let first = seed_track(&lib, "One", "Album", "Artist");
        let second = seed_track(&lib, "Two", "Album", "Artist");
        let album_id = lib.upsert_album("Album", Some(2020), None).unwrap();

        lib.set_track_genres(first, &["Ambient".into()]).unwrap();
        lib.set_track_genres(second, &["AMBIENT".into()]).unwrap();

        assert_eq!(
            sorted_album_genres(&lib, album_id),
            vec!["Ambient".to_string()],
            "genres resolve through the lowercase key, as a scan does"
        );
    }

    #[test]
    fn test_album_artists_survive_a_track_upsert() {
        let (lib, _path) = create_test_db();
        let credited = lib.upsert_artist("Album Artist").unwrap();
        let album_id = lib.upsert_album("Album", Some(2020), None).unwrap();
        lib.set_album_artists(album_id, &[(credited, 0)]).unwrap();

        let performer = lib.upsert_artist("Guest").unwrap();
        let track = NewTrack {
            path: "/music/one.flac".into(),
            title: Some("One".into()),
            album_title: Some("Album".into()),
            artist_names: vec!["Guest".into()],
            track_number: Some(1),
            disc_number: Some(1),
            year: Some(2020),
            duration_ms: Some(1000),
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        lib.upsert_track(&track, Some(album_id), &[(performer, 0)])
            .unwrap();

        assert_eq!(
            lib.album_artists(album_id).unwrap(),
            vec!["Album Artist".to_string()],
            "a per-track write must not rewrite the row its siblings share"
        );
    }

    fn insert_album_track(
        lib: &SqliteLibrary,
        path: &str,
        album_id: Option<i64>,
        track_number: Option<u32>,
        artist_ids: &[(i64, i32)],
    ) -> i64 {
        let track = NewTrack {
            path: path.into(),
            title: Some(path.into()),
            album_title: None,
            artist_names: Vec::new(),
            track_number,
            disc_number: Some(1),
            year: None,
            duration_ms: None,
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        lib.upsert_track(&track, album_id, artist_ids).unwrap()
    }

    fn count_of(artists: &[ArtistSummary], id: i64) -> Option<i64> {
        artists.iter().find(|a| a.id == id).map(|a| a.track_count)
    }

    #[test]
    fn album_artist_grouping_uses_the_tag_and_falls_back_to_the_track_artist() {
        let (lib, _path) = create_test_db();
        let band = lib.upsert_artist("Band").unwrap();
        let guest = lib.upsert_artist("Guest").unwrap();
        let album = lib.upsert_album("Split", Some(2020), None).unwrap();
        let tagged = insert_album_track(&lib, "/m/01.flac", Some(album), Some(1), &[(guest, 0)]);
        let untagged = insert_album_track(&lib, "/m/02.flac", Some(album), Some(2), &[(guest, 0)]);
        lib.set_track_album_artists(tagged, &[(band, 0)]).unwrap();

        let by_album = lib.artists(ArtistGrouping::AlbumArtist).unwrap();
        assert_eq!(count_of(&by_album, band), Some(1));
        assert_eq!(
            count_of(&by_album, guest),
            Some(2),
            "listed through the untagged track, the page then carries every credit"
        );

        let by_track = lib.artists(ArtistGrouping::TrackArtist).unwrap();
        assert_eq!(count_of(&by_track, band), None);
        assert_eq!(count_of(&by_track, guest), Some(2));

        let ids = |tracks: Vec<Track>| tracks.into_iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(
            ids(lib
                .tracks_by_artist(band, ArtistGrouping::AlbumArtist)
                .unwrap()),
            vec![tagged]
        );
        assert_eq!(
            ids(lib
                .tracks_by_artist(guest, ArtistGrouping::AlbumArtist)
                .unwrap()),
            vec![tagged, untagged]
        );
        assert_eq!(
            ids(lib
                .tracks_by_artist(guest, ArtistGrouping::TrackArtist)
                .unwrap()),
            vec![tagged, untagged]
        );
        assert_eq!(
            lib.track_album_artists(tagged).unwrap(),
            vec!["Band".to_string()]
        );
        assert!(lib.track_album_artists(untagged).unwrap().is_empty());
    }

    #[test]
    fn artist_summary_follows_the_grouping() {
        let (lib, _path) = create_test_db();
        let band = lib.upsert_artist("Band").unwrap();
        let guest = lib.upsert_artist("Guest").unwrap();
        let album = lib.upsert_album("Split", Some(2020), None).unwrap();
        let tagged = insert_album_track(&lib, "/m/01.flac", Some(album), Some(1), &[(guest, 0)]);
        lib.set_track_album_artists(tagged, &[(band, 0)]).unwrap();

        let summary = lib
            .artist_summary(band, ArtistGrouping::AlbumArtist)
            .unwrap()
            .unwrap();
        assert_eq!((summary.name.as_str(), summary.track_count), ("Band", 1));
        assert!(
            lib.artist_summary(band, ArtistGrouping::TrackArtist)
                .unwrap()
                .is_none()
        );
        assert!(
            lib.artist_summary(guest, ArtistGrouping::AlbumArtist)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            lib.artist_summary(guest, ArtistGrouping::TrackArtist)
                .unwrap()
                .map(|a| a.track_count),
            Some(1)
        );
        assert!(
            lib.artist_summary(NO_METADATA_ARTIST_ID, ArtistGrouping::AlbumArtist)
                .unwrap()
                .is_none()
        );

        insert_album_track(&lib, "/m/loose.flac", None, None, &[]);
        for grouping in [ArtistGrouping::TrackArtist, ArtistGrouping::AlbumArtist] {
            let orphan = lib
                .artist_summary(NO_METADATA_ARTIST_ID, grouping)
                .unwrap()
                .unwrap();
            assert_eq!((orphan.id, orphan.track_count), (NO_METADATA_ARTIST_ID, 1));
            assert_eq!(
                count_of(&lib.artists(grouping).unwrap(), NO_METADATA_ARTIST_ID),
                Some(1)
            );
        }
    }

    #[test]
    fn resolve_album_artists_ignores_insertion_order() {
        let (lib, _path) = create_test_db();
        let alpha = lib.upsert_artist("Alpha").unwrap();
        let zed = lib.upsert_artist("Zed").unwrap();
        let compiler = lib.upsert_artist("Compiler").unwrap();

        let comp = lib.upsert_album("Comp", Some(2021), None).unwrap();
        let second = insert_album_track(&lib, "/c/02.flac", Some(comp), Some(2), &[(zed, 0)]);
        lib.set_track_album_artists(second, &[(compiler, 0)])
            .unwrap();
        insert_album_track(&lib, "/c/01.flac", Some(comp), Some(1), &[(alpha, 0)]);
        lib.set_album_artists(comp, &[(zed, 0)]).unwrap();

        let plain = lib.upsert_album("Plain", Some(2022), None).unwrap();
        insert_album_track(&lib, "/p/b.flac", Some(plain), Some(2), &[(zed, 0)]);
        insert_album_track(&lib, "/p/a.flac", Some(plain), Some(1), &[(alpha, 0)]);

        lib.resolve_album_artists().unwrap();
        assert_eq!(
            lib.album_artists(comp).unwrap(),
            vec!["Compiler".to_string()]
        );
        assert_eq!(lib.album_artists(plain).unwrap(), vec!["Alpha".to_string()]);

        lib.resolve_album_artists().unwrap();
        assert_eq!(
            lib.album_artists(comp).unwrap(),
            vec!["Compiler".to_string()]
        );
        assert_eq!(lib.album_artists(plain).unwrap(), vec!["Alpha".to_string()]);
    }

    #[test]
    fn orphan_cleanup_keeps_an_artist_only_named_as_a_track_album_artist() {
        let (lib, _path) = create_test_db();
        let alpha = lib.upsert_artist("Alpha").unwrap();
        let only_tag = lib.upsert_artist("Only In Tag").unwrap();
        let album = lib.upsert_album("Comp", Some(2021), None).unwrap();
        insert_album_track(&lib, "/c/01.flac", Some(album), Some(1), &[(alpha, 0)]);
        let second = insert_album_track(&lib, "/c/02.flac", Some(album), Some(2), &[(alpha, 0)]);
        lib.set_track_album_artists(second, &[(only_tag, 0)])
            .unwrap();

        lib.resolve_album_artists().unwrap();
        lib.delete_orphaned_albums_and_artists().unwrap();

        assert_eq!(
            lib.album_artists(album).unwrap(),
            vec!["Only In Tag".to_string()]
        );
        assert_eq!(
            lib.track_album_artists(second).unwrap(),
            vec!["Only In Tag".to_string()]
        );
    }

    #[test]
    fn an_album_without_tags_is_credited_to_the_shared_primary_artist() {
        let (lib, _path) = create_test_db();
        let band = lib.upsert_artist("Band").unwrap();
        let featuring = lib.upsert_artist("Band Feat. Guest").unwrap();
        let album = lib.upsert_album("Record", Some(2020), None).unwrap();
        insert_album_track(&lib, "/r/01.flac", Some(album), Some(1), &[(band, 0)]);
        insert_album_track(&lib, "/r/02.flac", Some(album), Some(2), &[(featuring, 0)]);
        insert_album_track(&lib, "/r/03.flac", Some(album), Some(3), &[]);

        lib.resolve_album_artists().unwrap();
        assert_eq!(lib.album_artists(album).unwrap(), vec!["Band".to_string()]);
        assert!(lib.album_artist_known(album).unwrap());

        let by_album = lib.artists(ArtistGrouping::AlbumArtist).unwrap();
        assert_eq!(count_of(&by_album, band), Some(3));
        assert_eq!(count_of(&by_album, featuring), None);
        assert_eq!(count_of(&by_album, NO_METADATA_ARTIST_ID), None);
        assert_eq!(
            lib.tracks_by_artist(band, ArtistGrouping::AlbumArtist)
                .unwrap()
                .len(),
            3
        );

        let by_track = lib.artists(ArtistGrouping::TrackArtist).unwrap();
        assert_eq!(count_of(&by_track, band), Some(1));
        assert_eq!(count_of(&by_track, featuring), Some(1));
        assert_eq!(count_of(&by_track, NO_METADATA_ARTIST_ID), Some(1));
    }

    #[test]
    fn a_mixed_album_without_tags_keeps_each_track_under_its_own_artist() {
        let (lib, _path) = create_test_db();
        let alpha = lib.upsert_artist("Alpha").unwrap();
        let zed = lib.upsert_artist("Zed").unwrap();
        let album = lib.upsert_album("Comp", Some(2020), None).unwrap();
        insert_album_track(&lib, "/c/01.flac", Some(album), Some(1), &[(alpha, 0)]);
        insert_album_track(&lib, "/c/02.flac", Some(album), Some(2), &[(zed, 0)]);

        lib.resolve_album_artists().unwrap();
        assert_eq!(lib.album_artists(album).unwrap(), vec!["Alpha".to_string()]);
        assert!(!lib.album_artist_known(album).unwrap());

        let by_album = lib.artists(ArtistGrouping::AlbumArtist).unwrap();
        assert_eq!(count_of(&by_album, alpha), Some(1));
        assert_eq!(count_of(&by_album, zed), Some(1));
    }

    #[test]
    fn a_partly_tagged_album_pulls_its_untagged_tracks_under_the_tag() {
        let (lib, _path) = create_test_db();
        let band = lib.upsert_artist("Band").unwrap();
        let guest = lib.upsert_artist("Guest").unwrap();
        let album = lib.upsert_album("Split", Some(2020), None).unwrap();
        let tagged = insert_album_track(&lib, "/m/01.flac", Some(album), Some(1), &[(guest, 0)]);
        insert_album_track(&lib, "/m/02.flac", Some(album), Some(2), &[(guest, 0)]);
        lib.set_track_album_artists(tagged, &[(band, 0)]).unwrap();

        lib.resolve_album_artists().unwrap();
        assert!(lib.album_artist_known(album).unwrap());
        let by_album = lib.artists(ArtistGrouping::AlbumArtist).unwrap();
        assert_eq!(count_of(&by_album, band), Some(2));
        assert_eq!(count_of(&by_album, guest), None);
    }

    #[test]
    fn a_listed_artists_page_also_holds_the_tracks_they_are_only_credited_on() {
        let (lib, _path) = create_test_db();
        let mick = lib.upsert_artist("Mick Gordon").unwrap();
        let other = lib.upsert_artist("Other").unwrap();
        let various = lib.upsert_artist("Various Artists").unwrap();
        let solo = lib.upsert_album("Old Blood", Some(2015), None).unwrap();
        let comp = lib.upsert_album("New Colossus", Some(2017), None).unwrap();
        let solo_track = insert_album_track(&lib, "/s/01.flac", Some(solo), Some(1), &[(mick, 0)]);
        let credited = insert_album_track(&lib, "/c/01.flac", Some(comp), Some(1), &[(mick, 0)]);
        let others = insert_album_track(&lib, "/c/02.flac", Some(comp), Some(2), &[(other, 0)]);
        lib.set_track_album_artists(credited, &[(various, 0)])
            .unwrap();
        lib.set_track_album_artists(others, &[(various, 0)])
            .unwrap();
        lib.resolve_album_artists().unwrap();

        let listed = lib.artists(ArtistGrouping::AlbumArtist).unwrap();
        assert_eq!(count_of(&listed, various), Some(2));
        assert_eq!(
            count_of(&listed, mick),
            Some(2),
            "his own album plus the credit"
        );
        assert_eq!(
            count_of(&listed, other),
            None,
            "credits alone do not list an artist"
        );

        let ids = |tracks: Vec<Track>| tracks.into_iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(
            ids(lib
                .tracks_by_artist(mick, ArtistGrouping::AlbumArtist)
                .unwrap()),
            vec![solo_track, credited]
        );
        assert_eq!(
            lib.artist_summary(mick, ArtistGrouping::AlbumArtist)
                .unwrap()
                .map(|a| a.track_count),
            Some(2)
        );
        assert!(
            lib.artist_summary(other, ArtistGrouping::AlbumArtist)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            lib.artist_summary(other, ArtistGrouping::TrackArtist)
                .unwrap()
                .map(|a| a.track_count),
            Some(1)
        );

        let haystacks = lib
            .artist_search_haystacks(ArtistGrouping::AlbumArtist)
            .unwrap();
        let various_hay = &haystacks[&various];
        assert!(various_hay.starts_with("Various Artists"));
        assert!(various_hay.contains("Mick Gordon") && various_hay.contains("Other"));
        assert_eq!(haystacks[&mick].trim(), "Mick Gordon");
        assert!(
            !haystacks.contains_key(&other),
            "only listed artists are searchable"
        );
    }

    #[test]
    fn set_album_artists_marks_the_albums_artist_known() {
        let (lib, _path) = create_test_db();
        let band = lib.upsert_artist("Band").unwrap();
        let album = lib.upsert_album("Record", Some(2020), None).unwrap();
        assert!(!lib.album_artist_known(album).unwrap());

        lib.set_album_artists(album, &[(band, 0)]).unwrap();
        assert!(lib.album_artist_known(album).unwrap());

        lib.set_album_artists(album, &[]).unwrap();
        assert!(
            !lib.album_artist_known(album).unwrap(),
            "clearing the credits clears the flag, so the two can never disagree"
        );
    }

    #[test]
    fn avatar_covers_skip_artists_the_grouping_does_not_list() {
        let (lib, _path) = create_test_db();
        let cover = {
            let img = image::RgbImage::from_pixel(4, 4, image::Rgb([9, 9, 9]));
            let mut buf = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(img)
                .write_to(&mut buf, image::ImageFormat::Jpeg)
                .unwrap();
            lib.save_cover_art(&buf.into_inner(), "/c/01.flac", true)
                .unwrap()
        };
        let headliner = lib.upsert_artist("Headliner").unwrap();
        let guest = lib.upsert_artist("Guest Only").unwrap();
        let various = lib.upsert_artist("Various Artists").unwrap();
        let comp = lib.upsert_album("Comp", Some(2020), Some(cover)).unwrap();
        let own = lib.upsert_album("Solo", Some(2021), Some(cover)).unwrap();
        let on_comp = insert_album_track(&lib, "/c/01.flac", Some(comp), Some(1), &[(guest, 0)]);
        lib.set_track_album_artists(on_comp, &[(various, 0)])
            .unwrap();
        let solo = insert_album_track(&lib, "/s/01.flac", Some(own), Some(1), &[(headliner, 0)]);
        lib.set_track_album_artists(solo, &[(headliner, 0)])
            .unwrap();
        lib.resolve_album_artists().unwrap();

        let covers = lib
            .artist_album_covers(ArtistGrouping::AlbumArtist)
            .unwrap();
        assert!(covers.contains_key(&various));
        assert!(covers.contains_key(&headliner));
        assert!(
            !covers.contains_key(&guest),
            "an artist the tab does not list never has an avatar to draw"
        );

        let by_track = lib
            .artist_album_covers(ArtistGrouping::TrackArtist)
            .unwrap();
        assert!(
            by_track.contains_key(&guest),
            "the other mode does list them"
        );
    }

    #[test]
    fn track_artist_grouping_searches_names_only() {
        let (lib, _path) = create_test_db();
        let band = lib.upsert_artist("Band").unwrap();
        let guest = lib.upsert_artist("Guest").unwrap();
        let album = lib.upsert_album("Record", Some(2020), None).unwrap();
        let shared = insert_album_track(
            &lib,
            "/r/01.flac",
            Some(album),
            Some(1),
            &[(band, 0), (guest, 1)],
        );
        lib.set_track_album_artists(shared, &[(band, 0)]).unwrap();
        lib.resolve_album_artists().unwrap();

        let by_track = lib
            .artist_search_haystacks(ArtistGrouping::TrackArtist)
            .unwrap();
        assert_eq!(by_track[&band].trim(), "Band");
        assert_eq!(by_track[&guest].trim(), "Guest");

        let by_album = lib
            .artist_search_haystacks(ArtistGrouping::AlbumArtist)
            .unwrap();
        assert!(
            by_album[&band].contains("Guest"),
            "searching for a guest must surface the artist whose page holds them"
        );
        assert!(!by_album.contains_key(&guest));
    }

    fn a_play_at(qualified: bool, started_at: u64) -> models::NewPlay {
        models::NewPlay {
            started_at,
            ..a_play(qualified)
        }
    }

    fn a_play(qualified: bool) -> models::NewPlay {
        models::NewPlay {
            track_id: None,
            artist: "Tool".into(),
            title: "Pneuma".into(),
            album: Some("Fear Inoculum".into()),
            album_artist: Some("Tool".into()),
            track_number: Some(2),
            duration_secs: Some(713),
            played_secs: Some(400),
            started_at: 1_700_000_000,
            qualified,
        }
    }

    fn a_love() -> models::NewLove {
        models::NewLove {
            track_id: None,
            artist: "Tool".into(),
            title: "Pneuma".into(),
            loved: true,
            at: 1_700_000_500,
        }
    }

    fn count_rows(db_path: &PathBuf, sql: &str) -> i64 {
        let conn = rusqlite::Connection::open(db_path).unwrap();
        conn.query_row(sql, [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn a_play_is_recorded_even_with_no_target_configured() {
        let (lib, path) = create_test_db();
        lib.record_play(&a_play(true), &[]).unwrap();
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM plays"), 1);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM play_deliveries"), 0);
        assert_eq!(lib.pending_scrobble_count(&["lastfm"]).unwrap(), 0);
    }

    #[test]
    fn an_unqualified_play_is_history_only() {
        let (lib, path) = create_test_db();
        lib.record_play(&a_play(false), &["lastfm", "csv_log"])
            .unwrap();
        assert_eq!(
            count_rows(&path, "SELECT COUNT(*) FROM plays WHERE qualified = 0"),
            1
        );
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM play_deliveries"), 0);
    }

    #[test]
    fn settling_one_target_keeps_the_play_pending_for_the_others() {
        let (lib, _path) = create_test_db();
        let id = lib
            .record_play(&a_play(true), &["lastfm", "listen_brainz"])
            .unwrap();

        lib.settle_plays(&[id], "lastfm", &models::DeliveryOutcome::Sent)
            .unwrap();

        assert!(lib.pending_plays("lastfm", 10).unwrap().is_empty());
        assert_eq!(lib.pending_plays("listen_brainz", 10).unwrap().len(), 1);
        assert_eq!(
            lib.pending_scrobble_count(&["lastfm", "listen_brainz"])
                .unwrap(),
            1
        );
    }

    #[test]
    fn pending_count_counts_an_item_once_and_ignores_unlisted_targets() {
        let (lib, _path) = create_test_db();
        lib.record_play(&a_play_at(true, 1), &["lastfm", "listen_brainz"])
            .unwrap();
        lib.record_play(&a_play_at(true, 2), &["csv_log"]).unwrap();

        assert_eq!(
            lib.pending_scrobble_count(&["lastfm", "listen_brainz"])
                .unwrap(),
            1
        );
        assert_eq!(lib.pending_scrobble_count(&["csv_log"]).unwrap(), 1);
        assert_eq!(
            lib.pending_scrobble_count(&["lastfm", "csv_log"]).unwrap(),
            2
        );
        assert_eq!(lib.pending_scrobble_count(&[]).unwrap(), 0);
    }

    #[test]
    fn a_deferred_delivery_stays_pending_and_keeps_its_error() {
        let (lib, path) = create_test_db();
        let id = lib.record_play(&a_play(true), &["listen_brainz"]).unwrap();

        lib.settle_plays(
            &[id],
            "listen_brainz",
            &models::DeliveryOutcome::Deferred("unverified email".into()),
        )
        .unwrap();

        assert_eq!(lib.pending_plays("listen_brainz", 10).unwrap().len(), 1);
        assert_eq!(
            count_rows(
                &path,
                "SELECT attempts FROM play_deliveries WHERE last_error = 'unverified email'"
            ),
            1
        );
    }

    #[test]
    fn a_dropped_delivery_leaves_the_queue_but_not_the_history() {
        let (lib, path) = create_test_db();
        let id = lib.record_play(&a_play(true), &["lastfm"]).unwrap();

        lib.settle_plays(
            &[id],
            "lastfm",
            &models::DeliveryOutcome::Dropped("bad params".into()),
        )
        .unwrap();

        assert!(lib.pending_plays("lastfm", 10).unwrap().is_empty());
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM plays"), 1);
        assert_eq!(
            count_rows(&path, "SELECT state FROM play_deliveries"),
            models::delivery_state::DROPPED
        );
    }

    #[test]
    fn loves_round_trip_with_their_own_timestamp() {
        let (lib, _path) = create_test_db();
        let id = lib.record_love(&a_love(), &["lastfm"]).unwrap();

        let pending = lib.pending_loves("lastfm", 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id);
        assert!(pending[0].loved);
        assert_eq!(pending[0].at, 1_700_000_500);
        assert_eq!(lib.pending_scrobble_count(&["lastfm"]).unwrap(), 1);

        lib.settle_loves(&[id], "lastfm", &models::DeliveryOutcome::Sent)
            .unwrap();
        assert_eq!(lib.pending_scrobble_count(&["lastfm"]).unwrap(), 0);
    }

    #[test]
    fn trimming_drops_the_oldest_deliveries_but_never_the_history() {
        let (lib, path) = create_test_db();
        for n in 0..5 {
            lib.record_play(&a_play_at(true, n), &["lastfm"]).unwrap();
        }

        let dropped = lib.trim_pending_deliveries(2).unwrap();

        assert_eq!(dropped, 3);
        assert_eq!(lib.pending_plays("lastfm", 10).unwrap().len(), 2);
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM plays"), 5);
        assert_eq!(
            count_rows(
                &path,
                "SELECT COUNT(*) FROM play_deliveries WHERE last_error = 'queue overflow'"
            ),
            3
        );
    }

    #[test]
    fn trimming_under_the_cap_changes_nothing() {
        let (lib, _path) = create_test_db();
        lib.record_play(&a_play(true), &["lastfm"]).unwrap();
        assert_eq!(lib.trim_pending_deliveries(5000).unwrap(), 0);
        assert_eq!(lib.pending_plays("lastfm", 10).unwrap().len(), 1);
    }

    #[test]
    fn pending_plays_come_back_oldest_first() {
        let (lib, _path) = create_test_db();
        for n in 0..3u64 {
            let mut play = a_play(true);
            play.started_at = 1_700_000_000 + n;
            lib.record_play(&play, &["lastfm"]).unwrap();
        }
        let pending = lib.pending_plays("lastfm", 2).unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].started_at, 1_700_000_000);
        assert_eq!(pending[1].started_at, 1_700_000_001);
    }

    #[test]
    fn history_survives_a_track_disappearing_from_the_library() {
        let (lib, path) = create_test_db();
        let artist = lib.upsert_artist("Tool").unwrap();
        let new_track = NewTrack {
            path: "/m/pneuma.flac".into(),
            title: Some("Pneuma".into()),
            album_title: None,
            artist_names: vec!["Tool".into()],
            track_number: Some(2),
            disc_number: Some(1),
            year: None,
            duration_ms: Some(713_000),
            cover_art_id: None,
            start_offset_ms: None,
            bitrate: None,
        };
        let track = lib.upsert_track(&new_track, None, &[(artist, 0)]).unwrap();
        let mut play = a_play(true);
        play.track_id = Some(track);
        lib.record_play(&play, &[]).unwrap();

        lib.clear().unwrap();

        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM plays"), 1);
        assert_eq!(
            count_rows(
                &path,
                &format!("SELECT COUNT(*) FROM plays WHERE track_id = {track} AND artist = 'Tool'")
            ),
            1
        );
    }

    #[test]
    fn the_cap_counts_items_not_delivery_rows() {
        let (lib, path) = create_test_db();
        let targets = ["lastfm", "listen_brainz", "csv_log"];
        for n in 0..5 {
            lib.record_play(&a_play_at(true, n), &targets).unwrap();
        }
        assert_eq!(
            count_rows(&path, "SELECT COUNT(*) FROM play_deliveries"),
            15
        );

        let dropped = lib.trim_pending_deliveries(4).unwrap();

        assert_eq!(dropped, 1, "one play over the cap of four");
        assert_eq!(
            lib.pending_scrobble_count(&targets).unwrap(),
            4,
            "the cap must mean plays, the same unit the badge reports"
        );
        assert_eq!(
            count_rows(
                &path,
                "SELECT COUNT(*) FROM play_deliveries WHERE state = 0"
            ),
            12,
            "all three delivery rows of the dropped play go together"
        );
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM plays"), 5);
    }

    #[test]
    fn recording_the_same_listen_twice_merges_and_never_shrinks_it() {
        let (lib, path) = create_test_db();
        let mut early = a_play(true);
        early.played_secs = Some(200);
        let first = lib.record_play(&early, &["lastfm"]).unwrap();

        let mut total = a_play(true);
        total.played_secs = Some(500);
        let second = lib.record_play(&total, &["lastfm"]).unwrap();

        assert_eq!(first, second, "one listen is one row");
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM plays"), 1);
        assert_eq!(count_rows(&path, "SELECT played_secs FROM plays"), 500);
        assert_eq!(
            count_rows(&path, "SELECT COUNT(*) FROM play_deliveries"),
            1,
            "and it is owed to last.fm once, not twice"
        );

        let mut shorter = a_play(true);
        shorter.played_secs = Some(120);
        lib.record_play(&shorter, &["lastfm"]).unwrap();
        assert_eq!(
            count_rows(&path, "SELECT played_secs FROM plays"),
            500,
            "a late commit must never shrink what was already recorded"
        );
    }

    #[test]
    fn an_unqualified_listen_can_be_upgraded_when_it_later_qualifies() {
        let (lib, path) = create_test_db();
        lib.record_play(&a_play(false), &["lastfm"]).unwrap();
        assert_eq!(count_rows(&path, "SELECT COUNT(*) FROM play_deliveries"), 0);

        lib.record_play(&a_play(true), &["lastfm"]).unwrap();

        assert_eq!(count_rows(&path, "SELECT qualified FROM plays"), 1);
        assert_eq!(lib.pending_scrobble_count(&["lastfm"]).unwrap(), 1);
    }

    #[test]
    fn a_settled_delivery_is_not_reopened_by_a_late_settle() {
        let (lib, path) = create_test_db();
        let id = lib.record_play(&a_play(true), &["lastfm"]).unwrap();
        lib.settle_plays(&[id], "lastfm", &models::DeliveryOutcome::Sent)
            .unwrap();

        lib.settle_plays(
            &[id],
            "lastfm",
            &models::DeliveryOutcome::Dropped("late".into()),
        )
        .unwrap();

        assert_eq!(
            count_rows(&path, "SELECT state FROM play_deliveries"),
            models::delivery_state::SENT
        );
        assert_eq!(count_rows(&path, "SELECT attempts FROM play_deliveries"), 1);
    }

    #[test]
    fn both_history_tables_have_an_index_for_their_foreign_key() {
        let (_lib, path) = create_test_db();
        let conn = rusqlite::Connection::open(&path).unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name = ?1")
            .unwrap();
        for index in ["idx_plays_track", "idx_loves_track"] {
            let found = stmt.exists([index]).unwrap();
            assert!(
                found,
                "{index} is missing: a rescan deletes every track, and without it SQLite \
                 full-scans the history table once per deleted row"
            );
        }
    }
}
