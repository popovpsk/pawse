use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use music_library::{LibraryRepository, RemoteCover, RemoteSong, RemoteSource, RemoteSyncReport};

use crate::servers::{
    PlaylistScope, RemoteConfig, RemoteError, RemotePlaylist, RemoteServer, ServerClient,
    ServerKind, source_key,
};

pub fn reconcile(
    repo: &dyn LibraryRepository,
    servers: &[RemoteServer],
) -> (HashMap<i64, RemoteConfig>, bool) {
    let mut changed = false;
    for kind in ServerKind::ALL {
        let sources: Vec<RemoteSource> = servers
            .iter()
            .filter(|server| server.kind() == kind)
            .map(|server| RemoteSource {
                uri: server.uri.clone(),
                name: server.name.clone(),
            })
            .collect();
        match repo.reconcile_remote_sources(kind.as_str(), &sources) {
            Ok(kind_changed) => changed |= kind_changed,
            Err(e) => log::error!("Failed to reconcile {} sources: {e}", kind.title()),
        }
    }
    let ids = source_ids(repo);
    let configs = servers
        .iter()
        .filter_map(|server| {
            ids.get(&server.key())
                .map(|id| (*id, server.config.clone()))
        })
        .collect();
    (configs, changed)
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

fn is_enabled(repo: &dyn LibraryRepository, source_id: i64) -> bool {
    match repo.sources() {
        Ok(sources) => sources
            .iter()
            .any(|source| source.id == source_id && source.enabled),
        Err(_) => true,
    }
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
const COVER_FETCHERS: usize = 4;
const COVER_PROGRESS_EVERY: std::time::Duration = std::time::Duration::from_secs(1);
const COVER_UNREACHABLE_LIMIT: usize = 8;

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

pub struct CoverWork {
    client: Arc<dyn ServerClient>,
    kind: ServerKind,
    source_id: i64,
    songs: Vec<RemoteSong>,
    resolved: HashMap<String, Option<String>>,
    shown: HashMap<String, String>,
    wanted: Vec<String>,
}

pub struct SyncOutcome {
    pub result: Result<RemoteSyncReport, RemoteError>,
    pub changed: bool,
    pub moved: Option<RemoteConfig>,
    pub covers: Option<CoverWork>,
}

fn with_covers(
    songs: &[RemoteSong],
    resolved: &HashMap<String, Option<String>>,
    shown: &HashMap<String, String>,
) -> Vec<RemoteSong> {
    songs
        .iter()
        .map(|song| {
            let mut song = song.clone();
            match song.cover_key.as_ref().map(|key| resolved.get(key)) {
                None => song.cover_hash = None,
                Some(Some(hash)) => song.cover_hash = hash.clone(),
                Some(None) => {
                    song.cover_hash = shown.get(&song.key).cloned();
                    song.cover_key = None;
                }
            }
            song
        })
        .collect()
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
        .and_then(|songs| {
            let resolved: HashMap<String, Option<String>> = repo
                .remote_cover_hashes(source_id)
                .map_err(|e| Failure::Local(e.into()))?
                .into_iter()
                .map(|(key, hash)| (key, Some(hash)))
                .collect();
            let mut seen = HashSet::new();
            let wanted: Vec<String> = songs
                .iter()
                .filter_map(|song| song.cover_key.as_deref())
                .filter(|key| !resolved.contains_key(*key) && seen.insert(*key))
                .map(str::to_string)
                .collect();
            let shown = if wanted.is_empty() {
                HashMap::new()
            } else {
                repo.remote_song_cover_hashes(source_id)
                    .map_err(|e| Failure::Local(e.into()))?
            };
            apply_listing(
                repo,
                source_id,
                &with_covers(&songs, &resolved, &shown),
                &[],
            )
            .map(|report| (report, songs, resolved, shown, wanted))
        });
    let moved = client.moved();
    match listed {
        Ok((report, songs, resolved, shown, wanted)) => SyncOutcome {
            changed: report.changed(),
            result: Ok(report),
            moved,
            covers: (!wanted.is_empty()).then(|| CoverWork {
                kind: config.kind(),
                client,
                source_id,
                songs,
                resolved,
                shown,
                wanted,
            }),
        },
        Err(Failure::Local(error)) => SyncOutcome {
            result: Err(error),
            changed: false,
            moved,
            covers: None,
        },
        Err(Failure::Source(error)) => {
            let changed = is_enabled(repo, source_id)
                && repo
                    .set_source_available(source_id, false)
                    .unwrap_or_else(|db| {
                        log::error!("Failed to mark source {source_id} unavailable: {db}");
                        false
                    });
            SyncOutcome {
                result: Err(error),
                changed,
                moved,
                covers: None,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlaylistImport {
    pub playlists: usize,
    pub added: usize,
    pub found: usize,
    pub total: usize,
}

pub fn import_playlists(
    repo: &dyn LibraryRepository,
    source_id: i64,
    config: &RemoteConfig,
    scope: PlaylistScope,
) -> Result<(PlaylistImport, Vec<i64>), RemoteError> {
    let remote: Vec<RemotePlaylist> = config
        .client()
        .playlists(scope)?
        .into_iter()
        .filter(|playlist| !playlist.name.trim().is_empty())
        .collect();
    let keys: Vec<String> = remote
        .iter()
        .flat_map(|playlist| playlist.keys.iter().cloned())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let items = repo.items_by_remote_key(source_id, &keys)?;
    let mut local: HashMap<String, i64> = HashMap::new();
    for playlist in repo.playlists()? {
        local.entry(playlist.name).or_insert(playlist.id);
    }
    let mut report = PlaylistImport::default();
    let mut touched = Vec::new();
    for playlist in remote {
        let mut seen = HashSet::new();
        let unique: Vec<&str> = playlist
            .keys
            .iter()
            .map(String::as_str)
            .filter(|key| seen.insert(*key))
            .collect();
        let found: Vec<i64> = unique
            .iter()
            .filter_map(|key| items.get(*key).copied())
            .collect();
        report.total += unique.len();
        report.found += found.len();
        if found.is_empty() {
            continue;
        }
        let name = playlist.name.trim();
        let (id, created) = match local.get(name) {
            Some(&id) => (id, false),
            None => {
                let id = repo.create_playlist(name)?;
                local.insert(name.to_string(), id);
                (id, true)
            }
        };
        let added = repo.add_tracks_to_playlist(id, &found)?;
        report.added += added;
        if (created || added > 0) && !touched.contains(&id) {
            touched.push(id);
        }
    }
    report.playlists = touched.len();
    Ok((report, touched))
}

pub fn fetch_covers(
    repo: &dyn LibraryRepository,
    work: CoverWork,
    progress: &dyn Fn(usize, usize),
) -> bool {
    let CoverWork {
        client,
        kind,
        source_id,
        songs,
        mut resolved,
        shown,
        wanted,
    } = work;
    let total = wanted.len();
    progress(0, total);
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let (tx, rx) = flume::bounded::<(String, Result<Vec<u8>, RemoteError>)>(COVER_FETCHERS * 2);
    let mut covers: Vec<RemoteCover> = Vec::new();
    let mut fetched = 0usize;
    std::thread::scope(|scope| {
        for _ in 0..COVER_FETCHERS.min(total) {
            let (tx, next, stop, wanted, client) = (tx.clone(), &next, &stop, &wanted, &*client);
            let spawned = std::thread::Builder::new()
                .name("cover-fetch".into())
                .spawn_scoped(scope, move || {
                    while !stop.load(Ordering::Relaxed) {
                        let Some(key) = wanted.get(next.fetch_add(1, Ordering::Relaxed)) else {
                            break;
                        };
                        let result = client.cover_art(key, music_library::thumbnail::LARGE_SIZE);
                        if tx.send((key.clone(), result)).is_err() {
                            break;
                        }
                    }
                });
            if let Err(e) = spawned {
                log::warn!("{} covers: a fetch thread did not start: {e}", kind.title());
            }
        }
        drop(tx);
        let mut thumbnailed: HashSet<String> = HashSet::new();
        let mut done = 0usize;
        let mut unreachable = 0usize;
        let mut reported = std::time::Instant::now();
        for (key, result) in rx.iter() {
            done += 1;
            let hash = match result {
                Ok(bytes) if !bytes.is_empty() => {
                    unreachable = 0;
                    let hash = music_library::sha256_hex(&bytes);
                    if thumbnailed.contains(&hash) {
                        Some(hash)
                    } else {
                        match music_library::thumbnail::generate_thumbnails(&bytes) {
                            Ok(thumbs) => {
                                thumbnailed.insert(hash.clone());
                                covers.push(RemoteCover {
                                    hash: hash.clone(),
                                    small: thumbs.small,
                                    large: thumbs.large,
                                    source_path: music_library::remote::cover_source(
                                        kind.as_str(),
                                        source_id,
                                        &key,
                                    ),
                                });
                                Some(hash)
                            }
                            Err(e) => {
                                log::warn!("{} cover {key} not decoded: {e}", kind.title());
                                None
                            }
                        }
                    }
                }
                Ok(_) => {
                    unreachable = 0;
                    None
                }
                Err(e @ (RemoteError::Unreachable(_) | RemoteError::Auth)) => {
                    unreachable += 1;
                    log::warn!("{} cover {key} not fetched: {e:?}", kind.title());
                    if unreachable >= COVER_UNREACHABLE_LIMIT {
                        log::warn!(
                            "{} source {source_id}: the server stopped answering, {} covers left for the next sync",
                            kind.title(),
                            total - done
                        );
                        break;
                    }
                    continue;
                }
                Err(e) => {
                    unreachable = 0;
                    log::warn!("{} cover {key} not fetched: {e:?}", kind.title());
                    None
                }
            };
            resolved.insert(key, hash);
            fetched += 1;
            if reported.elapsed() >= COVER_PROGRESS_EVERY {
                reported = std::time::Instant::now();
                progress(done, total);
            }
        }
        stop.store(true, Ordering::Relaxed);
        drop(rx);
        progress(done, total);
    });
    if fetched == 0 || !is_enabled(repo, source_id) {
        return false;
    }
    match apply_listing(
        repo,
        source_id,
        &with_covers(&songs, &resolved, &shown),
        &covers,
    ) {
        Ok(report) => report.changed(),
        Err(Failure::Source(error) | Failure::Local(error)) => {
            log::warn!("Server source {source_id}: storing fetched covers failed: {error:?}");
            false
        }
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

    type CoverReply = (&'static str, &'static str, Vec<u8>);

    fn stub_server() -> String {
        let cover = png();
        subsonic_stub(
            || {
                serde_json::json!([
                    {"id": "s1", "title": "Local Too", "artist": "Artist", "album": "Album",
                     "duration": 180, "suffix": "flac", "path": "x/a.flac", "coverArt": "c1"},
                    {"id": "s2", "title": "Only Remote", "artist": "Artist", "album": "Album",
                     "duration": 200, "suffix": "mp3", "path": "x/b.mp3", "coverArt": "c1"}
                ])
            },
            move |_| ("200 OK", "image/png", cover.clone()),
        )
    }

    fn subsonic_stub(
        songs: impl Fn() -> serde_json::Value + Send + 'static,
        cover: impl Fn(&str) -> CoverReply + Send + 'static,
    ) -> String {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
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
                let id = target
                    .split(['?', '&'])
                    .find_map(|pair| pair.strip_prefix("id="))
                    .unwrap_or("")
                    .replace("%3A", ":");
                let mut status = "200 OK";
                let (content_type, body) = match method.as_str() {
                    "ping" => ("application/json", json(serde_json::json!({}))),
                    "search3" if !target.contains("songOffset=0") => (
                        "application/json",
                        json(serde_json::json!({"searchResult3": {}})),
                    ),
                    "search3" => (
                        "application/json",
                        json(serde_json::json!({"searchResult3": {"song": songs()}})),
                    ),
                    "getCoverArt" => {
                        let (code, content_type, body) = cover(&id);
                        status = code;
                        (content_type, body)
                    }
                    "getStarred2" => (
                        "application/json",
                        json(serde_json::json!({"starred2": {"song": [
                            {"id": "s2", "title": "Only Remote"},
                            {"id": "not-synced", "title": "Unknown"}
                        ]}})),
                    ),
                    "getPlaylists" => (
                        "application/json",
                        json(serde_json::json!({"playlists": {"playlist": [
                            {"id": "p1", "name": "Road", "owner": "me"},
                            {"id": "p2", "name": "Dad's", "owner": "dad"},
                            {"id": "p3", "name": "Nothing here", "owner": "me"},
                            {"id": "p4", "name": "Road", "owner": "dad"}
                        ]}})),
                    ),
                    "getPlaylist" => {
                        let entries = match id.as_str() {
                            "p1" => serde_json::json!([
                                {"id": "s2"}, {"id": "s1"}, {"id": "s2"}, {"id": "not-synced"}
                            ]),
                            "p2" | "p4" => serde_json::json!([{"id": "s1"}]),
                            _ => serde_json::json!([{"id": "not-synced"}]),
                        };
                        (
                            "application/json",
                            json(serde_json::json!({"playlist": {"id": id, "entry": entries}})),
                        )
                    }
                    _ => ("application/json", json(serde_json::json!({}))),
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

    fn sync(repo: &SqliteLibrary, source_id: i64, config: &RemoteConfig) -> SyncOutcome {
        let mut outcome = sync_server(repo, source_id, config);
        if let Some(covers) = outcome.covers.take() {
            outcome.changed |= fetch_covers(repo, covers, &|_, _| {});
        }
        outcome
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
        let (configs, _) = reconcile(&repo, &servers);
        let (&source_id, _) = configs.iter().next().unwrap();

        let outcome = sync(&repo, source_id, &servers[0].config);
        let changed = outcome.changed;
        let report = outcome.result.unwrap();
        assert!(changed);
        assert_eq!((report.total, report.added, report.adopted), (2, 1, 1));
        scan_local(&repo, vec![local_track()]);

        let again = sync_server(&repo, source_id, &servers[0].config);
        assert!(!again.changed);
        assert!(again.covers.is_none());
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
    fn server_playlists_land_on_library_items_and_a_reimport_only_adds() {
        let repo = temp_db("playlists");
        repo.reconcile_local_sources(&[LocalFolder {
            path: "/music".into(),
            available: true,
        }])
        .unwrap();
        scan_local(&repo, vec![local_track()]);
        let url = stub_server();
        let servers = vec![server(&url)];
        let (configs, _) = reconcile(&repo, &servers);
        let (&source_id, _) = configs.iter().next().unwrap();
        sync(&repo, source_id, &servers[0].config).result.unwrap();
        scan_local(&repo, vec![local_track()]);
        let tracks = repo.all_tracks().unwrap();
        let id_of = |path: &str| tracks.iter().find(|t| t.path == path).unwrap().id;
        let local = id_of("/music/x/a.flac");
        let remote = id_of(&music_library::remote::locator(source_id, "s2", "mp3"));
        let entries = |playlist_id: i64| -> Vec<i64> {
            repo.tracks_for_playlist(playlist_id)
                .unwrap()
                .iter()
                .map(|t| t.id)
                .collect()
        };

        let (report, touched) =
            import_playlists(&repo, source_id, &servers[0].config, PlaylistScope::Mine).unwrap();
        assert_eq!(
            report,
            PlaylistImport {
                playlists: 1,
                added: 2,
                found: 2,
                total: 4
            }
        );
        let playlists = repo.playlists().unwrap();
        assert_eq!(playlists.len(), 1);
        let road = playlists[0].id;
        assert_eq!(playlists[0].name, "Road");
        assert_eq!(touched, vec![road]);
        assert_eq!(entries(road), vec![remote, local]);

        repo.remove_track_from_playlist(road, remote).unwrap();
        let (report, touched) =
            import_playlists(&repo, source_id, &servers[0].config, PlaylistScope::All).unwrap();
        assert_eq!(
            report,
            PlaylistImport {
                playlists: 2,
                added: 2,
                found: 4,
                total: 6
            }
        );
        let playlists = repo.playlists().unwrap();
        let names: Vec<&str> = playlists.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Road", "Dad's"]);
        let dads = playlists[1].id;
        assert_eq!(touched, vec![road, dads]);
        assert_eq!(entries(road), vec![local, remote]);
        assert_eq!(entries(dads), vec![local]);

        let (report, touched) =
            import_playlists(&repo, source_id, &servers[0].config, PlaylistScope::All).unwrap();
        assert_eq!(
            report,
            PlaylistImport {
                playlists: 0,
                added: 0,
                found: 4,
                total: 6
            }
        );
        assert!(touched.is_empty());
        assert_eq!(repo.playlists().unwrap().len(), 2);
        assert_eq!(entries(road), vec![local, remote]);
        assert_eq!(entries(dads), vec![local]);
    }

    fn song_json(id: &str, cover: &str) -> serde_json::Value {
        serde_json::json!({"id": id, "title": format!("Song {id}"), "artist": "Artist",
            "album": "Album", "duration": 180, "suffix": "mp3", "coverArt": cover})
    }

    fn jpeg(seed: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbImage::from_pixel(4, 4, image::Rgb([seed, 10, 10]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        bytes
    }

    fn subsonic_source(repo: &SqliteLibrary, url: &str) -> (i64, RemoteConfig) {
        let servers = vec![server(url)];
        let (configs, _) = reconcile(repo, &servers);
        let (&source_id, _) = configs.iter().next().unwrap();
        (source_id, servers[0].config.clone())
    }

    fn cover_of(repo: &SqliteLibrary, key: &str, source_id: i64) -> Option<i64> {
        let path = music_library::remote::locator(source_id, key, "mp3");
        repo.all_tracks()
            .unwrap()
            .into_iter()
            .find(|track| track.path == path)
            .and_then(|track| track.cover_art_id)
    }

    #[test]
    fn the_listing_lands_before_the_covers_and_every_cover_comes_through_the_fetch_threads() {
        let repo = temp_db("listing-first");
        let url = subsonic_stub(
            || {
                (0..40)
                    .map(|n| song_json(&format!("s{n}"), &format!("mf-{n}")))
                    .collect()
            },
            |id| {
                let seed: u8 = id.trim_start_matches("mf-").parse().unwrap();
                ("200 OK", "image/jpeg", jpeg(seed))
            },
        );
        let (source_id, config) = subsonic_source(&repo, &url);

        let mut outcome = sync_server(&repo, source_id, &config);
        assert!(outcome.changed);
        scan_local(&repo, vec![]);
        assert_eq!(repo.all_tracks().unwrap().len(), 40);
        assert_eq!(cover_of(&repo, "s7", source_id), None);

        let covers = outcome.covers.take().unwrap();
        let seen = std::sync::Mutex::new(Vec::new());
        assert!(fetch_covers(&repo, covers, &|done, total| seen
            .lock()
            .unwrap()
            .push((done, total))));
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.first(), Some(&(0, 40)));
        assert_eq!(seen.last(), Some(&(40, 40)));
        scan_local(&repo, vec![]);
        let tracks = repo.all_tracks().unwrap();
        assert!(tracks.iter().all(|track| track.cover_art_id.is_some()));
        assert_eq!(repo.remote_cover_hashes(source_id).unwrap().len(), 40);
        assert!(sync_server(&repo, source_id, &config).covers.is_none());
    }

    #[test]
    fn a_cover_without_a_picture_is_asked_for_again_and_does_not_hold_the_others_back() {
        let repo = temp_db("no-picture");
        let url = subsonic_stub(
            || serde_json::json!([song_json("s1", "dc-1:1"), song_json("s2", "al-2")]),
            |id| match id {
                "dc-1:1" => ("200 OK", "image/jpeg", b"not a picture".to_vec()),
                _ => ("200 OK", "image/jpeg", jpeg(1)),
            },
        );
        let (source_id, config) = subsonic_source(&repo, &url);
        sync(&repo, source_id, &config);
        scan_local(&repo, vec![]);
        assert_eq!(cover_of(&repo, "s1", source_id), None);
        assert!(cover_of(&repo, "s2", source_id).is_some());
        let again = sync_server(&repo, source_id, &config);
        assert_eq!(again.covers.unwrap().wanted, vec!["dc-1:1".to_string()]);
    }

    #[test]
    fn a_new_cover_key_keeps_the_old_cover_until_its_picture_arrives_even_across_a_restart() {
        let repo = temp_db("rekey");
        let key = std::sync::Arc::new(std::sync::Mutex::new("al-1_old".to_string()));
        let listed = key.clone();
        let url = subsonic_stub(
            move || serde_json::json!([song_json("s1", &listed.lock().unwrap())]),
            |id| match id {
                "al-1_old" => ("200 OK", "image/jpeg", jpeg(1)),
                _ => ("200 OK", "image/jpeg", jpeg(200)),
            },
        );
        let (source_id, config) = subsonic_source(&repo, &url);
        sync(&repo, source_id, &config);
        scan_local(&repo, vec![]);
        let old = cover_of(&repo, "s1", source_id).unwrap();

        *key.lock().unwrap() = "al-1_new".to_string();
        let interrupted = sync_server(&repo, source_id, &config);
        assert!(!interrupted.changed);
        scan_local(&repo, vec![]);
        assert_eq!(cover_of(&repo, "s1", source_id), Some(old));

        let mut outcome = sync_server(&repo, source_id, &config);
        assert_eq!(
            outcome.covers.as_ref().unwrap().wanted,
            vec!["al-1_new".to_string()]
        );
        assert!(fetch_covers(
            &repo,
            outcome.covers.take().unwrap(),
            &|_, _| {}
        ));
        scan_local(&repo, vec![]);
        let new = cover_of(&repo, "s1", source_id).unwrap();
        assert_ne!(new, old);
        assert!(sync_server(&repo, source_id, &config).covers.is_none());
    }

    #[test]
    fn covers_stop_after_the_server_keeps_failing() {
        let repo = temp_db("cover-outage");
        let requests = std::sync::Arc::new(AtomicUsize::new(0));
        let counted = requests.clone();
        let url = subsonic_stub(
            || {
                (0..200)
                    .map(|n| song_json(&format!("s{n}"), &format!("mf-{n}")))
                    .collect()
            },
            move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
                ("503 Service Unavailable", "text/plain", Vec::new())
            },
        );
        let (source_id, config) = subsonic_source(&repo, &url);
        let mut outcome = sync_server(&repo, source_id, &config);
        assert!(!fetch_covers(
            &repo,
            outcome.covers.take().unwrap(),
            &|_, _| {}
        ));
        assert!(requests.load(Ordering::Relaxed) < 50);
        assert_eq!(
            sync_server(&repo, source_id, &config)
                .covers
                .unwrap()
                .wanted
                .len(),
            200
        );
    }

    #[test]
    fn a_failed_sync_of_a_removed_source_changes_nothing() {
        let repo = temp_db("removed-failed");
        let dead = format!("http://127.0.0.1:{}", {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.local_addr().unwrap().port()
        });
        let servers = vec![server(&dead)];
        let (configs, _) = reconcile(&repo, &servers);
        let (&source_id, _) = configs.iter().next().unwrap();

        let live = sync(&repo, source_id, &servers[0].config);
        assert!(live.result.is_err());
        assert!(live.changed);

        repo.set_source_available(source_id, true).unwrap();
        reconcile(&repo, &[]);
        let removed = sync(&repo, source_id, &servers[0].config);
        assert!(removed.result.is_err());
        assert!(!removed.changed);
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
        let (configs, _) = reconcile(&repo, &servers);
        assert_eq!(configs.len(), 2);
        let (&source_id, config) = configs
            .iter()
            .find(|(_, config)| config.kind() == ServerKind::Jellyfin)
            .unwrap();

        let outcome = sync(&repo, source_id, config);
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
        let (&source_id, _) = reconcile(&repo, &servers).0.iter().next().unwrap();

        let outcome = sync(&repo, source_id, &servers[0].config);
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
