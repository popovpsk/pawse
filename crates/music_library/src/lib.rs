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

        let temp_dir = std::env::temp_dir().join("gpui-test-music-library");
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
            track_number: Some(1),
            disc_number: Some(1),
            year: Some(1997),
            duration_ms: Some(285_000),
            cover_art: None,
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
            track_number: None,
            disc_number: None,
            year: None,
            duration_ms: None,
            cover_art: None,
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
            track_number: None,
            disc_number: None,
            year: None,
            duration_ms: None,
            cover_art: None,
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
            track_number: None,
            disc_number: None,
            year: None,
            duration_ms: None,
            cover_art: None,
        };
        let _track_id = lib.upsert_track(&track, Some(album_id), &[(artist_id, 0)]).unwrap();
        let tracks = lib.tracks_for_album(album_id).unwrap();
        assert_eq!(tracks[0].title, "Unknown Title");
    }
}
