use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::{FIRST_BYTE_FAILURES, FetchError, Fetched, MAX_FAILURES, RangeFetch};

const BLOCK: u64 = 32 * 1024;
const MAX_REQUEST: u64 = 4 * 1024 * 1024;
const CACHE_BYTES: u64 = 8 * 1024 * 1024;
const STOP_POLL: Duration = Duration::from_millis(50);

pub type StopCheck = Box<dyn Fn() -> bool + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeError {
    Unsupported,
    Stopped,
    Failed(String),
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RangeError::Unsupported => f.write_str("the server does not serve byte ranges"),
            RangeError::Stopped => f.write_str("the read was stopped"),
            RangeError::Failed(message) => f.write_str(message),
        }
    }
}

#[derive(Clone, Default)]
pub struct ReadFailure(Arc<Mutex<Option<String>>>);

impl ReadFailure {
    pub fn get(&self) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set(&self, message: String) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert(message);
    }
}

pub struct RangeReader {
    fetch: Arc<dyn RangeFetch>,
    len: Option<u64>,
    pos: u64,
    blocks: HashMap<u64, Box<[u8]>>,
    order: VecDeque<u64>,
    run_end: u64,
    run: u64,
    stop: StopCheck,
    failure: ReadFailure,
    broken: Option<String>,
}

impl RangeReader {
    pub fn open(fetch: Arc<dyn RangeFetch>, stop: StopCheck) -> Result<Self, RangeError> {
        let mut reader = Self {
            fetch,
            len: None,
            pos: 0,
            blocks: HashMap::new(),
            order: VecDeque::new(),
            run_end: u64::MAX,
            run: 0,
            stop,
            failure: ReadFailure::default(),
            broken: None,
        };
        reader.request(0, BLOCK, FIRST_BYTE_FAILURES)?;
        reader.run_end = 1;
        reader.run = 1;
        Ok(reader)
    }

    pub fn byte_len(&self) -> u64 {
        self.len.unwrap_or(0)
    }

    pub fn failure(&self) -> ReadFailure {
        self.failure.clone()
    }

    fn load(&mut self, index: u64) -> Result<(), RangeError> {
        let len = self.byte_len();
        let wanted = if index == self.run_end {
            (self.run * 2).min(MAX_REQUEST / BLOCK)
        } else {
            1
        };
        let last = len.div_ceil(BLOCK);
        let mut count = 0;
        while count < wanted && index + count < last && !self.blocks.contains_key(&(index + count))
        {
            count += 1;
        }
        let end = ((index + count) * BLOCK).min(len);
        self.request(index * BLOCK, end, MAX_FAILURES)?;
        self.run_end = index + count;
        self.run = count;
        Ok(())
    }

    fn request(&mut self, start: u64, end: u64, limit: u32) -> Result<(), RangeError> {
        let mut cur = start;
        let mut end = end;
        let mut failures = 0u32;
        while cur < end {
            if (self.stop)() {
                return Err(RangeError::Stopped);
            }
            let error = match self.fetch.fetch(cur, Some(end)) {
                Ok(fetched) => {
                    let body = self.accept(cur, fetched)?;
                    end = end.min(self.byte_len());
                    let (reached, error) = self.receive(cur, end, body);
                    if (self.stop)() {
                        return Err(RangeError::Stopped);
                    }
                    if reached > cur {
                        failures = 0;
                        cur = reached;
                        continue;
                    }
                    match error {
                        Some(error) => error,
                        None => continue,
                    }
                }
                Err(FetchError::Fatal(message)) => return Err(RangeError::Failed(message)),
                Err(FetchError::Retry(message)) => message,
            };
            failures += 1;
            if failures > limit {
                return Err(RangeError::Failed(error));
            }
            log::warn!("media stream: {error}; retry {failures} of {limit} for a ranged read");
            self.pause(Duration::from_millis(250 << (failures - 1).min(4)))?;
        }
        Ok(())
    }

    fn accept(&mut self, cur: u64, fetched: Fetched) -> Result<Box<dyn Read + Send>, RangeError> {
        let Fetched {
            body,
            offset,
            total,
            ranged,
        } = fetched;
        let Some(len) = self.len else {
            return match (ranged, offset, total) {
                (true, 0, Some(total)) => {
                    self.len = Some(total);
                    Ok(body)
                }
                _ => Err(RangeError::Unsupported),
            };
        };
        if !ranged {
            return Err(RangeError::Failed(
                "the server stopped serving byte ranges".into(),
            ));
        }
        if offset != cur {
            return Err(RangeError::Failed(format!(
                "the server answered from byte {offset} instead of {cur}"
            )));
        }
        if total.is_some_and(|total| total != len) {
            return Err(RangeError::Failed("the file changed on the server".into()));
        }
        Ok(body)
    }

    fn receive(
        &mut self,
        mut cur: u64,
        end: u64,
        mut body: Box<dyn Read + Send>,
    ) -> (u64, Option<String>) {
        while cur < end {
            if (self.stop)() {
                return (cur, None);
            }
            let size = BLOCK.min(end - cur) as usize;
            let mut block = vec![0u8; size];
            let mut filled = 0;
            while filled < size {
                match body.read(&mut block[filled..]) {
                    Ok(0) => {
                        return (cur, Some("the server closed the connection early".into()));
                    }
                    Ok(n) => filled += n,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return (cur, Some(e.to_string())),
                }
            }
            self.insert(cur / BLOCK, block.into_boxed_slice());
            cur += size as u64;
        }
        finish(body.as_mut());
        (cur, None)
    }

    fn insert(&mut self, index: u64, block: Box<[u8]>) {
        if self.blocks.insert(index, block).is_none() {
            self.order.push_back(index);
        }
        while self.order.len() as u64 * BLOCK > CACHE_BYTES {
            if let Some(oldest) = self.order.pop_front() {
                self.blocks.remove(&oldest);
            }
        }
    }

    fn pause(&self, delay: Duration) -> Result<(), RangeError> {
        let deadline = Instant::now() + delay;
        loop {
            if (self.stop)() {
                return Err(RangeError::Stopped);
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(());
            }
            std::thread::sleep((deadline - now).min(STOP_POLL));
        }
    }
}

fn finish(body: &mut dyn Read) {
    let mut rest = [0u8; 1024];
    for _ in 0..4 {
        match body.read(&mut rest) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

impl Read for RangeReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(message) = &self.broken {
            return Err(io::Error::other(message.clone()));
        }
        if buf.is_empty() || self.pos >= self.byte_len() {
            return Ok(0);
        }
        let index = self.pos / BLOCK;
        if !self.blocks.contains_key(&index)
            && let Err(e) = self.load(index)
        {
            if let RangeError::Failed(message) = &e {
                self.failure.set(message.clone());
                self.broken = Some(message.clone());
            }
            return Err(io::Error::other(e.to_string()));
        }
        let Some(block) = self.blocks.get(&index) else {
            return Err(io::Error::other("a block of the file did not arrive"));
        };
        let offset = (self.pos - index * BLOCK) as usize;
        let n = buf.len().min(block.len() - offset);
        buf[..n].copy_from_slice(&block[offset..offset + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for RangeReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let next = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta),
            SeekFrom::End(delta) => self.byte_len().checked_add_signed(delta),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek out of range"))?;
        self.pos = next;
        Ok(next)
    }
}
