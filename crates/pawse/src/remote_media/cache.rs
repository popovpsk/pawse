use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use media_stream::{Downloads, OnComplete};
use music_library::remote::RemoteRef;

const PARTIAL_GRACE: Duration = Duration::from_secs(24 * 60 * 60);
const DIR_NAME: &str = "media";
const LEGACY_DIR_NAMES: [&str; 1] = ["subsonic"];

pub struct CacheStore {
    dir: PathBuf,
    downloads: Downloads,
    limit: Arc<AtomicU64>,
}

impl CacheStore {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            downloads: Downloads::default(),
            limit: Arc::new(AtomicU64::new(u64::MAX)),
        }
    }

    pub fn in_app_cache(base: &Path) -> Self {
        Self::new(media_dir(base))
    }

    pub fn downloads(&self) -> &Downloads {
        &self.downloads
    }

    pub fn path_for(&self, reference: &RemoteRef) -> PathBuf {
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
        self.dir
            .join(reference.source_id.to_string())
            .join(format!("{safe_key}-{digest}.{}", reference.suffix))
    }

    pub fn contains(&self, reference: &RemoteRef) -> bool {
        self.path_for(reference).exists()
    }

    pub fn lookup(&self, reference: &RemoteRef) -> Option<PathBuf> {
        let path = self.path_for(reference);
        path.exists().then(|| {
            touch(&path);
            path
        })
    }

    pub fn trim_on_complete(&self) -> OnComplete {
        let dir = self.dir.clone();
        let limit = self.limit.clone();
        Box::new(move |done: &Path| trim(&dir, limit.load(Ordering::Acquire), done))
    }

    pub fn size(&self) -> u64 {
        files(&self.dir).iter().map(|(_, len, _)| len).sum()
    }

    pub fn set_limit(&self, bytes: u64) {
        if self.limit.swap(bytes, Ordering::AcqRel) > bytes {
            let dir = self.dir.clone();
            std::thread::spawn(move || trim(&dir, bytes, Path::new("")));
        }
    }

    pub fn clear(&self) {
        for (modified, _, path) in files(&self.dir) {
            if !fresh_partial(modified, &path) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn media_dir(base: &Path) -> PathBuf {
    let dir = base.join(DIR_NAME);
    if !dir.exists()
        && let Some(legacy) = LEGACY_DIR_NAMES
            .iter()
            .map(|name| base.join(name))
            .find(|legacy| legacy.is_dir())
        && let Err(e) = std::fs::rename(&legacy, &dir)
    {
        log::warn!("Could not move the media cache from {legacy:?}: {e}");
    }
    dir
}

fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().append(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

fn files(dir: &Path) -> Vec<(SystemTime, u64, PathBuf)> {
    let mut files = Vec::new();
    let Ok(sources) = std::fs::read_dir(dir) else {
        return files;
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
    files
}

fn fresh_partial(modified: SystemTime, path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "partial")
        && modified.elapsed().is_ok_and(|age| age < PARTIAL_GRACE)
}

fn trim(dir: &Path, limit: u64, keep: &Path) {
    let mut files = files(dir);
    files.retain(|(modified, _, path)| !fresh_partial(*modified, path));
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
        let dir = std::env::temp_dir().join(format!("pawse-cache-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn aged(path: &Path, age: Duration) {
        std::fs::write(path, vec![0u8; 10]).unwrap();
        let file = std::fs::File::options().append(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
    }

    fn reference(source_id: i64, key: &str, suffix: &str) -> RemoteRef {
        RemoteRef {
            source_id,
            key: key.into(),
            suffix: suffix.into(),
        }
    }

    #[test]
    fn paths_are_per_source_safe_and_distinct_for_keys_that_sanitize_alike() {
        let store = CacheStore::new(PathBuf::from("/c"));
        let a = store.path_for(&reference(3, "abc/1", "flac"));
        let b = store.path_for(&reference(3, "abc_1", "flac"));
        assert!(a.starts_with("/c/3"));
        assert_eq!(a.parent(), b.parent());
        assert_ne!(a, b);
        assert!(a.extension().is_some_and(|e| e == "flac"));
        assert!(!a.file_name().unwrap().to_string_lossy().contains('/'));
    }

    #[test]
    fn the_old_subsonic_cache_is_moved_not_orphaned() {
        let base = temp_dir("legacy");
        let old = base.join("subsonic").join("4");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("a.flac"), b"x").unwrap();
        let store = CacheStore::in_app_cache(&base);
        assert!(base.join("media").join("4").join("a.flac").exists());
        assert!(!base.join("subsonic").exists());
        assert_eq!(store.size(), 1);

        std::fs::create_dir_all(base.join("subsonic")).unwrap();
        let _ = CacheStore::in_app_cache(&base);
        assert!(base.join("subsonic").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn trimming_leaves_downloads_in_progress_alone() {
        let dir = temp_dir("trim-partial");
        let sub = dir.join("1");
        std::fs::create_dir_all(&sub).unwrap();
        let partial = sub.join("a.flac.1-1.partial");
        let stale = sub.join("b.flac.1-2.partial");
        let done = sub.join("c.flac");
        aged(&partial, Duration::ZERO);
        aged(&stale, PARTIAL_GRACE * 2);
        aged(&done, Duration::ZERO);
        trim(&dir, 5, &done);
        assert!(partial.exists());
        assert!(!stale.exists());
        assert!(done.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clearing_keeps_only_downloads_in_progress_and_reports_the_size() {
        let dir = temp_dir("clear");
        let store = CacheStore::new(dir.clone());
        let sub = dir.join("2");
        std::fs::create_dir_all(&sub).unwrap();
        let partial = sub.join("a.flac.1-1.partial");
        let stale = sub.join("b.flac.1-2.partial");
        let done = sub.join("c.flac");
        aged(&partial, Duration::ZERO);
        aged(&stale, PARTIAL_GRACE * 2);
        aged(&done, Duration::ZERO);
        assert_eq!(store.size(), 30);
        store.clear();
        assert!(partial.exists());
        assert!(!stale.exists());
        assert!(!done.exists());
        assert_eq!(store.size(), 10);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lowering_the_limit_trims_the_cache_right_away() {
        let dir = temp_dir("limit");
        let store = CacheStore::new(dir.clone());
        store.set_limit(100);
        let sub = dir.join("3");
        std::fs::create_dir_all(&sub).unwrap();
        let old = sub.join("old.flac");
        let new = sub.join("new.flac");
        aged(&old, Duration::from_secs(200));
        aged(&new, Duration::from_secs(100));
        store.set_limit(15);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while old.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!old.exists());
        assert!(new.exists());
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
        aged(&old, Duration::from_secs(300));
        aged(&keep, Duration::from_secs(200));
        aged(&newer, Duration::from_secs(100));
        trim(&dir, 15, &keep);
        assert!(!old.exists());
        assert!(keep.exists());
        assert!(!newer.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lookup_refreshes_the_file_so_it_is_trimmed_last() {
        let dir = temp_dir("touch");
        let store = CacheStore::new(dir.clone());
        let hit = reference(1, "hit", "flac");
        let path = store.path_for(&hit);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        aged(&path, Duration::from_secs(1000));
        let other = dir.join("1").join("other.flac");
        aged(&other, Duration::from_secs(10));
        assert_eq!(store.lookup(&hit), Some(path.clone()));
        assert_eq!(store.lookup(&reference(1, "miss", "flac")), None);
        trim(&dir, 15, Path::new(""));
        assert!(path.exists());
        assert!(!other.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
