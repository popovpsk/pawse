use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use librqbit::ManagedTorrent;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek};

use crate::Error;
use crate::engine::{Engine, STALL_TIMEOUT, Shared, patient};

pub(crate) trait Source: AsyncRead + AsyncSeek + Unpin + Send {}

impl<T: AsyncRead + AsyncSeek + Unpin + Send> Source for T {}

pub(crate) struct Lease {
    shared: Arc<Shared>,
    info_hash: String,
    discard: bool,
    pub(crate) torrent: Arc<ManagedTorrent>,
}

impl Lease {
    pub(crate) fn new(
        shared: &Arc<Shared>,
        info_hash: &str,
        torrent: Arc<ManagedTorrent>,
        discard: bool,
    ) -> Self {
        Self {
            shared: shared.clone(),
            info_hash: info_hash.to_string(),
            discard,
            torrent,
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.shared.release(&self.info_hash, self.discard);
    }
}

pub struct Probe {
    root: PathBuf,
    _lease: Lease,
}

impl Probe {
    pub(crate) fn new(root: PathBuf, lease: Lease) -> Self {
        Self {
            root,
            _lease: lease,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

pub(crate) struct Helper {
    pub _stream: Box<dyn Source>,
    pub _slot: tokio::sync::OwnedSemaphorePermit,
}

pub(crate) struct Opened {
    pub stream: Box<dyn Source>,
    pub helpers: Vec<Helper>,
    pub pending: Vec<u8>,
    pub limit: u64,
    pub offset: u64,
    pub total: u64,
}

pub struct Body {
    stream: Box<dyn Source>,
    _helpers: Vec<Helper>,
    pending: Vec<u8>,
    served: usize,
    remaining: u64,
    offset: u64,
    total: u64,
    lease: Lease,
    engine: Engine,
}

impl Body {
    pub(crate) fn new(engine: Engine, lease: Lease, opened: Opened) -> Self {
        Self {
            stream: opened.stream,
            _helpers: opened.helpers,
            pending: opened.pending,
            served: 0,
            remaining: opened.limit,
            offset: opened.offset,
            total: opened.total,
            lease,
            engine,
        }
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn total(&self) -> u64 {
        self.total
    }
}

impl Read for Body {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.remaining == 0 {
            return Ok(0);
        }
        let cap = buf
            .len()
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        let got = if self.served < self.pending.len() {
            let n = cap.min(self.pending.len() - self.served);
            buf[..n].copy_from_slice(&self.pending[self.served..self.served + n]);
            self.served += n;
            n
        } else {
            let stream = &mut self.stream;
            let torrent = &self.lease.torrent;
            self.engine.runtime().block_on(async {
                match patient(torrent, STALL_TIMEOUT, stream.read(&mut buf[..cap])).await {
                    Ok(read) => read,
                    Err(Error::Timeout) => {
                        Err(io::Error::new(io::ErrorKind::TimedOut, "torrent stalled"))
                    }
                    Err(Error::NoPeers) => Err(io::Error::new(
                        io::ErrorKind::NotConnected,
                        Error::NoPeers.to_string(),
                    )),
                    Err(error) => Err(io::Error::other(error.to_string())),
                }
            })?
        };
        self.remaining -= got as u64;
        Ok(got)
    }
}
