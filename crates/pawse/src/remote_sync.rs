use std::collections::{HashMap, HashSet};

use music_library::{LibraryRepository, RemoteCover, RemoteSong, RemoteSource, RemoteSyncReport};

pub const SUBSONIC_KIND: &str = "subsonic";

#[derive(Clone, Debug)]
pub struct RemoteServer {
    pub uri: String,
    pub name: String,
    pub config: subsonic::Config,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteError {
    Auth,
    Unreachable(String),
    Other(String),
}

impl From<subsonic::Error> for RemoteError {
    fn from(error: subsonic::Error) -> Self {
        match error {
            subsonic::Error::Auth => RemoteError::Auth,
            subsonic::Error::Transient(message) => RemoteError::Unreachable(message),
            subsonic::Error::Server(message) => RemoteError::Other(message),
        }
    }
}

impl From<music_library::LibraryError> for RemoteError {
    fn from(error: music_library::LibraryError) -> Self {
        RemoteError::Other(error.to_string())
    }
}

pub fn reconcile(
    repo: &dyn LibraryRepository,
    servers: &[RemoteServer],
) -> HashMap<i64, subsonic::Config> {
    let sources: Vec<RemoteSource> = servers
        .iter()
        .map(|server| RemoteSource {
            uri: server.uri.clone(),
            name: server.name.clone(),
        })
        .collect();
    if let Err(e) = repo.reconcile_remote_sources(SUBSONIC_KIND, &sources) {
        log::error!("Failed to reconcile Subsonic sources: {e}");
    }
    let ids = source_ids(repo);
    servers
        .iter()
        .filter_map(|server| ids.get(&server.uri).map(|id| (*id, server.config.clone())))
        .collect()
}

pub fn source_ids(repo: &dyn LibraryRepository) -> HashMap<String, i64> {
    repo.sources()
        .unwrap_or_default()
        .into_iter()
        .filter(|source| source.kind == SUBSONIC_KIND && source.enabled)
        .map(|source| (source.uri, source.id))
        .collect()
}

pub struct SyncOutcome {
    pub result: Result<RemoteSyncReport, RemoteError>,
    pub changed: bool,
}

pub fn offline_servers(
    summaries: &[music_library::SourceSummary],
    servers: Vec<RemoteServer>,
) -> Vec<RemoteServer> {
    servers
        .into_iter()
        .filter(|server| {
            summaries.iter().any(|s| {
                s.kind == SUBSONIC_KIND && s.enabled && !s.available && s.uri == server.uri
            })
        })
        .collect()
}

const APPLY_ATTEMPTS: usize = 4;
const APPLY_RETRY: std::time::Duration = std::time::Duration::from_secs(2);

enum Failure {
    Source(RemoteError),
    Local(RemoteError),
}

fn apply_listing(
    repo: &dyn LibraryRepository,
    source_id: i64,
    songs: &[RemoteSong],
    covers: &[RemoteCover],
) -> Result<RemoteSyncReport, Failure> {
    let mut last = String::new();
    for attempt in 0..APPLY_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(APPLY_RETRY);
        }
        match repo.apply_remote_listing(source_id, songs, covers) {
            Ok(report) => return Ok(report),
            Err(music_library::LibraryError::InvalidData(message)) => {
                return Err(Failure::Source(RemoteError::Other(message)));
            }
            Err(error) => {
                log::warn!("Subsonic source {source_id}: storing the listing failed: {error}");
                last = error.to_string();
            }
        }
    }
    Err(Failure::Local(RemoteError::Other(last)))
}

pub fn sync_server(
    repo: &dyn LibraryRepository,
    source_id: i64,
    config: &subsonic::Config,
) -> SyncOutcome {
    let client = subsonic::Client::new(config);
    let listed = client
        .ping()
        .and_then(|()| client.songs())
        .map_err(|e| Failure::Source(RemoteError::from(e)))
        .and_then(|songs| {
            let (hashes, covers) = fetch_covers(repo, &client, source_id, &songs);
            let remote: Vec<RemoteSong> = songs
                .into_iter()
                .map(|song| to_remote_song(song, &hashes))
                .collect();
            apply_listing(repo, source_id, &remote, &covers)
        });
    match listed {
        Ok(report) => SyncOutcome {
            changed: report.changed(),
            result: Ok(report),
        },
        Err(Failure::Local(error)) => SyncOutcome {
            result: Err(error),
            changed: false,
        },
        Err(Failure::Source(error)) => {
            let changed = repo
                .set_source_available(source_id, false)
                .unwrap_or_else(|db| {
                    log::error!("Failed to mark source {source_id} unavailable: {db}");
                    false
                });
            SyncOutcome {
                result: Err(error),
                changed,
            }
        }
    }
}

pub fn import_stars(
    repo: &dyn LibraryRepository,
    source_id: i64,
    config: &subsonic::Config,
) -> Result<(Vec<i64>, usize), RemoteError> {
    let starred = subsonic::Client::new(config).starred_songs()?;
    let keys: Vec<String> = starred.into_iter().map(|song| song.id).collect();
    let items = repo.items_for_remote_keys(source_id, &keys)?;
    Ok((items, keys.len()))
}

fn fetch_covers(
    repo: &dyn LibraryRepository,
    client: &subsonic::Client,
    source_id: i64,
    songs: &[subsonic::Song],
) -> (HashMap<String, String>, Vec<RemoteCover>) {
    let mut hashes = repo.remote_cover_hashes(source_id).unwrap_or_default();
    let wanted: HashSet<&str> = songs
        .iter()
        .filter_map(|song| song.cover_art.as_deref())
        .filter(|key| !hashes.contains_key(*key))
        .collect();
    let mut covers: Vec<RemoteCover> = Vec::new();
    let mut stored: HashSet<String> = HashSet::new();
    for key in wanted {
        let bytes = match client.cover_art(key) {
            Ok(bytes) if !bytes.is_empty() => bytes,
            Ok(_) => continue,
            Err(e) => {
                log::warn!("Subsonic cover {key} not fetched: {e}");
                continue;
            }
        };
        let hash = music_library::sha256_hex(&bytes);
        if stored.insert(hash.clone()) {
            match music_library::thumbnail::generate_thumbnails(&bytes) {
                Ok(thumbs) => covers.push(RemoteCover {
                    hash: hash.clone(),
                    small: thumbs.small,
                    large: thumbs.large,
                    source_path: format!("subsonic-cover://{source_id}/{key}"),
                }),
                Err(e) => {
                    log::warn!("Subsonic cover {key} not decoded: {e}");
                    continue;
                }
            }
        }
        hashes.insert(key.to_string(), hash);
    }
    (hashes, covers)
}

const UNKNOWN_ALBUM: &str = "[unknown album]";
const MAX_TRACK_NUMBER: u32 = 999;

fn first_name(names: &[subsonic::Named]) -> Option<String> {
    names
        .iter()
        .map(|named| named.name.trim())
        .find(|name| !name.is_empty())
        .map(str::to_string)
}

fn real_artist(name: Option<String>) -> Option<String> {
    name.map(|name| name.trim().to_string())
        .filter(|name| !crate::library_service::is_placeholder_artist(name))
}

fn to_remote_song(song: subsonic::Song, covers: &HashMap<String, String>) -> RemoteSong {
    let cover_hash = song
        .cover_art
        .as_ref()
        .and_then(|key| covers.get(key).cloned());
    RemoteSong {
        key: song.id,
        title: song.title,
        artist: real_artist(first_name(&song.artists).or(song.artist.clone())),
        artist_aliases: song
            .artist
            .iter()
            .cloned()
            .chain(song.artists.iter().skip(1).map(|named| named.name.clone()))
            .filter_map(|name| real_artist(Some(name)))
            .collect(),
        album: song
            .album
            .filter(|album| !album.trim().eq_ignore_ascii_case(UNKNOWN_ALBUM)),
        album_artist: real_artist(first_name(&song.album_artists).or(song.album_artist)),
        track_number: song.track.filter(|n| (1..=MAX_TRACK_NUMBER).contains(n)),
        disc_number: song.disc_number,
        year: song.year,
        genre: song.genre,
        duration_ms: song.duration.map(|secs| (secs * 1000) as i64),
        size: song.size.map(|size| size as i64),
        suffix: song.suffix,
        content_type: song.content_type,
        bitrate: song.bit_rate,
        cover_key: song.cover_art,
        cover_hash,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    use music_library::{LibraryRepository, LocalFolder, ScanTrack, SqliteLibrary};

    use super::*;

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbImage::from_pixel(4, 4, image::Rgb([200, 10, 10]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    fn stub_server() -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let cover = png();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err()
                        || header == "\r\n"
                        || header.is_empty()
                    {
                        break;
                    }
                }
                let target = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let method = target
                    .split('?')
                    .next()
                    .unwrap_or("")
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_string();
                let json = |body: serde_json::Value| {
                    let mut inner = serde_json::json!({"status": "ok", "version": "1.16.1"});
                    for (k, v) in body.as_object().unwrap() {
                        inner[k] = v.clone();
                    }
                    serde_json::to_vec(&serde_json::json!({"subsonic-response": inner})).unwrap()
                };
                let (content_type, body) = match method.as_str() {
                    "ping" => ("application/json", json(serde_json::json!({}))),
                    "search3" if !target.contains("songOffset=0") => (
                        "application/json",
                        json(serde_json::json!({"searchResult3": {}})),
                    ),
                    "search3" => (
                        "application/json",
                        json(serde_json::json!({"searchResult3": {"song": [
                            {"id": "s1", "title": "Local Too", "artist": "Artist", "album": "Album",
                             "duration": 180, "suffix": "flac", "path": "x/a.flac", "coverArt": "c1"},
                            {"id": "s2", "title": "Only Remote", "artist": "Artist", "album": "Album",
                             "duration": 200, "suffix": "mp3", "path": "x/b.mp3", "coverArt": "c1"}
                        ]}})),
                    ),
                    "getCoverArt" => ("image/png", cover.clone()),
                    "getStarred2" => (
                        "application/json",
                        json(serde_json::json!({"starred2": {"song": [
                            {"id": "s2", "title": "Only Remote"},
                            {"id": "not-synced", "title": "Unknown"}
                        ]}})),
                    ),
                    _ => ("application/json", json(serde_json::json!({}))),
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        url
    }

    fn temp_db(name: &str) -> SqliteLibrary {
        let path = std::env::temp_dir().join(format!(
            "pawse-remote-sync-{}-{name}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        SqliteLibrary::open_at(&path).unwrap()
    }

    fn scan_local(repo: &SqliteLibrary, tracks: Vec<ScanTrack>) {
        let mut session = repo.open_scan_session().unwrap();
        session.clear().unwrap();
        for track in tracks {
            session.add_track(track).unwrap();
        }
        session.finish().unwrap();
    }

    fn local_track() -> ScanTrack {
        ScanTrack {
            path: "/music/x/a.flac".into(),
            title: Some("Local Too".into()),
            album_title: Some("Album".into()),
            artist_names: vec!["Artist".into()],
            duration_ms: Some(180_000),
            ..Default::default()
        }
    }

    fn server(url: &str) -> RemoteServer {
        RemoteServer {
            uri: format!("me@{url}"),
            name: url.to_string(),
            config: subsonic::Config {
                url: url.to_string(),
                username: "me".into(),
                password: "pw".into(),
            },
        }
    }

    #[test]
    fn a_sync_lists_the_server_matches_local_copies_and_brings_covers() {
        let repo = temp_db("sync");
        repo.reconcile_local_sources(&[LocalFolder {
            path: "/music".into(),
            available: true,
        }])
        .unwrap();
        scan_local(&repo, vec![local_track()]);
        let url = stub_server();
        let servers = vec![server(&url)];
        let configs = reconcile(&repo, &servers);
        let (&source_id, _) = configs.iter().next().unwrap();

        let outcome = sync_server(&repo, source_id, &servers[0].config);
        let changed = outcome.changed;
        let report = outcome.result.unwrap();
        assert!(changed);
        assert_eq!((report.total, report.added, report.adopted), (2, 1, 1));
        scan_local(&repo, vec![local_track()]);

        let again = sync_server(&repo, source_id, &servers[0].config);
        assert!(!again.changed);
        let tracks = repo.all_tracks().unwrap();
        let mut paths: Vec<&str> = tracks.iter().map(|t| t.path.as_str()).collect();
        paths.sort();
        let remote_path = music_library::remote::locator(source_id, "s2", "mp3");
        assert_eq!(paths, vec!["/music/x/a.flac", remote_path.as_str()]);
        let remote_track = tracks.iter().find(|t| t.path == remote_path).unwrap();
        assert!(remote_track.cover_art_id.is_some());
        assert_eq!(remote_track.duration_ms, Some(200_000));

        let (items, total) = import_stars(&repo, source_id, &servers[0].config).unwrap();
        assert_eq!((items, total), (vec![remote_track.id], 2));
    }

    #[test]
    fn the_first_credited_artist_is_used_instead_of_the_joined_display_name() {
        let song = subsonic::Song {
            id: "1".into(),
            title: "Moonlight".into(),
            artist: Some("Daniel Lanois • Daryl Johnson".into()),
            artists: vec![
                subsonic::Named {
                    name: "Daniel Lanois".into(),
                },
                subsonic::Named {
                    name: "Daryl Johnson".into(),
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            to_remote_song(song, &HashMap::new()).artist.as_deref(),
            Some("Daniel Lanois")
        );
    }

    #[test]
    fn server_placeholders_for_missing_tags_are_dropped() {
        let song = subsonic::Song {
            id: "1".into(),
            title: "Whole Album Image".into(),
            artist: Some("[Unknown Artist]".into()),
            album_artist: Some(" [unknown artist] ".into()),
            album: Some("[Unknown Album]".into()),
            track: Some(1997),
            ..Default::default()
        };
        let remote = to_remote_song(song, &HashMap::new());
        assert_eq!(remote.artist, None);
        assert_eq!(remote.album_artist, None);
        assert_eq!(remote.album, None);
        assert_eq!(remote.track_number, None);
    }

    #[test]
    fn only_enabled_unavailable_servers_are_retried_in_the_background() {
        let summary =
            |uri: &str, kind: &str, enabled: bool, available: bool| music_library::SourceSummary {
                id: 0,
                kind: kind.into(),
                uri: uri.into(),
                enabled,
                available,
                track_count: 0,
            };
        let summaries = vec![
            summary("me@http://down", SUBSONIC_KIND, true, false),
            summary("me@http://up", SUBSONIC_KIND, true, true),
            summary("me@http://gone", SUBSONIC_KIND, false, false),
            summary("/music", "local", true, false),
        ];
        let named = |uri: &str| RemoteServer {
            uri: uri.into(),
            ..server("http://unused")
        };
        let picked: Vec<String> = offline_servers(
            &summaries,
            ["me@http://down", "me@http://up", "me@http://gone", "/music"]
                .into_iter()
                .map(named)
                .collect(),
        )
        .into_iter()
        .map(|server| server.uri)
        .collect();
        assert_eq!(picked, vec!["me@http://down".to_string()]);
    }

    #[test]
    fn an_unreachable_server_is_marked_unavailable_and_nothing_else_changes() {
        let repo = temp_db("down");
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        drop(listener);
        let servers = vec![server(&url)];
        let (&source_id, _) = reconcile(&repo, &servers).iter().next().unwrap();

        let outcome = sync_server(&repo, source_id, &servers[0].config);
        assert!(matches!(outcome.result, Err(RemoteError::Unreachable(_))));
        assert!(outcome.changed);
        let source = repo
            .sources()
            .unwrap()
            .into_iter()
            .find(|s| s.id == source_id)
            .unwrap();
        assert!(!source.available);
    }
}
