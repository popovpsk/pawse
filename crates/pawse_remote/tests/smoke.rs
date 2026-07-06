use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

struct TestLibrary;

impl pawse_remote::LibraryReader for TestLibrary {
    fn cover(&self, id: i64, size: pawse_remote::CoverSize) -> Option<Vec<u8>> {
        match (id, size) {
            (7, pawse_remote::CoverSize::Small) => Some(b"small-bytes".to_vec()),
            (7, pawse_remote::CoverSize::Large) => Some(b"large-bytes".to_vec()),
            _ => None,
        }
    }

    fn cover_original(&self, id: i64) -> Option<(Vec<u8>, String)> {
        match id {
            7 => Some((b"original-bytes".to_vec(), "image/jpeg".to_string())),
            _ => None,
        }
    }

    fn artists(&self) -> Vec<pawse_remote::ArtistEntry> {
        vec![pawse_remote::ArtistEntry {
            id: 1,
            name: "Smoke Artist".into(),
            track_count: 1,
            cover_ids: vec![7],
        }]
    }

    fn artist_detail(&self, artist_id: i64, full: bool) -> Option<pawse_remote::ArtistDetail> {
        if artist_id != 1 {
            return None;
        }
        let title = if full { "Full Album" } else { "Smoke Album" };
        Some(pawse_remote::ArtistDetail {
            id: 1,
            name: "Smoke Artist".into(),
            has_partial: true,
            albums: vec![pawse_remote::ArtistAlbum {
                album_id: Some(3),
                title: title.into(),
                year: Some(2024),
                cover_id: Some(7),
                partial: true,
                tracks: vec![pawse_remote::AlbumTrack {
                    id: 11,
                    title: "Smoke Track".into(),
                    track_number: Some(1),
                    disc_number: 1,
                    duration_ms: 1000,
                }],
            }],
        })
    }

    fn playlists(&self) -> Vec<pawse_remote::PlaylistEntry> {
        vec![pawse_remote::PlaylistEntry {
            id: 5,
            name: "Smoke Playlist".into(),
            track_count: 1,
        }]
    }

    fn playlist_detail(&self, playlist_id: i64) -> Option<pawse_remote::PlaylistDetail> {
        if playlist_id != 5 {
            return None;
        }
        Some(pawse_remote::PlaylistDetail {
            id: 5,
            name: "Smoke Playlist".into(),
            tracks: vec![pawse_remote::PlaylistTrack {
                id: 11,
                title: "Smoke Track".into(),
                artist: Some("Smoke Artist".into()),
                cover_id: Some(7),
                duration_ms: 1000,
            }],
        })
    }

    fn liked(&self) -> Vec<pawse_remote::PlaylistTrack> {
        vec![pawse_remote::PlaylistTrack {
            id: 11,
            title: "Smoke Track".into(),
            artist: Some("Smoke Artist".into()),
            cover_id: Some(7),
            duration_ms: 1000,
        }]
    }
}

#[test]
fn serves_state_snapshot() {
    let addr: SocketAddr = ([127, 0, 0, 1], 18770).into();
    let (handle, rx) = pawse_remote::channel();
    let (commands, _command_rx) = pawse_remote::commands();
    let (_server, _ready) = pawse_remote::spawn(addr, rx, commands, Arc::new(TestLibrary));
    handle.publish(pawse_remote::PlayerState {
        has_track: true,
        title: Some("Smoke Track".into()),
        playing: true,
        volume: 0.5,
        ..Default::default()
    });

    let body = wait_for_state(addr);
    assert!(body.contains("\"v\":3"), "body: {body}");
    assert!(body.contains("Smoke Track"), "body: {body}");
    assert!(body.contains("\"playing\":true"), "body: {body}");
    assert!(body.contains("\"repeat\":\"off\""), "body: {body}");
    assert!(body.contains("\"volume\":0.5"), "body: {body}");

    let queue = try_get(addr, "/api/queue").expect("queue endpoint");
    assert_eq!(queue.trim(), "[]", "queue: {queue}");

    let small = try_get(addr, "/api/cover?id=7&size=small").expect("cover endpoint");
    assert_eq!(small, "small-bytes");
    let large = try_get(addr, "/api/cover?id=7").expect("cover endpoint");
    assert_eq!(large, "large-bytes");
    assert!(try_get(addr, "/api/cover?id=8").is_none());

    let artists = try_get(addr, "/api/artists").expect("artists endpoint");
    assert!(artists.contains("Smoke Artist"), "artists: {artists}");
    assert!(artists.contains("\"cover_ids\":[7]"), "artists: {artists}");

    let artist = try_get(addr, "/api/artist?id=1").expect("artist endpoint");
    assert!(artist.contains("Smoke Album"), "artist: {artist}");
    assert!(artist.contains("\"has_partial\":true"), "artist: {artist}");
    let artist_full = try_get(addr, "/api/artist?id=1&full=1").expect("artist endpoint");
    assert!(artist_full.contains("Full Album"), "artist: {artist_full}");
    assert!(try_get(addr, "/api/artist?id=2").is_none());

    let playlists = try_get(addr, "/api/playlists").expect("playlists endpoint");
    assert!(
        playlists.contains("Smoke Playlist"),
        "playlists: {playlists}"
    );

    let playlist = try_get(addr, "/api/playlist?id=5").expect("playlist endpoint");
    assert!(playlist.contains("Smoke Playlist"), "playlist: {playlist}");
    assert!(playlist.contains("Smoke Track"), "playlist: {playlist}");
    assert!(try_get(addr, "/api/playlist?id=6").is_none());

    let liked = try_get(addr, "/api/liked").expect("liked endpoint");
    assert!(liked.contains("Smoke Track"), "liked: {liked}");
}

fn wait_for_state(addr: SocketAddr) -> String {
    for _ in 0..50 {
        if let Some(body) = try_get(addr, "/api/state") {
            return body;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("server did not respond on {addr}");
}

fn try_get(addr: SocketAddr, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(200)).ok()?;
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .ok()?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf).ok()?;
    let (head, body) = buf.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 200") {
        return None;
    }
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}
