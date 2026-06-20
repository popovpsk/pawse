pub mod error;
pub mod migrations;
pub mod models;
pub mod repository;
pub mod sqlite;
pub mod thumbnail;

pub use error::{LibraryError, Result};
pub use models::{
    Album, AlbumSearchEntry, AlbumSummary, Artist, ArtistSummary, CoverArt, NewTrack, Playlist,
    PlaylistSummary, PlaylistTrackRef, ScanLyrics, ScanTrack, StoredLyrics, Track,
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
            album_artist_names: Vec::new(),
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
                album_artist_names: Vec::new(),
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
                album_artist_names: Vec::new(),
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
            album_artist_names: Vec::new(),
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
            !lib.artists()
                .unwrap()
                .iter()
                .any(|a| a.id == NO_METADATA_ARTIST_ID)
        );

        let bare = NewTrack {
            path: "/loose/track.flac".into(),
            title: Some("track".into()),
            album_title: None,
            artist_names: Vec::new(),
            album_artist_names: Vec::new(),
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

        let artists = lib.artists().unwrap();
        let no_meta_artist = artists.iter().find(|a| a.id == NO_METADATA_ARTIST_ID);
        assert_eq!(no_meta_artist.map(|a| a.track_count), Some(1));
        let artist_tracks = lib.tracks_by_artist(NO_METADATA_ARTIST_ID).unwrap();
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
                album_artist_names: Vec::new(),
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
            album_artist_names: Vec::new(),
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
            album_artist_names: Vec::new(),
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
            album_artist_names: vec!["Album Artist".into()],
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
            album_artist_names: vec!["Album Artist".into()],
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
            album_artist_names: Vec::new(),
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
    fn test_album_has_artists_false() {
        let (lib, _path) = create_test_db();
        let album_id = lib.upsert_album("Solo Album", None, None).unwrap();
        assert!(!lib.album_has_artists(album_id).unwrap());
    }

    #[test]
    fn test_delete_orphaned_albums_and_artists() {
        let (lib, _path) = create_test_db();
        let artist_id = lib.upsert_artist("Orphan Artist").unwrap();
        let album_id = lib.upsert_album("Orphan Album", None, None).unwrap();
        lib.set_album_artists(album_id, &[(artist_id, 0)]).unwrap();

        lib.delete_orphaned_albums_and_artists().unwrap();

        assert!(lib.album_title(album_id).unwrap().is_none());
        assert!(!lib.album_has_artists(album_id).unwrap());
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

        let id = lib.save_cover_art(&data).unwrap();
        assert!(id > 0);

        let id2 = lib.save_cover_art(&data).unwrap();
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
            album_artist_names: Vec::new(),
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
            album_artist_names: Vec::new(),
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
            album_artist_names: Vec::new(),
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
            album_artist_names: Vec::new(),
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
            album_artist_names: Vec::new(),
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

        let id1 = lib.save_cover_art(&data1).unwrap();
        let id2 = lib.save_cover_art(&data2).unwrap();
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
        let id = lib.save_cover_art(&data).unwrap();
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
        let cover_id = lib.save_cover_art(&data).unwrap();

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
            album_artist_names: Vec::new(),
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

    #[test]
    fn test_cover_art_deduplication_across_albums() {
        let (lib, _path) = create_test_db();
        let data = make_test_jpeg(&[255, 0, 0]);

        let cover_id = lib.save_cover_art(&data).unwrap();
        let cover_id2 = lib.save_cover_art(&data).unwrap();
        assert_eq!(cover_id, cover_id2, "same bytes must return same ID");

        let artist_id = lib.upsert_artist("Artist").unwrap();
        let album1 = lib.upsert_album("Album 1", None, Some(cover_id)).unwrap();
        let album2 = lib.upsert_album("Album 2", None, Some(cover_id)).unwrap();

        let track1 = NewTrack {
            path: "/music/track1.flac".into(),
            title: Some("Track 1".into()),
            album_title: Some("Album 1".into()),
            artist_names: vec!["Artist".into()],
            album_artist_names: Vec::new(),
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
            album_artist_names: Vec::new(),
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
        let cover_id = lib.save_cover_art(&data).unwrap();

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
        let cover_art_id = lib.save_cover_art(&cover_data).unwrap();

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
            album_artist_names: Vec::new(),
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
            album_artist_names: vec![artist.into()],
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
    fn test_artists_enumerates_with_track_counts() {
        let (lib, _path) = create_test_db();
        seed_track(&lib, "Song A", "Album X", "Artist Alpha");
        seed_track(&lib, "Song B", "Album X", "Artist Alpha");
        seed_track(&lib, "Song C", "Album Y", "Artist Beta");

        let artists = lib.artists().unwrap();
        let alpha = artists.iter().find(|a| a.name == "Artist Alpha").unwrap();
        let beta = artists.iter().find(|a| a.name == "Artist Beta").unwrap();
        assert_eq!(alpha.track_count, 2);
        assert_eq!(beta.track_count, 1);
        // Artists with zero tracks should not appear.
        lib.upsert_artist("Lonely Artist").unwrap();
        let artists2 = lib.artists().unwrap();
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
            album_artist_names: vec![],
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

        let tracks = lib.tracks_by_artist(radiohead).unwrap();
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
            album_artist_names: Vec::new(),
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

    #[test]
    fn test_playlist_track_refs_survive_clear_and_rescan() {
        // Snapshot → clear → re-insert tracks at new ids → restore: playlist
        // contents must reappear referencing the new track ids.
        let (lib, _path) = create_test_db();
        let track_id = seed_track(&lib, "Persistent Song", "Album", "Artist");
        let playlist_id = lib.create_playlist("Keepers").unwrap();
        lib.add_track_to_playlist(playlist_id, track_id).unwrap();

        let refs = lib.playlist_track_refs().unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].path, "/music/Persistent Song.flac");

        lib.clear().unwrap();
        // After clear, playlists themselves survive but membership is empty.
        assert!(lib.tracks_for_playlist(playlist_id).unwrap().is_empty());

        // Rescan: re-insert the same track. It gets a fresh id; the snapshot
        // matches by (path, start_offset_ms) and reinstates the membership.
        let new_track_id = seed_track(&lib, "Persistent Song", "Album", "Artist");
        lib.restore_playlist_track_refs(&refs).unwrap();
        let tracks = lib.tracks_for_playlist(playlist_id).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, new_track_id);
    }

    #[test]
    fn test_restore_playlist_track_refs_skips_missing_tracks() {
        // A track that doesn't reappear after a rescan (file deleted on disk)
        // simply drops out of any playlists referencing it.
        let (lib, _path) = create_test_db();
        let kept_id = seed_track(&lib, "Kept", "Album", "Artist");
        let dropped_id = seed_track(&lib, "Dropped", "Album", "Artist");
        let playlist_id = lib.create_playlist("Mixed").unwrap();
        lib.add_track_to_playlist(playlist_id, kept_id).unwrap();
        lib.add_track_to_playlist(playlist_id, dropped_id).unwrap();

        let refs = lib.playlist_track_refs().unwrap();
        lib.clear().unwrap();
        // Only re-seed the kept track.
        seed_track(&lib, "Kept", "Album", "Artist");
        lib.restore_playlist_track_refs(&refs).unwrap();

        let tracks = lib.tracks_for_playlist(playlist_id).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "Kept");
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
            })
            .unwrap();
        session
            .add_cover(&hash, thumbs.small, thumbs.large, "/music/a.flac", true)
            .unwrap();
        session.finish().unwrap();

        let albums = lib.albums().unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].title, "Album");
        assert_eq!(albums[0].artist_name, "Artist");
        assert!(albums[0].cover_art_id.is_some());
        let tracks = lib.tracks_for_album(albums[0].id).unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "A");
        assert_eq!(tracks[0].cover_art_id, albums[0].cover_art_id);
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

    #[test]
    fn test_cover_art_source_none_when_untracked() {
        let (lib, _path) = create_test_db();
        let id = lib.save_cover_art(&make_test_jpeg(&[4, 5, 6])).unwrap();
        assert_eq!(lib.get_cover_art_source(id).unwrap(), None);
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
        let existing_id = lib.save_cover_art(&cover).unwrap();
        let hash = sha256_hex(&cover);

        let mut session = lib.open_scan_session().unwrap();
        session.clear().unwrap();
        session
            .add_track(ScanTrack {
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
                    synced: true,
                    source: "lrclib".into(),
                }),
            })
            .unwrap();
        session.finish().unwrap();

        let tracks = lib.all_tracks().unwrap();
        assert_eq!(tracks.len(), 1);
        let stored = lib.lyrics_for_track(tracks[0].id).unwrap().unwrap();
        assert!(stored.synced);
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
                    synced: false,
                    source: "embedded".into(),
                }),
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

        let cover1 = lib.save_cover_art(&make_test_jpeg(&[255, 0, 0])).unwrap();
        let cover2 = lib.save_cover_art(&make_test_jpeg(&[0, 255, 0])).unwrap();
        let cover3 = lib.save_cover_art(&make_test_jpeg(&[0, 0, 255])).unwrap();
        let cover4 = lib.save_cover_art(&make_test_jpeg(&[128, 128, 0])).unwrap();

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
            album_artist_names: vec![],
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

        let covers = lib.artist_album_covers().unwrap();

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
        let covers2 = lib.artist_album_covers().unwrap();
        assert!(!covers2.contains_key(&no_cover_artist));
    }
}
