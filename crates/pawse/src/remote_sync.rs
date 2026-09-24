use std::collections::{HashMap, HashSet};

use music_library::{LibraryRepository, RemoteCover, RemoteSong, RemoteSource, RemoteSyncReport};

use crate::servers::{
    RemoteConfig, RemoteError, RemoteServer, ServerClient, ServerKind, source_key,
};

pub fn reconcile(
    repo: &dyn LibraryRepository,
    servers: &[RemoteServer],
) -> HashMap<i64, RemoteConfig> {
    for kind in ServerKind::ALL {
        let sources: Vec<RemoteSource> = servers
            .iter()
            .filter(|server| server.kind() == kind)
            .map(|server| RemoteSource {
                uri: server.uri.clone(),
                name: server.name.clone(),
            })
            .collect();
        if let Err(e) = repo.reconcile_remote_sources(kind.as_str(), &sources) {
            log::error!("Failed to reconcile {} sources: {e}", kind.title());
        }
    }
    let ids = source_ids(repo);
    servers
        .iter()
        .filter_map(|server| {
            ids.get(&server.key())
                .map(|id| (*id, server.config.clone()))
        })
        .collect()
}

pub fn source_ids(repo: &dyn LibraryRepository) -> HashMap<String, i64> {
    repo.sources()
        .unwrap_or_default()
        .into_iter()
        .filter(|source| source.enabled)
        .filter_map(|source| {
            let kind = ServerKind::parse(&source.kind)?;
            Some((source_key(kind, &source.uri), source.id))
        })
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
                s.kind == server.kind().as_str() && s.enabled && !s.available && s.uri == server.uri
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
                log::warn!("Server source {source_id}: storing the listing failed: {error}");
                last = error.to_string();
            }
        }
    }
    Err(Failure::Local(RemoteError::Other(last)))
}

pub fn sync_server(
    repo: &dyn LibraryRepository,
    source_id: i64,
    config: &RemoteConfig,
) -> SyncOutcome {
    let client = config.client();
    let listed = client
        .ping()
        .and_then(|()| client.songs())
        .map_err(Failure::Source)
        .and_then(|mut songs| {
            let covers = fetch_covers(repo, &*client, config.kind(), source_id, &mut songs);
            apply_listing(repo, source_id, &songs, &covers)
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
    config: &RemoteConfig,
) -> Result<(Vec<i64>, usize), RemoteError> {
    let keys = config.client().favorite_keys()?;
    let items = repo.items_for_remote_keys(source_id, &keys)?;
    Ok((items, keys.len()))
}

fn fetch_covers(
    repo: &dyn LibraryRepository,
    client: &dyn ServerClient,
    kind: ServerKind,
    source_id: i64,
    songs: &mut [RemoteSong],
) -> Vec<RemoteCover> {
    let mut hashes = repo.remote_cover_hashes(source_id).unwrap_or_default();
    let wanted: HashSet<String> = songs
        .iter()
        .filter_map(|song| song.cover_key.clone())
        .filter(|key| !hashes.contains_key(key))
        .collect();
    let mut covers: Vec<RemoteCover> = Vec::new();
    let mut stored: HashSet<String> = HashSet::new();
    for key in wanted {
        let bytes = match client.cover_art(&key) {
            Ok(bytes) if !bytes.is_empty() => bytes,
            Ok(_) => continue,
            Err(e) => {
                log::warn!("{} cover {key} not fetched: {e:?}", kind.title());
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
                    source_path: format!("{}-cover://{source_id}/{key}", kind.as_str()),
                }),
                Err(e) => {
                    log::warn!("{} cover {key} not decoded: {e}", kind.title());
                    continue;
                }
            }
        }
        hashes.insert(key, hash);
    }
    for song in songs.iter_mut() {
        song.cover_hash = song
            .cover_key
            .as_ref()
            .and_then(|key| hashes.get(key).cloned());
    }
    covers
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
            config: RemoteConfig::Subsonic(subsonic::Config {
                url: url.to_string(),
                username: "me".into(),
                password: "pw".into(),
            }),
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

    fn jellyfin_stub() -> String {
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
                let (path, query) = target.split_once('?').unwrap_or((&target, ""));
                let items = |items: serde_json::Value| {
                    let total = items.as_array().map_or(0, Vec::len);
                    serde_json::to_vec(
                        &serde_json::json!({"Items": items, "TotalRecordCount": total}),
                    )
                    .unwrap()
                };
                let song = |id: &str, title: &str, ticks: u64, file: &str| {
                    serde_json::json!({"Id": id, "Name": title, "Artists": ["Artist"],
                        "Album": "Album", "AlbumId": "al", "AlbumPrimaryImageTag": "t",
                        "RunTimeTicks": ticks, "Path": file})
                };
                let (content_type, body) = match path {
                    "/System/Info/Public" => ("application/json", br#"{"Id":"srv"}"#.to_vec()),
                    "/Users/Me" => ("application/json", br#"{"Id":"u1"}"#.to_vec()),
                    "/Items" if query.contains("Filters=IsFavorite") => (
                        "application/json",
                        items(serde_json::json!([
                            song("j2", "Only Remote", 2_000_000_000, "/m/b.opus"),
                            song("gone", "Unknown", 1, "/m/x.mp3")
                        ])),
                    ),
                    "/Items" if !query.contains("StartIndex=0") => {
                        ("application/json", items(serde_json::json!([])))
                    }
                    "/Items" => (
                        "application/json",
                        items(serde_json::json!([
                            song("j1", "Local Too", 1_800_000_000, "/m/a.flac"),
                            song("j2", "Only Remote", 2_000_000_000, "/m/b.opus")
                        ])),
                    ),
                    "/Items/al/Images/Primary" => ("image/png", cover.clone()),
                    _ => ("text/plain", Vec::new()),
                };
                let status = if content_type == "text/plain" {
                    "404 Not Found"
                } else {
                    "200 OK"
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        url
    }

    #[test]
    fn a_jellyfin_sync_matches_local_copies_and_imports_favorites() {
        let repo = temp_db("jellyfin");
        repo.reconcile_local_sources(&[LocalFolder {
            path: "/music".into(),
            available: true,
        }])
        .unwrap();
        scan_local(&repo, vec![local_track()]);
        let url = jellyfin_stub();
        let servers = vec![
            server("http://subsonic.invalid"),
            RemoteServer {
                uri: format!("me@{url}"),
                name: url.clone(),
                config: RemoteConfig::Jellyfin(jellyfin::Config {
                    url: url.clone(),
                    user_id: "u1".into(),
                    token: "tok".into(),
                    device_id: "dev".into(),
                }),
            },
        ];
        let configs = reconcile(&repo, &servers);
        assert_eq!(configs.len(), 2);
        let (&source_id, config) = configs
            .iter()
            .find(|(_, config)| config.kind() == ServerKind::Jellyfin)
            .unwrap();

        let outcome = sync_server(&repo, source_id, config);
        let report = outcome.result.unwrap();
        assert_eq!((report.total, report.added, report.adopted), (2, 1, 1));
        scan_local(&repo, vec![local_track()]);
        let remote_path = music_library::remote::locator(source_id, "j2", "opus");
        let tracks = repo.all_tracks().unwrap();
        let mut paths: Vec<&str> = tracks.iter().map(|t| t.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, vec!["/music/x/a.flac", remote_path.as_str()]);
        let remote_track = tracks.iter().find(|t| t.path == remote_path).unwrap();
        assert!(remote_track.cover_art_id.is_some());
        assert_eq!(remote_track.duration_ms, Some(200_000));

        let (items, total) = import_stars(&repo, source_id, config).unwrap();
        assert_eq!((items, total), (vec![remote_track.id], 2));

        let ids = source_ids(&repo);
        assert_eq!(ids.len(), 2);
        assert_eq!(ids.get(&servers[1].key()), Some(&source_id));
        reconcile(&repo, &servers[..1]);
        assert_eq!(source_ids(&repo).len(), 1);
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
            summary("me@http://down", "subsonic", true, false),
            summary("me@http://up", "subsonic", true, true),
            summary("me@http://gone", "subsonic", false, false),
            summary("/music", "local", true, false),
        ];
        let named = |uri: &str| RemoteServer {
            uri: uri.into(),
            ..server("http://unused")
        };
        let jellyfin_at_down = RemoteServer {
            config: RemoteConfig::Jellyfin(jellyfin::Config {
                url: "http://down".into(),
                user_id: "u".into(),
                token: "t".into(),
                device_id: "d".into(),
            }),
            ..named("me@http://down")
        };
        let picked: Vec<String> = offline_servers(
            &summaries,
            ["me@http://down", "me@http://up", "me@http://gone", "/music"]
                .into_iter()
                .map(named)
                .chain(std::iter::once(jellyfin_at_down))
                .collect(),
        )
        .into_iter()
        .map(|server| server.key())
        .collect();
        assert_eq!(picked, vec!["subsonic:me@http://down".to_string()]);
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
