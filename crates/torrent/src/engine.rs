use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use librqbit::api::TorrentIdOrHash;
use librqbit::dht::DhtPersistenceConfig;
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListOnlyResponse,
    ListenerMode, ListenerOptions, ManagedTorrent, Session, SessionOptions,
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::runtime::{Handle, Runtime};

use crate::body::{Body, Helper, Lease, Opened, Probe, Source};
use crate::{Config, Error, FileEntry, Input, Meta, Network, Swarm, Upload, Want};

const JANITOR_TICK: Duration = Duration::from_secs(10);
const INIT_TIMEOUT: Duration = Duration::from_secs(60);
const UNLOAD_POLL: Duration = Duration::from_millis(20);
const PROGRESS_TICK: Duration = Duration::from_secs(1);
const PROBE_STREAMS: usize = 4;
const BLOCKING_PERMITS: usize = 64;
const SHARED_STREAMS: usize = 24;
const OFF_WHILE_RUNNING_BPS: u32 = 16 * 1024;
pub(crate) const STALL_TIMEOUT: Duration = Duration::from_secs(30);
const PATIENCE_CAP: Duration = Duration::from_secs(5 * 60);
const STREAM_LOOKAHEAD: u64 = 32 * 1024 * 1024;
const PIECES_IN_FLIGHT: u64 = 8;
const MAX_HELPERS: u64 = 3;

struct Slot {
    torrent: Arc<ManagedTorrent>,
    leases: usize,
    idle_since: Instant,
}

struct State {
    session: Option<Arc<Session>>,
    session_upload_off: bool,
    restart: bool,
    last_active: Instant,
    busy: usize,
    slots: HashMap<String, Slot>,
    unloading: HashSet<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            session: None,
            session_upload_off: false,
            restart: false,
            last_active: Instant::now(),
            busy: 0,
            slots: HashMap::new(),
            unloading: HashSet::new(),
        }
    }
}

struct Busy(Arc<Shared>);

impl Busy {
    fn new(shared: &Arc<Shared>) -> Self {
        let mut state = shared.state.lock().unwrap();
        state.busy += 1;
        state.last_active = Instant::now();
        drop(state);
        Self(shared.clone())
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap();
        state.busy = state.busy.saturating_sub(1);
        state.last_active = Instant::now();
    }
}

pub(crate) struct Shared {
    config: Mutex<Config>,
    state: Mutex<State>,
    starting: tokio::sync::Mutex<()>,
    streams: Arc<tokio::sync::Semaphore>,
    handle: Handle,
}

#[derive(Clone)]
pub struct Engine {
    shared: Arc<Shared>,
    runtime: Arc<Runtime>,
}

impl Engine {
    pub fn new(config: Config) -> Result<Self, Error> {
        if config.work_dir.exists() {
            let _ = std::fs::remove_dir_all(&config.work_dir);
        }
        std::fs::create_dir_all(&config.work_dir).map_err(other)?;
        std::fs::create_dir_all(&config.state_dir).map_err(other)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("torrent")
            .enable_all()
            .build()
            .map_err(other)?;
        let shared = Arc::new(Shared {
            config: Mutex::new(config),
            state: Mutex::new(State::default()),
            starting: tokio::sync::Mutex::new(()),
            streams: Arc::new(tokio::sync::Semaphore::new(SHARED_STREAMS)),
            handle: runtime.handle().clone(),
        });
        runtime.spawn(janitor(Arc::downgrade(&shared)));
        Ok(Self {
            shared,
            runtime: Arc::new(runtime),
        })
    }

    pub fn resolve(&self, input: Input, timeout: Duration) -> Result<Meta, Error> {
        let shared = self.shared.clone();
        let (add, original) = match input {
            Input::File(bytes) => (AddTorrent::from_bytes(bytes.clone()), Some(bytes)),
            Input::Magnet(link) => (AddTorrent::from_url(link), None),
        };
        self.runtime.block_on(async move {
            let _busy = Busy::new(&shared);
            let session = shared.session().await?;
            let options = AddTorrentOptions {
                list_only: true,
                initial_peers: shared.peers(),
                ..Default::default()
            };
            let added = tokio::time::timeout(timeout, session.add_torrent(add, Some(options)))
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|e| Error::Invalid(format!("{e:#}")))?;
            let AddTorrentResponse::ListOnly(listed) = added else {
                return Err(Error::Other(
                    "the torrent was started instead of listed".into(),
                ));
            };
            let meta = meta_of(&listed);
            let bytes = original.unwrap_or_else(|| listed.torrent_bytes.to_vec());
            std::fs::write(shared.torrent_file(&meta.info_hash), bytes).map_err(other)?;
            Ok(meta)
        })
    }

    pub fn meta(&self, info_hash: &str) -> Result<Meta, Error> {
        let bytes = self.shared.stored(info_hash)?;
        let shared = self.shared.clone();
        self.runtime.block_on(async move {
            let _busy = Busy::new(&shared);
            let session = shared.session().await?;
            let options = AddTorrentOptions {
                list_only: true,
                ..Default::default()
            };
            match session
                .add_torrent(AddTorrent::from_bytes(bytes), Some(options))
                .await
                .map_err(|e| Error::Invalid(format!("{e:#}")))?
            {
                AddTorrentResponse::ListOnly(listed) => Ok(meta_of(&listed)),
                _ => Err(Error::Other(
                    "the torrent was started instead of listed".into(),
                )),
            }
        })
    }

    pub fn is_stored(&self, info_hash: &str) -> bool {
        self.shared.torrent_file(info_hash).is_file()
    }

    pub fn probe(&self, info_hash: &str, wants: &[Want], stall: Duration) -> Result<Probe, Error> {
        let shared = self.shared.clone();
        let hash = info_hash.to_string();
        let wants = wants.to_vec();
        let lease = self.runtime.block_on(async move {
            let lease = Shared::acquire(&shared, &hash, true).await?;
            let progress = Arc::new(AtomicU64::new(0));
            let streams = Arc::new(tokio::sync::Semaphore::new(PROBE_STREAMS));
            let mut jobs = tokio::task::JoinSet::new();
            for want in wants.into_iter().filter(|w| w.end > w.start) {
                let torrent = lease.torrent.clone();
                let progress = progress.clone();
                let streams = streams.clone();
                let shared_streams = shared.streams.clone();
                jobs.spawn(async move {
                    let _stream_slot = streams.acquire_owned().await?;
                    let _shared_slot = shared_streams.acquire_owned().await?;
                    let mut stream = torrent.stream(want.file).await?;
                    stream.seek(std::io::SeekFrom::Start(want.start)).await?;
                    let mut range = stream.take(want.end - want.start);
                    let mut buf = vec![0u8; 64 * 1024];
                    loop {
                        let n = range.read(&mut buf).await?;
                        if n == 0 {
                            break;
                        }
                        progress.fetch_add(n as u64, Ordering::Relaxed);
                    }
                    anyhow::Ok(())
                });
            }
            let mut seen = 0;
            let mut last_progress = Instant::now();
            loop {
                tokio::select! {
                    done = jobs.join_next() => match done {
                        None => break,
                        Some(done) => done
                            .map_err(other)?
                            .map_err(|e| Error::Other(format!("{e:#}")))?,
                    },
                    _ = tokio::time::sleep(PROGRESS_TICK) => {
                        let now = progress.load(Ordering::Relaxed);
                        if now != seen {
                            seen = now;
                            last_progress = Instant::now();
                        } else if last_progress.elapsed() >= stall {
                            return Err(Error::Timeout);
                        }
                    }
                }
            }
            Ok::<_, Error>(lease)
        })?;
        Ok(Probe::new(self.shared.work_root(info_hash), lease))
    }

    pub fn read(
        &self,
        info_hash: &str,
        file: usize,
        start: u64,
        end: Option<u64>,
        timeout: Duration,
    ) -> Result<Body, Error> {
        let shared = self.shared.clone();
        let hash = info_hash.to_string();
        let (lease, stream, helpers, first, total) = self.runtime.block_on(async move {
            let lease = Shared::acquire(&shared, &hash, false).await?;
            let total = lease
                .torrent
                .with_metadata(|m| m.file_infos.get(file).map(|f| f.len))
                .map_err(|e| Error::Other(format!("{e:#}")))?
                .ok_or_else(|| Error::Invalid(format!("no file {file}")))?;
            let torrent = lease.torrent.clone();
            let open = async {
                let mut stream = torrent
                    .clone()
                    .stream(file)
                    .await
                    .map_err(|e| Error::Other(format!("{e:#}")))?;
                stream
                    .seek(std::io::SeekFrom::Start(start))
                    .await
                    .map_err(other)?;
                Ok::<_, Error>(stream)
            };
            let mut stream = patient(&torrent, timeout, open).await??;
            let mut first = vec![0u8; 64 * 1024];
            let wanted = end
                .map_or(total, |end| end.min(total))
                .saturating_sub(start);
            let cap = first.len().min(wanted as usize);
            let got = if cap == 0 {
                0
            } else {
                patient(&torrent, timeout, stream.read(&mut first[..cap]))
                    .await?
                    .map_err(other)?
            };
            first.truncate(got);
            let helpers = helpers(&shared, &torrent, file, start, total).await;
            let stream: Box<dyn Source> = Box::new(stream);
            Ok::<_, Error>((lease, stream, helpers, first, total))
        })?;
        let limit = end
            .map_or(total, |end| end.min(total))
            .saturating_sub(start);
        Ok(Body::new(
            self.clone(),
            lease,
            Opened {
                stream,
                helpers,
                pending: first,
                limit,
                offset: start,
                total,
            },
        ))
    }

    pub fn forget(&self, info_hash: &str) {
        if !crate::is_info_hash(info_hash) {
            return;
        }
        let shared = self.shared.clone();
        let hash = info_hash.to_string();
        self.runtime
            .block_on(async move { shared.unload(&hash, true).await });
        let Ok(entries) = std::fs::read_dir(self.state_dir()) else {
            return;
        };
        for entry in entries.flatten() {
            if !entry.file_name().to_string_lossy().starts_with(info_hash) {
                continue;
            }
            let path = entry.path();
            let removed = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            if let Err(e) = removed {
                log::warn!("torrent {info_hash}: {path:?} not removed: {e}");
            }
        }
    }

    pub fn swarm(&self, info_hash: &str) -> Option<Swarm> {
        let torrent = self
            .shared
            .state
            .lock()
            .unwrap()
            .slots
            .get(info_hash)?
            .torrent
            .clone();
        let peers = torrent.live()?.stats_snapshot().peer_stats;
        Some(Swarm {
            connected: peers.live,
            known: peers.seen,
        })
    }

    pub fn state_dir(&self) -> PathBuf {
        self.shared.config.lock().unwrap().state_dir.clone()
    }

    pub fn set_upload(&self, upload: Upload) {
        self.shared.config.lock().unwrap().upload = upload;
        let mut state = self.shared.state.lock().unwrap();
        let Some(session) = state.session.clone() else {
            return;
        };
        let off = upload == Upload::Off;
        state.restart = off != state.session_upload_off;
        let bps = if off {
            NonZeroU32::new(OFF_WHILE_RUNNING_BPS)
        } else {
            upload_bps(upload)
        };
        session.ratelimits.set_upload_bps(bps);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn loaded(&self) -> Vec<String> {
        let mut hashes: Vec<String> = self
            .shared
            .state
            .lock()
            .unwrap()
            .slots
            .keys()
            .cloned()
            .collect();
        hashes.sort();
        hashes
    }

    pub(crate) fn runtime(&self) -> &Runtime {
        &self.runtime
    }
}

impl Shared {
    fn work_root(&self, info_hash: &str) -> PathBuf {
        self.config.lock().unwrap().work_dir.join(info_hash)
    }

    fn torrent_file(&self, info_hash: &str) -> PathBuf {
        self.config
            .lock()
            .unwrap()
            .state_dir
            .join(format!("{info_hash}.torrent"))
    }

    fn stored(&self, info_hash: &str) -> Result<Vec<u8>, Error> {
        if !crate::is_info_hash(info_hash) {
            return Err(Error::Unknown);
        }
        std::fs::read(self.torrent_file(info_hash)).map_err(|_| Error::Unknown)
    }

    fn peers(&self) -> Option<Vec<std::net::SocketAddr>> {
        match &self.config.lock().unwrap().network {
            Network::Public => None,
            Network::Local { peers, .. } => Some(peers.clone()),
        }
    }

    fn running_session(&self) -> Option<Arc<Session>> {
        let mut state = self.state.lock().unwrap();
        let session = state.session.clone()?;
        state.last_active = Instant::now();
        Some(session)
    }

    async fn session(&self) -> Result<Arc<Session>, Error> {
        if let Some(session) = self.running_session() {
            return Ok(session);
        }
        let _starting = self.starting.lock().await;
        if let Some(session) = self.running_session() {
            return Ok(session);
        }
        let config = self.config.lock().unwrap().clone();
        let local = matches!(config.network, Network::Local { .. });
        let listen_addr = match &config.network {
            Network::Public => ListenerOptions::default().listen_addr,
            Network::Local { listen_port, .. } => ([127, 0, 0, 1], *listen_port).into(),
        };
        let dht = (!local).then(|| DhtSessionConfig {
            persistence: Some(DhtPersistenceConfig {
                config_filename: Some(config.state_dir.join("dht.json")),
                ..Default::default()
            }),
            ..Default::default()
        });
        let options = SessionOptions {
            dht,
            disable_trackers: local,
            disable_local_service_discovery: local,
            persistence: None,
            fastresume: false,
            listen: Some(ListenerOptions {
                mode: if local {
                    ListenerMode::TcpOnly
                } else {
                    ListenerMode::TcpAndUtp
                },
                listen_addr,
                ..Default::default()
            }),
            ratelimits: LimitsConfig {
                upload_bps: upload_bps(config.upload),
                download_bps: None,
            },
            disable_upload: config.upload == Upload::Off,
            runtime_worker_threads: Some(BLOCKING_PERMITS),
            ..Default::default()
        };
        let session = Session::new_with_opts(config.work_dir.clone(), options)
            .await
            .map_err(|e| Error::Other(format!("{e:#}")))?;
        let mut state = self.state.lock().unwrap();
        state.session = Some(session.clone());
        state.session_upload_off = config.upload == Upload::Off;
        state.restart = false;
        state.last_active = Instant::now();
        Ok(session)
    }

    async fn acquire(shared: &Arc<Self>, info_hash: &str, discard: bool) -> Result<Lease, Error> {
        let _busy = Busy::new(shared);
        loop {
            {
                let mut state = shared.state.lock().unwrap();
                if !state.unloading.contains(info_hash) {
                    if let Some(slot) = state.slots.get_mut(info_hash) {
                        slot.leases += 1;
                        return Ok(Lease::new(shared, info_hash, slot.torrent.clone(), discard));
                    }
                    break;
                }
            }
            tokio::time::sleep(UNLOAD_POLL).await;
        }
        let bytes = shared.stored(info_hash)?;
        let session = shared.session().await?;
        let root = shared.work_root(info_hash);
        let options = AddTorrentOptions {
            only_files: Some(Vec::new()),
            output_folder: Some(root.to_string_lossy().into_owned()),
            overwrite: true,
            initial_peers: shared.peers(),
            ..Default::default()
        };
        let (torrent, fresh) = match session
            .add_torrent(AddTorrent::from_bytes(bytes), Some(options))
            .await
            .map_err(|e| Error::Invalid(format!("{e:#}")))?
        {
            AddTorrentResponse::Added(_, torrent) => (torrent, true),
            AddTorrentResponse::AlreadyManaged(_, torrent) => (torrent, false),
            AddTorrentResponse::ListOnly(_) => {
                return Err(Error::Other(
                    "the torrent was listed instead of started".into(),
                ));
            }
        };
        let initialized =
            match tokio::time::timeout(INIT_TIMEOUT, torrent.wait_until_initialized()).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(Error::Other(format!("{e:#}"))),
                Err(_) => Err(Error::Timeout),
            };
        if let Err(error) = initialized {
            if fresh {
                if let Err(e) = session
                    .delete(TorrentIdOrHash::Id(torrent.id()), true)
                    .await
                {
                    log::warn!("torrent {info_hash}: a failed start was not removed: {e:#}");
                }
                let _ = std::fs::remove_dir_all(&root);
            }
            return Err(error);
        }
        let mut state = shared.state.lock().unwrap();
        let slot = state
            .slots
            .entry(info_hash.to_string())
            .or_insert_with(|| Slot {
                torrent: torrent.clone(),
                leases: 0,
                idle_since: Instant::now(),
            });
        slot.leases += 1;
        Ok(Lease::new(shared, info_hash, slot.torrent.clone(), discard))
    }

    pub(crate) fn release(self: &Arc<Self>, info_hash: &str, discard: bool) {
        let idle = {
            let mut state = self.state.lock().unwrap();
            match state.slots.get_mut(info_hash) {
                Some(slot) => {
                    slot.leases = slot.leases.saturating_sub(1);
                    if slot.leases == 0 {
                        slot.idle_since = Instant::now();
                    }
                    slot.leases == 0
                }
                None => false,
            }
        };
        if !idle {
            return;
        }
        let shared = self.clone();
        let hash = info_hash.to_string();
        if discard {
            self.handle
                .spawn(async move { shared.unload(&hash, false).await });
            return;
        }
        let (limit, work_dir) = {
            let config = self.config.lock().unwrap();
            (config.work_limit_bytes, config.work_dir.clone())
        };
        if limit == u64::MAX {
            return;
        }
        self.handle.spawn(async move {
            let used = tokio::task::spawn_blocking(move || dir_size(&work_dir))
                .await
                .unwrap_or(0);
            if used > limit {
                shared.unload(&hash, false).await;
            }
        });
    }

    async fn unload(&self, info_hash: &str, force: bool) {
        let (session, torrent): (Option<Arc<Session>>, Option<Arc<ManagedTorrent>>) = {
            let mut state = self.state.lock().unwrap();
            let busy = state
                .slots
                .get(info_hash)
                .is_some_and(|slot| slot.leases > 0);
            if (busy && !force) || !state.unloading.insert(info_hash.to_string()) {
                return;
            }
            let torrent = state.slots.remove(info_hash).map(|slot| slot.torrent);
            state.last_active = Instant::now();
            (state.session.clone(), torrent)
        };
        if let (Some(session), Some(torrent)) = (session, torrent.as_ref()) {
            let id = TorrentIdOrHash::Id(torrent.id());
            if let Err(e) = session.delete(id, true).await {
                log::warn!("torrent {info_hash}: unloading failed: {e:#}");
            }
        }
        if torrent.is_some() || force {
            let root = self.work_root(info_hash);
            let removed = tokio::task::spawn_blocking(move || {
                if root.exists() {
                    std::fs::remove_dir_all(&root)
                } else {
                    Ok(())
                }
            })
            .await;
            if let Ok(Err(e)) = removed {
                log::warn!("torrent {info_hash}: work data not removed: {e}");
            }
        }
        self.state.lock().unwrap().unloading.remove(info_hash);
    }

    async fn tidy(&self) {
        let (idle_unload, limit, work_dir) = {
            let config = self.config.lock().unwrap();
            (
                config.idle_unload,
                config.work_limit_bytes,
                config.work_dir.clone(),
            )
        };
        let mut idle: Vec<(Instant, String)> = self
            .state
            .lock()
            .unwrap()
            .slots
            .iter()
            .filter(|(_, slot)| slot.leases == 0)
            .map(|(hash, slot)| (slot.idle_since, hash.clone()))
            .collect();
        idle.sort();
        let mut over = if idle.is_empty() || limit == u64::MAX {
            0
        } else {
            tokio::task::spawn_blocking(move || dir_size(&work_dir))
                .await
                .unwrap_or(0)
                .saturating_sub(limit)
        };
        for (since, hash) in idle {
            if since.elapsed() >= idle_unload || over > 0 {
                let root = self.work_root(&hash);
                let size = tokio::task::spawn_blocking(move || dir_size(&root))
                    .await
                    .unwrap_or(0);
                self.unload(&hash, false).await;
                over = over.saturating_sub(size);
            }
        }
        let stale = {
            let mut state = self.state.lock().unwrap();
            let stale = state.slots.is_empty()
                && state.busy == 0
                && state.unloading.is_empty()
                && (state.restart || state.last_active.elapsed() >= idle_unload);
            if stale {
                state.restart = false;
                state.session.take()
            } else {
                None
            }
        };
        if let Some(session) = stale {
            session.stop().await;
        }
    }
}

fn fetched(torrent: &ManagedTorrent) -> u64 {
    torrent
        .live()
        .map_or(0, |live| live.stats_snapshot().fetched_bytes)
}

pub(crate) async fn patient<F: std::future::Future>(
    torrent: &ManagedTorrent,
    quiet: Duration,
    work: F,
) -> Result<F::Output, Error> {
    tokio::pin!(work);
    let started = Instant::now();
    let mut seen = fetched(torrent);
    let mut last_progress = Instant::now();
    loop {
        tokio::select! {
            done = &mut work => return Ok(done),
            _ = tokio::time::sleep(PROGRESS_TICK) => {
                let now = fetched(torrent);
                if now != seen {
                    seen = now;
                    last_progress = Instant::now();
                }
                if last_progress.elapsed() >= quiet || started.elapsed() >= PATIENCE_CAP {
                    return Err(Error::Timeout);
                }
            }
        }
    }
}

pub(crate) fn helper_count(piece: u64) -> u64 {
    (PIECES_IN_FLIGHT * piece)
        .div_ceil(STREAM_LOOKAHEAD)
        .saturating_sub(1)
        .min(MAX_HELPERS)
}

async fn helpers(
    shared: &Shared,
    torrent: &Arc<ManagedTorrent>,
    file: usize,
    start: u64,
    total: u64,
) -> Vec<Helper> {
    let piece = torrent
        .with_metadata(|m| u64::from(m.lengths().default_piece_length()))
        .unwrap_or(0);
    let count = helper_count(piece);
    let mut helpers = Vec::new();
    for k in 1..=count {
        let at = start + k * STREAM_LOOKAHEAD;
        if at >= total {
            break;
        }
        let Ok(slot) = shared.streams.clone().try_acquire_owned() else {
            break;
        };
        let Ok(mut stream) = torrent.clone().stream(file).await else {
            break;
        };
        if stream.seek(std::io::SeekFrom::Start(at)).await.is_err() {
            break;
        }
        helpers.push(Helper {
            _stream: Box::new(stream),
            _slot: slot,
        });
    }
    helpers
}

async fn janitor(shared: Weak<Shared>) {
    loop {
        tokio::time::sleep(JANITOR_TICK).await;
        let Some(shared) = shared.upgrade() else {
            return;
        };
        shared.tidy().await;
    }
}

fn meta_of(listed: &ListOnlyResponse) -> Meta {
    let files = listed
        .info
        .iter_file_details()
        .enumerate()
        .filter(|(_, details)| !details.attrs().padding)
        .map(|(index, details)| FileEntry {
            index,
            path: details.filename.to_pathbuf(),
            len: details.len,
        })
        .collect();
    Meta {
        info_hash: listed.info_hash.as_string(),
        name: listed
            .info
            .name()
            .map(|name| name.into_owned())
            .unwrap_or_default(),
        files,
    }
}

fn upload_bps(upload: Upload) -> Option<NonZeroU32> {
    match upload {
        Upload::WhileActive => None,
        Upload::Limited(kib) => Some(kib.saturating_mul(NonZeroU32::new(1024).unwrap())),
        Upload::Off => None,
    }
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().ends_with(".view"))
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => dir_size(&entry.path()),
            Ok(meta) => allocated(&meta),
            Err(_) => 0,
        })
        .sum()
}

#[cfg(unix)]
fn allocated(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks() * 512
}

#[cfg(not(unix))]
fn allocated(meta: &std::fs::Metadata) -> u64 {
    meta.len()
}

fn other(error: impl std::fmt::Display) -> Error {
    Error::Other(error.to_string())
}
