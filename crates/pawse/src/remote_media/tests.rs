use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pawse-remote-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn media(dir: &Path) -> RemoteMedia {
    RemoteMedia::with_cache(CacheStore::new(dir.to_path_buf()))
}

fn serve_file(bytes: Vec<u8>) -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let mut range = None;
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).is_err() || header == "\r\n" || header.is_empty() {
                    break;
                }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("range: bytes=") {
                    range = Some(value.trim().to_string());
                }
            }
            let total = bytes.len();
            let (start, end) = range
                .as_deref()
                .and_then(|r| r.split_once('-'))
                .map(|(a, b)| {
                    (
                        a.parse::<usize>().unwrap(),
                        b.parse::<usize>().map_or(total, |b| (b + 1).min(total)),
                    )
                })
                .unwrap_or((0, total));
            let head = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{}/{total}\r\nConnection: close\r\n\r\n",
                end - start,
                end - 1
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&bytes[start..end]);
        }
    });
    url
}

fn fixture(name: &str) -> Vec<u8> {
    let root = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    std::fs::read(PathBuf::from(root).join("../../fixtures").join(name)).unwrap()
}

fn subsonic(url: String) -> RemoteConfig {
    RemoteConfig::Subsonic(subsonic::Config {
        url,
        username: "u".into(),
        password: "p".into(),
    })
}

fn jellyfin(url: String) -> RemoteConfig {
    RemoteConfig::Jellyfin(jellyfin::Config {
        url,
        user_id: "u".into(),
        token: "t".into(),
        device_id: "d".into(),
    })
}

fn wait_for(path: &Path) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !path.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    path.exists()
}

fn cache_path(media: &RemoteMedia, locator: &str) -> PathBuf {
    media.cache.path_for(&remote::parse(locator).unwrap())
}

#[test]
fn local_paths_pass_through_untouched() {
    let media = media(&temp_dir("pass"));
    let path = Path::new("/music/a.flac");
    assert_eq!(media.resolve(path, &|| false).unwrap(), path);
    assert_eq!(media.cached(path), Some(path.to_path_buf()));
}

#[test]
fn a_broken_locator_is_never_taken_for_a_local_file() {
    let media = media(&temp_dir("broken"));
    let broken = Path::new("pawse-source://x/k.flac");
    assert_eq!(media.cached(broken), None);
    assert!(media.resolve(broken, &|| false).is_err());
}

#[test]
fn a_cached_track_resolves_without_a_server_and_an_unknown_server_is_an_error() {
    let dir = temp_dir("cached");
    let media = media(&dir);
    let locator = remote::locator(3, "abc/1", "flac");
    let cached = cache_path(&media, &locator);
    assert!(media.resolve(Path::new(&locator), &|| false).is_err());
    assert!((media.resolver())(Path::new(&locator)).is_err());
    assert!(media.ping(3).is_none());

    std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
    std::fs::write(&cached, b"x").unwrap();
    assert_eq!(
        media.resolve(Path::new(&locator), &|| false).unwrap(),
        cached
    );
    assert_eq!((media.resolver())(Path::new(&locator)).unwrap(), cached);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn server_tracks_of_both_kinds_stream_and_end_up_in_the_cache() {
    let bytes = fixture("sine_440_16_44_stereo.wav");
    let dir = temp_dir("stream");
    let media = media(&dir);
    media.set_servers(HashMap::from([
        (5, subsonic(serve_file(bytes.clone()))),
        (6, jellyfin(serve_file(bytes.clone()))),
    ]));
    for locator in [
        remote::locator(5, "song-1", "wav"),
        remote::locator(6, "0f1e2d3c", "wav"),
    ] {
        let path = Path::new(&locator);
        assert!(RemoteMedia::can_stream(path));
        assert!(media.cached(path).is_none());
        let mut pending = media.open_stream(path).unwrap();
        assert_eq!(pending.extension, "wav");
        let mut streamed = Vec::new();
        pending.stream.read_to_end(&mut streamed).unwrap();
        assert_eq!(streamed, bytes);
        drop(pending);
        let cached = cache_path(&media, &locator);
        assert!(wait_for(&cached), "{locator}");
        assert_eq!(std::fs::read(&cached).unwrap(), bytes);
        assert_eq!(media.cached(path), Some(cached));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_streamed_track_decodes_through_the_engine_source() {
    let dir = temp_dir("decode");
    let media = media(&dir);
    media.set_servers(HashMap::from([(
        5,
        subsonic(serve_file(fixture("1khz_16_44_1.wav"))),
    )]));
    let locator = remote::locator(5, "song-2", "wav");
    let pending = media.open_stream(Path::new(&locator)).unwrap();
    let control = pending.control.clone();
    let source = audio_engine::StreamingSource::open(
        pending.stream,
        Some(pending.extension),
        Box::new(move || control.abort()),
    )
    .unwrap();
    assert_eq!(source.params().sample_rate, 44_100);
    assert!(
        source
            .duration()
            .is_some_and(|d| (d.as_secs_f64() - 2.0).abs() < 0.01)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prefetch_fills_the_cache_and_formats_that_cannot_stream_are_downloaded_whole() {
    let bytes = fixture("sine_440_16_44_mono.wav");
    let dir = temp_dir("prefetch");
    let media = media(&dir);
    media.set_servers(HashMap::from([(5, subsonic(serve_file(bytes.clone())))]));
    let locator = remote::locator(5, "song-3", "flac");
    media.prefetch(&locator);
    assert!(wait_for(&cache_path(&media, &locator)));

    let ape = remote::locator(5, "song-4", "ape");
    assert!(!RemoteMedia::can_stream(Path::new(&ape)));
    let whole = media.resolve(Path::new(&ape), &|| false).unwrap();
    assert_eq!(std::fs::read(whole).unwrap(), bytes);
    let _ = std::fs::remove_dir_all(&dir);
}

struct Fake {
    opened: AtomicUsize,
}

impl SourceMedia for Fake {
    fn ping(&self) -> Result<(), RemoteError> {
        Err(RemoteError::Unreachable("no peers".into()))
    }

    fn open(&self, _: &RemoteRef, _: &Path) -> Result<PendingStream, String> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        Err("fake".into())
    }

    fn fetch_whole(
        &self,
        _: &RemoteRef,
        dest: &Path,
        _: &dyn Fn() -> bool,
    ) -> Result<PathBuf, String> {
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(dest, b"whole").unwrap();
        Ok(dest.to_path_buf())
    }

    fn prefetch(&self, _: &RemoteRef, _: &Path) -> Result<KeepAlive, String> {
        Ok(Box::new(()))
    }
}

#[test]
fn any_source_media_plugs_in_by_source_id_and_writes_into_the_shared_cache() {
    let dir = temp_dir("fake");
    let media = media(&dir);
    let fake = Arc::new(Fake {
        opened: AtomicUsize::new(0),
    });
    media.set_sources(HashMap::from([(9, fake.clone() as Arc<dyn SourceMedia>)]));
    assert_eq!(
        media.ping(9),
        Some(Err(RemoteError::Unreachable("no peers".into())))
    );
    let locator = remote::locator(9, "file-1", "ape");
    assert!(media.open_stream(Path::new(&locator)).is_err());
    assert_eq!(fake.opened.load(Ordering::SeqCst), 1);
    let whole = media.resolve(Path::new(&locator), &|| false).unwrap();
    assert!(whole.starts_with(dir.join("9")));
    assert_eq!(media.cached(Path::new(&locator)), Some(whole));
    assert!(
        media
            .open_stream(Path::new(&remote::locator(8, "x", "flac")))
            .is_err()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
