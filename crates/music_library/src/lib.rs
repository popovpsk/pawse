pub mod error;
pub mod migrations;
pub mod models;
pub mod repository;
pub mod sqlite;
pub mod thumbnail;

pub use error::{LibraryError, Result};
pub use models::{
    Album, AlbumSearchEntry, AlbumSummary, Artist, ArtistSummary, CoverArt, NewTrack, Playlist,
    PlaylistSummary, Track,
};
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
            cover_art_id: None,
            start_offset_ms: None,
        };
        lib.upsert_track(&track, Some(album_id), &[(artist_id, 0)])
            .unwrap();

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
            cover_art_id: None,
            start_offset_ms: None,
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
            cover_art_id: None,
            start_offset_ms: None,
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

        lib.clear().unwrap();

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

        let mk_track = |path: &str, title: &str, album_id: i64| NewTrack {
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
}
