use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use music_library::RemoteSong;

use super::{RemoteError, ServerClient};

mod index;

const READ_TIMEOUT: Duration = Duration::from_secs(20);
const RETRY_INDEX_AFTER: Duration = Duration::from_secs(10 * 60);

static CONFIG: Mutex<Option<::torrent::Config>> = Mutex::new(None);
static ENGINE: OnceLock<Option<::torrent::Engine>> = OnceLock::new();
static STATE: Mutex<()> = Mutex::new(());
static FAILED_INDEX: Mutex<Option<HashMap<String, (Instant, String)>>> = Mutex::new(None);

pub fn configure(config: ::torrent::Config) {
    *CONFIG.lock().unwrap() = Some(config);
}

pub fn engine() -> Option<&'static ::torrent::Engine> {
    ENGINE
        .get_or_init(|| {
            let config = CONFIG.lock().unwrap().clone()?;
            ::torrent::Engine::new(config)
                .inspect_err(|e| log::error!("Torrents are unavailable: {e}"))
                .ok()
        })
        .as_ref()
}

pub fn running_engine() -> Option<&'static ::torrent::Engine> {
    ENGINE.get()?.as_ref()
}

pub fn set_upload(upload: ::torrent::Upload) {
    if let Some(config) = CONFIG.lock().unwrap().as_mut() {
        config.upload = upload;
    }
    if let Some(Some(engine)) = ENGINE.get() {
        engine.set_upload(upload);
    }
}

pub fn state_lock() -> MutexGuard<'static, ()> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn index_retry_in(info_hash: &str) -> Option<(Duration, String)> {
    let failed = FAILED_INDEX.lock().unwrap();
    let (since, reason) = failed.as_ref()?.get(info_hash)?;
    Some((
        RETRY_INDEX_AFTER.checked_sub(since.elapsed())?,
        reason.clone(),
    ))
}

fn note_index(info_hash: &str, failure: Option<String>) {
    let mut guard = FAILED_INDEX.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    match failure {
        Some(reason) => {
            map.insert(info_hash.to_string(), (Instant::now(), reason));
        }
        None => {
            map.remove(info_hash);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub info_hash: String,
}

pub struct Torrent {
    info_hash: String,
}

impl Torrent {
    pub fn new(config: &Config) -> Self {
        Self {
            info_hash: config.info_hash.clone(),
        }
    }

    fn engine(&self) -> Result<&'static ::torrent::Engine, RemoteError> {
        engine().ok_or_else(|| RemoteError::Other("torrents are not available".into()))
    }
}

pub fn error(error: ::torrent::Error) -> RemoteError {
    match error {
        ::torrent::Error::Timeout => RemoteError::Unreachable(error.to_string()),
        ::torrent::Error::Other(message) => RemoteError::Unreachable(message),
        ::torrent::Error::Unknown | ::torrent::Error::Invalid(_) => {
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
        match index_retry_in(&self.info_hash) {
            Some((wait, reason)) => Err(RemoteError::Unreachable(format!(
                "{reason}; next try in {} min",
                wait.as_secs().div_ceil(60)
            ))),
            None => Ok(()),
        }
    }

    fn songs(&self) -> Result<Vec<RemoteSong>, RemoteError> {
        let listed = index::songs(self.engine()?, &self.info_hash);
        let failure = match &listed {
            Ok(_) => None,
            Err(RemoteError::Unreachable(reason) | RemoteError::Other(reason)) => {
                Some(reason.clone())
            }
            Err(RemoteError::Auth) => Some("access denied".to_string()),
        };
        note_index(&self.info_hash, failure);
        listed
    }

    fn favorite_keys(&self) -> Result<Vec<String>, RemoteError> {
        Ok(Vec::new())
    }

    fn cover_art(&self, key: &str) -> Result<Vec<u8>, RemoteError> {
        index::cover(self.engine()?, &self.info_hash, key)
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
