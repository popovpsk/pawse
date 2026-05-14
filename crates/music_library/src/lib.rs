pub mod error;
pub mod migrations;
pub mod models;
pub mod repository;
pub mod sqlite;

pub use error::{LibraryError, Result};
pub use models::{Album, AlbumSummary, Artist, NewTrack, Track};
pub use repository::LibraryRepository;
pub use sqlite::SqliteLibrary;

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
        // Migrations should succeed and DB should be empty
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
        assert_eq!(album_id, album_id2, "upsert should return same id for same album");
    }

    #[test]
    fn test_album_artists() {
        let (lib, _path) = create_test_db();
        let artist1 = lib.upsert_artist("The Beatles").unwrap();
        let artist2 = lib.upsert_artist("Billy Preston").unwrap();
        let album_id = lib.upsert_album("Let It Be", Some(1970), None).unwrap();
        lib.set_album_artists(album_id, &[(artist1, 0), (artist2, 1)]).unwrap();

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
            cover_art: None,
            start_offset_ms: None,
        };
        let _track_id = lib.upsert_track(&track, Some(album_id), &[(artist_id, 0)]).unwrap();

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
        // Led Zeppelin should come before The Beatles (sort_name: "Beatles, The" vs "Led Zeppelin")
        assert_eq!(albums[0].title, "IV");
        assert_eq!(albums[1].title, "Abbey Road");
    }

    #[test]
    fn test_search_tracks() {
        let (lib, _path) = create_test_db();
        let artist_id = lib.upsert_artist("Artist").unwrap();
        let album_id = lib.upsert_album("Album", None, None).unwrap();
        lib.set_album_artists(album_id, &[(artist_id, 0)]).unwrap();

        let track = NewTrack {
            path: "/music/song.flac".into(),
            title: Some("My Song".into()),
            album_title: Some("Album".into()),
            artist_names: vec!["Artist".into()],
            album_artist_names: Vec::new(),
            track_number: None,
            disc_number: None,
            year: None,
            duration_ms: None,
            cover_art: None,
            start_offset_ms: None,
        };
        lib.upsert_track(&track, Some(album_id), &[(artist_id, 0)]).unwrap();

        let results = lib.search("Song").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "My Song");
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
            cover_art: None,
            start_offset_ms: None,
        };
        lib.upsert_track(&track, Some(album_id), &[(artist_id, 0)]).unwrap();

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
            cover_art: None,
            start_offset_ms: None,
        };
        let _track_id = lib.upsert_track(&track, Some(album_id), &[(artist_id, 0)]).unwrap();
        let tracks = lib.tracks_for_album(album_id).unwrap();
        assert_eq!(tracks[0].title, "Unknown Title");
    }

    #[test]
    fn test_multidisc_tracks_ordered_by_disc() {
        let (lib, _path) = create_test_db();
        let album_artist_id = lib.upsert_artist("Album Artist").unwrap();
        let track1_artist_id = lib.upsert_artist("Artist One").unwrap();
        let track2_artist_id = lib.upsert_artist("Artist Two").unwrap();
        let album_id = lib.upsert_album("Multi-Disc Album", Some(2020), None).unwrap();

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
            cover_art: None,
            start_offset_ms: None,
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
            cover_art: None,
            start_offset_ms: None,
        };

        lib.upsert_track(&track1, Some(album_id), &[(track1_artist_id, 0)]).unwrap();
        lib.upsert_track(&track2, Some(album_id), &[(track2_artist_id, 0)]).unwrap();
        lib.set_album_artists(album_id, &[(album_artist_id, 0)]).unwrap();

        let tracks = lib.tracks_for_album(album_id).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].disc_number, 1);
        assert_eq!(tracks[0].title, "Track One");
        assert_eq!(tracks[1].disc_number, 2);
        assert_eq!(tracks[1].title, "Track Two");

        // Album artist should be "Album Artist" from the first track's album_artist_names,
        // and should NOT have been overwritten by the second track's artist.
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
            cover_art: None,
            start_offset_ms: None,
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
        assert_eq!(lib.album_title(album_id).unwrap(), Some("Test Album".into()));
    }

    #[test]
    fn test_album_title_not_found() {
        let (lib, _path) = create_test_db();
        assert!(lib.album_title(999).unwrap().is_none());
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

        // Album has no tracks — it's an orphan
        lib.delete_orphaned_albums_and_artists().unwrap();

        // Album should be deleted
        assert!(lib.album_title(album_id).unwrap().is_none());
        // Album_artists cascade-deleted
        assert!(!lib.album_has_artists(album_id).unwrap());
    }

    #[test]
    fn test_save_cover_art() {
        let (lib, _path) = create_test_db();
        let data = b"fake-jpeg-bytes";
        let path = lib.save_cover_art(data).unwrap();
        assert!(path.ends_with(".jpg"));
        assert!(std::path::Path::new(&path).exists());
        let saved = std::fs::read(&path).unwrap();
        assert_eq!(saved, data);
    }

    #[test]
    fn test_search_no_results() {
        let (lib, _path) = create_test_db();
        let results = lib.search("nonexistent").unwrap();
        assert!(results.is_empty());
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
            cover_art: None,
            start_offset_ms: None,
        };
        lib.upsert_track(&track, Some(album_id), &[(artist_id, 0)])
            .unwrap();
        assert!(lib.has_tracks().unwrap());

        lib.clear().unwrap();
        assert!(!lib.has_tracks().unwrap());

        // Re-populate
        let artist_id2 = lib.upsert_artist("Artist").unwrap();
        let album_id2 = lib.upsert_album("Album", None, None).unwrap();
        lib.set_album_artists(album_id2, &[(artist_id2, 0)]).unwrap();
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
        // Add a track with track artists but no album artists
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
            cover_art: None,
            start_offset_ms: None,
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
            cover_art: None,
            start_offset_ms: None,
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
            cover_art: None,
            start_offset_ms: Some(0),
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
            cover_art: None,
            start_offset_ms: Some(300_000),
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
}
