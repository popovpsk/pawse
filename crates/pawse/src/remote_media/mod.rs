use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use audio_engine::TrackResolver;
use music_library::remote::{self, Location, RemoteRef};

use crate::servers::{RemoteConfig, RemoteError};

mod cache;
mod http;

pub use cache::CacheStore;
pub use http::HttpMedia;

pub trait StreamControl: Send + Sync {
    fn abort(&self);
    fn failure(&self) -> Option<String>;
}

impl StreamControl for media_stream::AbortHandle {
    fn abort(&self) {
        media_stream::AbortHandle::abort(self)
    }

    fn failure(&self) -> Option<String> {
        media_stream::AbortHandle::failure(self)
    }
}

pub struct PendingStream {
    pub stream: Box<dyn audio_engine::MediaStream>,
    pub control: Arc<dyn StreamControl>,
    pub extension: String,
}

pub type KeepAlive = Box<dyn Send>;

pub trait SourceMedia: Send + Sync {
    fn ping(&self) -> Result<(), RemoteError>;
    fn open(&self, reference: &RemoteRef, dest: &Path) -> Result<PendingStream, String>;
    fn fetch_whole(
        &self,
        reference: &RemoteRef,
        dest: &Path,
        abandoned: &dyn Fn() -> bool,
    ) -> Result<PathBuf, String>;
    fn prefetch(&self, reference: &RemoteRef, dest: &Path) -> Result<KeepAlive, String>;
}

type Sources = HashMap<i64, Arc<dyn SourceMedia>>;

#[derive(Clone)]
pub struct RemoteMedia {
    sources: Arc<RwLock<Sources>>,
    cache: Arc<CacheStore>,
    prefetching: Arc<Mutex<Option<KeepAlive>>>,
}

impl Default for RemoteMedia {
    fn default() -> Self {
        let base = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("pawse");
        Self::with_cache(CacheStore::in_app_cache(&base))
    }
}

impl RemoteMedia {
    pub fn with_cache(cache: CacheStore) -> Self {
        Self {
            sources: Arc::new(RwLock::new(HashMap::new())),
            cache: Arc::new(cache),
            prefetching: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_servers(&self, configs: HashMap<i64, RemoteConfig>) {
        let sources: Sources = configs
            .into_iter()
            .map(|(source_id, config)| {
                let media: Arc<dyn SourceMedia> =
                    Arc::new(HttpMedia::new(config.client(), self.cache.clone()));
                (source_id, media)
            })
            .collect();
        self.set_sources(sources);
    }

    pub fn set_sources(&self, sources: Sources) {
        *self.sources.write().unwrap() = sources;
    }

    fn source(&self, reference: &RemoteRef) -> Result<Arc<dyn SourceMedia>, String> {
        self.sources
            .read()
            .unwrap()
            .get(&reference.source_id)
            .cloned()
            .ok_or_else(|| "the server for this track is not configured".to_string())
    }

    pub fn ping(&self, source_id: i64) -> Option<Result<(), RemoteError>> {
        let source = self.sources.read().unwrap().get(&source_id).cloned()?;
        Some(source.ping())
    }

    pub fn resolver(&self) -> TrackResolver {
        let media = self.clone();
        Arc::new(move |path: &Path| match media.cached(path) {
            Some(local) => Ok(local),
            None => Err("the track is not downloaded yet".to_string()),
        })
    }

    pub fn cached(&self, path: &Path) -> Option<PathBuf> {
        match remote::location(&path.to_string_lossy()) {
            Location::File(_) => Some(path.to_path_buf()),
            Location::Remote(reference) => self.cache.lookup(&reference),
            Location::Invalid => None,
        }
    }

    pub fn is_cached(&self, path: &Path) -> bool {
        match remote::location(&path.to_string_lossy()) {
            Location::File(_) => true,
            Location::Remote(reference) => self.cache.contains(&reference),
            Location::Invalid => false,
        }
    }

    pub fn cache_size(&self) -> u64 {
        self.cache.size()
    }

    pub fn set_cache_limit(&self, bytes: u64) {
        self.cache.set_limit(bytes);
    }

    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    pub fn can_stream(path: &Path) -> bool {
        remote::parse(&path.to_string_lossy())
            .is_some_and(|reference| audio_engine::can_stream(&reference.suffix))
    }

    pub fn prefetch(&self, locator: &str) {
        let Some(reference) = remote::parse(locator) else {
            return;
        };
        if self.cache.lookup(&reference).is_some() {
            return;
        }
        let started = self
            .source(&reference)
            .and_then(|source| source.prefetch(&reference, &self.cache.path_for(&reference)));
        match started {
            Ok(keep) => *self.prefetching.lock().unwrap() = Some(keep),
            Err(e) => log::warn!("prefetch of {locator} failed: {e}"),
        }
    }

    pub fn resolve(&self, path: &Path, abandoned: &dyn Fn() -> bool) -> Result<PathBuf, String> {
        if let Some(local) = self.cached(path) {
            return Ok(local);
        }
        let reference = remote::parse(&path.to_string_lossy())
            .ok_or_else(|| "not a server track".to_string())?;
        self.source(&reference)?.fetch_whole(
            &reference,
            &self.cache.path_for(&reference),
            abandoned,
        )
    }

    pub fn open_stream(&self, path: &Path) -> Result<PendingStream, String> {
        let reference = remote::parse(&path.to_string_lossy())
            .ok_or_else(|| "not a server track".to_string())?;
        self.source(&reference)?
            .open(&reference, &self.cache.path_for(&reference))
    }
}

#[cfg(test)]
mod tests;
