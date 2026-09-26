use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use music_library::RemoteSong;

use super::{Peers, RemoteError, ServerClient};

mod index;

const READ_TIMEOUT: Duration = Duration::from_secs(20);
const RETRY_INDEX_AFTER: Duration = Duration::from_secs(10 * 60);

pub struct TorrentHost {
    config: Mutex<::torrent::Config>,
    engine: OnceLock<Option<::torrent::Engine>>,
    state: Mutex<()>,
    failed_index: Mutex<HashMap<String, (Instant, String)>>,
}

impl TorrentHost {
    pub fn new(config: ::torrent::Config) -> Arc<Self> {
        Arc::new(Self {
            config: Mutex::new(config),
            engine: OnceLock::new(),
            state: Mutex::new(()),
            failed_index: Mutex::new(HashMap::new()),
        })
    }

    pub fn engine(&self) -> Option<&::torrent::Engine> {
        self.engine
            .get_or_init(|| {
                let config = self.config.lock().unwrap().clone();
                ::torrent::Engine::new(config)
                    .inspect_err(|e| log::error!("Torrents are unavailable: {e}"))
                    .ok()
            })
            .as_ref()
    }

    pub fn running_engine(&self) -> Option<&::torrent::Engine> {
        self.engine.get()?.as_ref()
    }

    pub fn set_upload(&self, upload: ::torrent::Upload) {
        self.config.lock().unwrap().upload = upload;
        if let Some(engine) = self.running_engine() {
            engine.set_upload(upload);
        }
    }

    pub fn state_lock(&self) -> MutexGuard<'_, ()> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn index_retry_in(&self, info_hash: &str) -> Option<(Duration, String)> {
        let failed = self.failed_index.lock().unwrap();
        let (since, reason) = failed.get(info_hash)?;
        Some((
            RETRY_INDEX_AFTER.checked_sub(since.elapsed())?,
            reason.clone(),
        ))
    }

    fn note_index(&self, info_hash: &str, failure: Option<String>) {
        let mut failed = self.failed_index.lock().unwrap();
        match failure {
            Some(reason) => {
                failed.insert(info_hash.to_string(), (Instant::now(), reason));
            }
            None => {
                failed.remove(info_hash);
            }
        }
    }
}

#[derive(Clone)]
pub struct Config {
    pub info_hash: String,
    pub host: Arc<TorrentHost>,
}

impl PartialEq for Config {
    fn eq(&self, other: &Self) -> bool {
        self.info_hash == other.info_hash && Arc::ptr_eq(&self.host, &other.host)
    }
}

impl Eq for Config {}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("info_hash", &self.info_hash)
            .finish_non_exhaustive()
    }
}

pub struct Torrent {
    info_hash: String,
    host: Arc<TorrentHost>,
}

impl Torrent {
    pub fn new(config: &Config) -> Self {
        Self {
            info_hash: config.info_hash.clone(),
            host: config.host.clone(),
        }
    }

    fn engine(&self) -> Result<&::torrent::Engine, RemoteError> {
        self.host
            .engine()
            .ok_or_else(|| RemoteError::Other("torrents are not available".into()))
    }
}

pub fn error(error: ::torrent::Error) -> RemoteError {
    match error {
        ::torrent::Error::Timeout => RemoteError::Unreachable(error.to_string()),
        ::torrent::Error::Other(message) => RemoteError::Unreachable(message),
        ::torrent::Error::NoPeers | ::torrent::Error::Unknown | ::torrent::Error::Invalid(_) => {
            RemoteError::Other(error.to_string())
        }
    }
}

fn file_index(key: &str) -> Result<usize, RemoteError> {
    key.parse()
        .map_err(|_| RemoteError::Other(format!("not a torrent file key: {key}")))
}

impl ServerClient for Torrent {
    fn ping(&self) -> Result<(), RemoteError> {
        if !self.engine()?.is_stored(&self.info_hash) {
            return Err(RemoteError::Other(::torrent::Error::Unknown.to_string()));
        }
        match self.host.index_retry_in(&self.info_hash) {
            Some((wait, reason)) => Err(RemoteError::Unreachable(format!(
                "{reason}; next try in {} min",
                wait.as_secs().div_ceil(60)
            ))),
            None => Ok(()),
        }
    }

    fn songs(&self) -> Result<Vec<RemoteSong>, RemoteError> {
        let listed = index::songs(self.engine()?, &self.info_hash, &self.host.state);
        let failure = match &listed {
            Ok(_) => None,
            Err(RemoteError::Unreachable(reason) | RemoteError::Other(reason)) => {
                Some(reason.clone())
            }
            Err(RemoteError::Auth) => Some("access denied".to_string()),
        };
        self.host.note_index(&self.info_hash, failure);
        listed
    }

    fn favorite_keys(&self) -> Result<Vec<String>, RemoteError> {
        Ok(Vec::new())
    }

    fn cover_art(&self, key: &str) -> Result<Vec<u8>, RemoteError> {
        index::cover(self.engine()?, &self.info_hash, key)
    }

    fn forget(&self) {
        let _state = self.host.state_lock();
        if let Some(engine) = self.host.engine() {
            engine.forget(&self.info_hash);
        }
    }

    fn peers(&self) -> Option<Peers> {
        let swarm = self.host.running_engine()?.swarm(&self.info_hash)?;
        Some(Peers {
            connected: swarm.connected,
            known: swarm.known,
        })
    }

    fn fetch_range(
        &self,
        key: &str,
        start: u64,
        end: Option<u64>,
    ) -> Result<server_http::RangeBody, RemoteError> {
        let body = self
            .engine()?
            .read(&self.info_hash, file_index(key)?, start, end, READ_TIMEOUT)
            .map_err(error)?;
        Ok(server_http::RangeBody {
            offset: body.offset(),
            total: Some(body.total()),
            ranged: true,
            body: Box::new(body),
        })
    }
}
