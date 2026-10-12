use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::MediaStream;

const MAX_METADATA_BLOCKS: usize = 256;
const UNKNOWN_FRAME_SLACK: u64 = 64 * 1024;

trait ReadSeek: Read + Seek + Send + Sync {}

impl<T: Read + Seek + Send + Sync> ReadSeek for T {}

pub(crate) type SharedStream = Arc<Mutex<Box<dyn MediaStream>>>;

fn lock(shared: &SharedStream) -> MutexGuard<'_, Box<dyn MediaStream>> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) enum Origin {
    File(PathBuf),
    Shared(SharedStream),
}

impl Origin {
    fn reader(&self) -> io::Result<Box<dyn ReadSeek>> {
        Ok(match self {
            Origin::File(path) => Box::new(File::open(path)?),
            Origin::Shared(shared) => Box::new(Handle::new(Arc::clone(shared))),
        })
    }

    fn byte_len(&self) -> io::Result<u64> {
        match self {
            Origin::File(path) => Ok(std::fs::metadata(path)?.len()),
            Origin::Shared(shared) => lock(shared)
                .byte_len()
                .ok_or_else(|| io::Error::other("the stream has no length")),
        }
    }
}

pub(crate) struct Handle {
    shared: SharedStream,
    pos: u64,
}

impl Handle {
    pub(crate) fn new(shared: SharedStream) -> Self {
        Self { shared, pos: 0 }
    }
}

impl Read for Handle {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut stream = lock(&self.shared);
        stream.seek(SeekFrom::Start(self.pos))?;
        let n = stream.read(buf)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Handle {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let next = match pos {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(delta) => self
                .pos
                .checked_add_signed(delta)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek out of range"))?,
            SeekFrom::End(_) => lock(&self.shared).seek(pos)?,
        };
        self.pos = next;
        Ok(next)
    }
}

impl symphonia::core::io::MediaSource for Handle {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        lock(&self.shared).byte_len()
    }
}

pub(crate) struct Splicer {
    origin: Origin,
    header: Arc<[u8]>,
    audio_start: u64,
    len: u64,
    slack: u64,
}

impl Splicer {
    pub(crate) fn new(origin: Origin, stream_info: &[u8]) -> Option<Self> {
        let block_len = u32::try_from(stream_info.len())
            .ok()
            .filter(|n| *n < 1 << 24)?;
        let mut header = b"fLaC".to_vec();
        header.push(0x80);
        header.extend_from_slice(&block_len.to_be_bytes()[1..]);
        header.extend_from_slice(stream_info);
        let len = origin.byte_len().ok()?;
        let audio_start = audio_start(origin.reader().ok()?.as_mut()).ok()?;
        let max_frame = stream_info
            .get(7..10)
            .map_or(0, |b| u64::from(u32::from_be_bytes([0, b[0], b[1], b[2]])));
        let slack = match max_frame {
            0 => UNKNOWN_FRAME_SLACK,
            frame => 2 * frame,
        };
        (audio_start < len).then(|| Self {
            origin,
            header: header.into(),
            audio_start,
            len,
            slack,
        })
    }

    pub(crate) fn slack(&self) -> u64 {
        self.slack
    }

    pub(crate) fn audio(&self) -> (u64, u64) {
        (self.audio_start, self.len - self.audio_start)
    }

    pub(crate) fn will_need(&self, start: u64, len: u64) {
        if let Origin::Shared(shared) = &self.origin {
            lock(shared).will_need(start, len.min(self.len.saturating_sub(start)));
        }
    }

    pub(crate) fn open_at(&self, start: u64) -> io::Result<Spliced> {
        Ok(Spliced {
            header: Arc::clone(&self.header),
            inner: self.origin.reader()?,
            start,
            pos: 0,
            len: self.header.len() as u64 + self.len.saturating_sub(start),
        })
    }
}

fn audio_start(reader: &mut dyn ReadSeek) -> io::Result<u64> {
    let mut head = [0u8; 10];
    reader.read_exact(&mut head)?;
    let tag = match &head[..3] {
        b"ID3" => {
            let size = head[6..10]
                .iter()
                .fold(0u64, |size, b| (size << 7) | u64::from(b & 0x7f));
            let footer = if head[5] & 0x10 != 0 { 10 } else { 0 };
            10 + size + footer
        }
        _ => 0,
    };
    let mut marker = [0u8; 4];
    reader.seek(SeekFrom::Start(tag))?;
    reader.read_exact(&mut marker)?;
    if &marker != b"fLaC" {
        return Err(io::Error::other("not a FLAC stream"));
    }
    let mut pos = tag + 4;
    for _ in 0..MAX_METADATA_BLOCKS {
        let mut block = [0u8; 4];
        reader.seek(SeekFrom::Start(pos))?;
        reader.read_exact(&mut block)?;
        let len = u64::from(u32::from_be_bytes([0, block[1], block[2], block[3]]));
        pos += 4 + len;
        if block[0] & 0x80 != 0 {
            return Ok(pos);
        }
    }
    Err(io::Error::other("too many FLAC metadata blocks"))
}

pub(crate) struct Spliced {
    header: Arc<[u8]>,
    inner: Box<dyn ReadSeek>,
    start: u64,
    pos: u64,
    len: u64,
}

impl Read for Spliced {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let head = self.header.len() as u64;
        if self.pos < head {
            let from = self.pos as usize;
            let n = buf.len().min(self.header.len() - from);
            buf[..n].copy_from_slice(&self.header[from..from + n]);
            self.pos += n as u64;
            return Ok(n);
        }
        self.inner
            .seek(SeekFrom::Start(self.start + (self.pos - head)))?;
        let n = self.inner.read(buf)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Spliced {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let next = match pos {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta),
            SeekFrom::End(delta) => self.len.checked_add_signed(delta),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek out of range"))?;
        self.pos = next;
        Ok(next)
    }
}

impl symphonia::core::io::MediaSource for Spliced {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}
