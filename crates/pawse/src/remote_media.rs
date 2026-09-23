use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;

use audio_engine::TrackResolver;
use music_library::remote;

const CACHE_LIMIT_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Clone)]
pub struct RemoteMedia {
    configs: Arc<RwLock<HashMap<i64, subsonic::Config>>>,
    cache_dir: PathBuf,
    download_lock: Arc<Mutex<()>>,
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
            download_lock: Arc::new(Mutex::new(())),
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

    pub fn prefetch(&self, locator: &str) {
        if !remote::is_remote(locator) {
            return;
        }
        let media = self.clone();
        let locator = PathBuf::from(locator);
        std::thread::spawn(move || {
            if let Err(e) = media.resolve(&locator) {
                log::warn!("prefetch of {} failed: {e}", locator.display());
            }
        });
    }

    pub fn resolve(&self, path: &Path) -> Result<PathBuf, String> {
        if let Some(local) = self.cached(path) {
            return Ok(local);
        }
        let text = path.to_string_lossy();
        let Some(reference) = remote::parse(&text) else {
            return Ok(path.to_path_buf());
        };
        let dest = self.cached_path(&reference);
        let config = self
            .config(reference.source_id)
            .ok_or_else(|| "the server for this track is not configured".to_string())?;
        let _guard = self.download_lock.lock().unwrap();
        if dest.exists() {
            return Ok(dest);
        }
        subsonic::Client::new(&config)
            .download_to(&reference.key, &dest)
            .map_err(|e| e.to_string())?;
        trim_cache(&self.cache_dir, CACHE_LIMIT_BYTES, &dest);
        Ok(dest)
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
            if meta.is_file() {
                let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                files.push((modified, meta.len(), entry.path()));
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
        assert_eq!(media.resolve(path).unwrap(), path);
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
        assert!(media.resolve(Path::new(&locator)).is_err());
        assert!((media.resolver())(Path::new(&locator)).is_err());

        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, b"x").unwrap();
        assert_eq!(media.resolve(Path::new(&locator)).unwrap(), cached);
        assert_eq!((media.resolver())(Path::new(&locator)).unwrap(), cached);
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
