use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use audio_engine::TrackResolver;
use media_stream::{
    AbortHandle, Download, Downloads, FetchError, Fetched, RangeFetch, StreamReader,
};
use music_library::remote;

const CACHE_LIMIT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const PARTIAL_GRACE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone)]
pub struct RemoteMedia {
    configs: Arc<RwLock<HashMap<i64, subsonic::Config>>>,
    cache_dir: PathBuf,
    downloads: Downloads,
    prefetching: Arc<Mutex<Option<Download>>>,
}

pub struct PendingStream {
    pub stream: RemoteStream,
    pub abort: AbortHandle,
    pub extension: String,
}

pub struct RemoteStream(StreamReader);

impl Read for RemoteStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for RemoteStream {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.0.seek(pos)
    }
}

impl audio_engine::MediaStream for RemoteStream {
    fn byte_len(&self) -> Option<u64> {
        self.0.byte_len()
    }
}

struct SubsonicFetch {
    client: subsonic::Client,
    song_id: String,
}

impl RangeFetch for SubsonicFetch {
    fn fetch(&self, start: u64, end: Option<u64>) -> Result<Fetched, FetchError> {
        match self.client.fetch_range(&self.song_id, start, end) {
            Ok(range) => Ok(Fetched {
                body: range.body,
                offset: range.offset,
                total: range.total,
                ranged: range.ranged,
            }),
            Err(e @ subsonic::Error::Transient(_)) => Err(FetchError::Retry(e.to_string())),
            Err(e) => Err(FetchError::Fatal(e.to_string())),
        }
    }
}

impl Default for RemoteMedia {
    fn default() -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("pawse")
            .join("subsonic");
        Self::with_cache_dir(cache_dir)
    }
}

impl RemoteMedia {
    pub fn with_cache_dir(cache_dir: PathBuf) -> Self {
        Self {
            configs: Arc::new(RwLock::new(HashMap::new())),
            cache_dir,
            downloads: Downloads::default(),
            prefetching: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_configs(&self, configs: HashMap<i64, subsonic::Config>) {
        *self.configs.write().unwrap() = configs;
    }

    pub fn config(&self, source_id: i64) -> Option<subsonic::Config> {
        self.configs.read().unwrap().get(&source_id).cloned()
    }

    pub fn resolver(&self) -> TrackResolver {
        let media = self.clone();
        Arc::new(move |path: &Path| match media.cached(path) {
            Some(local) => Ok(local),
            None => Err("the track is not downloaded yet".to_string()),
        })
    }

    pub fn cached(&self, path: &Path) -> Option<PathBuf> {
        let text = path.to_string_lossy();
        let Some(reference) = remote::parse(&text) else {
            return Some(path.to_path_buf());
        };
        let dest = self.cached_path(&reference);
        dest.exists().then(|| {
            touch(&dest);
            dest
        })
    }

    pub fn can_stream(path: &Path) -> bool {
        remote::parse(&path.to_string_lossy())
            .is_some_and(|reference| audio_engine::can_stream(&reference.suffix))
    }

    pub fn prefetch(&self, locator: &str) {
        let Some(reference) = remote::parse(locator) else {
            return;
        };
        if self.cached(Path::new(locator)).is_some() {
            return;
        }
        match self.download(&reference) {
            Ok(download) => *self.prefetching.lock().unwrap() = Some(download),
            Err(e) => log::warn!("prefetch of {locator} failed: {e}"),
        }
    }

    pub fn resolve(&self, path: &Path, abandoned: &dyn Fn() -> bool) -> Result<PathBuf, String> {
        if let Some(local) = self.cached(path) {
            return Ok(local);
        }
        let reference = remote::parse(&path.to_string_lossy())
            .ok_or_else(|| "not a server track".to_string())?;
        self.download(&reference)?.wait_while(abandoned)
    }

    pub fn open_stream(&self, path: &Path) -> Result<PendingStream, String> {
        let reference = remote::parse(&path.to_string_lossy())
            .ok_or_else(|| "not a server track".to_string())?;
        let reader = self
            .download(&reference)?
            .reader()
            .map_err(|e| e.to_string())?;
        Ok(PendingStream {
            abort: reader.abort_handle(),
            stream: RemoteStream(reader),
            extension: reference.suffix,
        })
    }

    fn download(&self, reference: &remote::RemoteRef) -> Result<Download, String> {
        let config = self
            .config(reference.source_id)
            .ok_or_else(|| "the server for this track is not configured".to_string())?;
        let fetch = Arc::new(SubsonicFetch {
            client: subsonic::Client::new(&config),
            song_id: reference.key.clone(),
        });
        let cache_dir = self.cache_dir.clone();
        self.downloads
            .start(
                &self.cached_path(reference),
                fetch,
                Some(Box::new(move |done: &Path| {
                    trim_cache(&cache_dir, CACHE_LIMIT_BYTES, done)
                })),
            )
            .map_err(|e| e.to_string())
    }

    fn cached_path(&self, reference: &remote::RemoteRef) -> PathBuf {
        let safe_key: String = reference
            .key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let digest = &music_library::sha256_hex(reference.key.as_bytes())[..12];
        self.cache_dir
            .join(reference.source_id.to_string())
            .join(format!("{safe_key}-{digest}.{}", reference.suffix))
    }
}

fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().append(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

fn trim_cache(dir: &Path, limit: u64, keep: &Path) {
    let mut files: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    let Ok(sources) = std::fs::read_dir(dir) else {
        return;
    };
    for source in sources.flatten() {
        let Ok(entries) = std::fs::read_dir(source.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let path = entry.path();
            let fresh_partial = path.extension().is_some_and(|ext| ext == "partial")
                && modified.elapsed().is_ok_and(|age| age < PARTIAL_GRACE);
            if !fresh_partial {
                files.push((modified, meta.len(), path));
            }
        }
    }
    let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
    if total <= limit {
        return;
    }
    files.sort();
    for (_, len, path) in files {
        if total <= limit {
            break;
        }
        if path == keep {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pawse-remote-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn local_paths_pass_through_untouched() {
        let media = RemoteMedia::with_cache_dir(temp_dir("pass"));
        let path = Path::new("/music/a.flac");
        assert_eq!(media.resolve(path, &|| false).unwrap(), path);
    }

    #[test]
    fn a_cached_track_resolves_without_a_server_and_an_unknown_server_is_an_error() {
        let dir = temp_dir("cached");
        let media = RemoteMedia::with_cache_dir(dir.clone());
        let locator = remote::locator(3, "abc/1", "flac");
        let reference = remote::parse(&locator).unwrap();
        let cached = media.cached_path(&reference);
        assert!(cached.starts_with(dir.join("3")));
        assert!(cached.extension().is_some_and(|e| e == "flac"));
        assert!(media.resolve(Path::new(&locator), &|| false).is_err());
        assert!((media.resolver())(Path::new(&locator)).is_err());

        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, b"x").unwrap();
        assert_eq!(
            media.resolve(Path::new(&locator), &|| false).unwrap(),
            cached
        );
        assert_eq!((media.resolver())(Path::new(&locator)).unwrap(), cached);
        let _ = std::fs::remove_dir_all(&dir);
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
                    if reader.read_line(&mut header).is_err()
                        || header == "\r\n"
                        || header.is_empty()
                    {
                        break;
                    }
                    if let Some(value) = header.strip_prefix("range: bytes=") {
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

    fn media_with_server(name: &str, bytes: Vec<u8>) -> (RemoteMedia, PathBuf) {
        let dir = temp_dir(name);
        let media = RemoteMedia::with_cache_dir(dir.clone());
        media.set_configs(HashMap::from([(
            5,
            subsonic::Config {
                url: serve_file(bytes),
                username: "u".into(),
                password: "p".into(),
            },
        )]));
        (media, dir)
    }

    fn wait_for(path: &Path) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        path.exists()
    }

    #[test]
    fn a_server_track_streams_and_ends_up_in_the_cache() {
        let bytes = fixture("sine_440_16_44_stereo.wav");
        let (media, dir) = media_with_server("stream", bytes.clone());
        let locator = remote::locator(5, "song-1", "wav");
        let path = Path::new(&locator);
        assert!(RemoteMedia::can_stream(path));
        assert!(media.cached(path).is_none());

        let mut pending = media.open_stream(path).unwrap();
        assert_eq!(pending.extension, "wav");
        let mut streamed = Vec::new();
        pending.stream.read_to_end(&mut streamed).unwrap();
        assert_eq!(streamed, bytes);
        drop(pending);

        let cached = media.cached_path(&remote::parse(&locator).unwrap());
        assert!(wait_for(&cached));
        assert_eq!(std::fs::read(&cached).unwrap(), bytes);
        assert_eq!(media.cached(path), Some(cached));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_streamed_track_decodes_through_the_engine_source() {
        let (media, dir) = media_with_server("decode", fixture("1khz_16_44_1.wav"));
        let locator = remote::locator(5, "song-2", "wav");
        let pending = media.open_stream(Path::new(&locator)).unwrap();
        let abort = pending.abort.clone();
        let source = audio_engine::StreamingSource::open(
            Box::new(pending.stream),
            Some(pending.extension),
            Box::new(move || abort.abort()),
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
        let (media, dir) = media_with_server("prefetch", bytes.clone());
        let locator = remote::locator(5, "song-3", "flac");
        media.prefetch(&locator);
        let cached = media.cached_path(&remote::parse(&locator).unwrap());
        assert!(wait_for(&cached));

        let ape = remote::locator(5, "song-4", "ape");
        assert!(!RemoteMedia::can_stream(Path::new(&ape)));
        let whole = media.resolve(Path::new(&ape), &|| false).unwrap();
        assert_eq!(std::fs::read(whole).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trimming_leaves_downloads_in_progress_alone() {
        let dir = temp_dir("trim-partial");
        let sub = dir.join("1");
        std::fs::create_dir_all(&sub).unwrap();
        let partial = sub.join("a.flac.1-1.partial");
        let stale = sub.join("b.flac.1-2.partial");
        let done = sub.join("c.flac");
        for path in [&partial, &stale, &done] {
            std::fs::write(path, vec![0u8; 10]).unwrap();
        }
        let file = std::fs::File::options().append(true).open(&stale).unwrap();
        file.set_modified(SystemTime::now() - PARTIAL_GRACE * 2)
            .unwrap();
        trim_cache(&dir, 5, &done);
        assert!(partial.exists());
        assert!(!stale.exists());
        assert!(done.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trimming_drops_the_oldest_files_but_never_the_one_just_fetched() {
        let dir = temp_dir("trim");
        let sub = dir.join("1");
        std::fs::create_dir_all(&sub).unwrap();
        let old = sub.join("old.flac");
        let newer = sub.join("newer.flac");
        let keep = sub.join("keep.flac");
        for (path, age) in [(&old, 300), (&keep, 200), (&newer, 100)] {
            std::fs::write(path, vec![0u8; 10]).unwrap();
            let file = std::fs::File::options().append(true).open(path).unwrap();
            file.set_modified(SystemTime::now() - std::time::Duration::from_secs(age))
                .unwrap();
        }
        trim_cache(&dir, 15, &keep);
        assert!(!old.exists());
        assert!(keep.exists());
        assert!(!newer.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
