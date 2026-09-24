use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

const CHUNK_BYTES: u64 = 4 * 1024 * 1024;
const LOOKAHEAD_BYTES: u64 = 1024 * 1024;
const READ_BUFFER: usize = 64 * 1024;
const MAX_FAILURES: u32 = 6;
const WAIT_SLICE: Duration = Duration::from_millis(250);

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

pub struct Fetched {
    pub body: Box<dyn Read + Send>,
    pub offset: u64,
    pub total: Option<u64>,
    pub ranged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    Retry(String),
    Fatal(String),
}

pub trait RangeFetch: Send + Sync {
    fn fetch(&self, start: u64, end: Option<u64>) -> Result<Fetched, FetchError>;
}

pub type OnComplete = Box<dyn FnOnce(&Path) + Send>;

#[derive(Default, Debug)]
struct Ranges(Vec<(u64, u64)>);

impl Ranges {
    fn insert(&mut self, start: u64, end: u64) {
        if start >= end {
            return;
        }
        let (mut start, mut end) = (start, end);
        let mut merged = Vec::with_capacity(self.0.len() + 1);
        let mut placed = false;
        for &(s, e) in &self.0 {
            if e < start {
                merged.push((s, e));
            } else if s > end {
                if !placed {
                    merged.push((start, end));
                    placed = true;
                }
                merged.push((s, e));
            } else {
                start = start.min(s);
                end = end.max(e);
            }
        }
        if !placed {
            merged.push((start, end));
        }
        self.0 = merged;
    }

    fn available_at(&self, pos: u64) -> u64 {
        self.0
            .iter()
            .find(|(s, e)| *s <= pos && pos < *e)
            .map_or(0, |(_, e)| e - pos)
    }

    fn next_missing(&self, from: u64) -> u64 {
        self.0
            .iter()
            .find(|(s, e)| *s <= from && from < *e)
            .map_or(from, |(_, e)| *e)
    }

    fn next_present(&self, from: u64) -> Option<u64> {
        self.0.iter().map(|(s, _)| *s).find(|s| *s > from)
    }

    fn covers(&self, total: u64) -> bool {
        total == 0 || self.0.first() == Some(&(0, total))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Running,
    Complete,
    Failed(String),
    Cancelled,
}

struct State {
    location: PathBuf,
    total: Option<u64>,
    ranges: Ranges,
    reader_pos: Option<u64>,
    interest: usize,
    outcome: Outcome,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn release(&self) {
        let mut state = self.lock();
        state.interest = state.interest.saturating_sub(1);
        drop(state);
        self.changed.notify_all();
    }
}

#[derive(Clone, Default)]
pub struct Downloads {
    active: Arc<Mutex<HashMap<PathBuf, Weak<Shared>>>>,
}

impl Downloads {
    pub fn start(
        &self,
        dest: &Path,
        fetch: Arc<dyn RangeFetch>,
        on_complete: Option<OnComplete>,
    ) -> io::Result<Download> {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        active.retain(|_, shared| shared.strong_count() > 0);
        if let Some(shared) = active.get(dest).and_then(Weak::upgrade) {
            let mut state = shared.lock();
            let joinable = match state.outcome {
                Outcome::Running => state.interest > 0,
                Outcome::Complete => state.location.exists(),
                Outcome::Failed(_) | Outcome::Cancelled => false,
            };
            if joinable {
                state.interest += 1;
                drop(state);
                return Ok(Download { shared });
            }
        }
        let download = Download::spawn(dest, fetch, on_complete)?;
        active.insert(dest.to_path_buf(), Arc::downgrade(&download.shared));
        Ok(download)
    }

    pub fn is_active(&self, dest: &Path) -> bool {
        let active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        active
            .get(dest)
            .and_then(Weak::upgrade)
            .is_some_and(|shared| shared.lock().outcome == Outcome::Running)
    }
}

pub struct Download {
    shared: Arc<Shared>,
}

impl Download {
    fn spawn(
        dest: &Path,
        fetch: Arc<dyn RangeFetch>,
        on_complete: Option<OnComplete>,
    ) -> io::Result<Self> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut name = dest.as_os_str().to_owned();
        name.push(format!(
            ".{}-{}.partial",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let partial = PathBuf::from(name);
        let file = File::create(&partial)?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                location: partial.clone(),
                total: None,
                ranges: Ranges::default(),
                reader_pos: None,
                interest: 1,
                outcome: Outcome::Running,
            }),
            changed: Condvar::new(),
        });
        let worker = Worker {
            shared: Arc::clone(&shared),
            fetch,
            file,
            partial: partial.clone(),
            dest: dest.to_path_buf(),
            ranged: true,
        };
        let spawned = std::thread::Builder::new()
            .name("media-stream".into())
            .spawn(move || worker.run(on_complete));
        if let Err(e) = spawned {
            let _ = std::fs::remove_file(&partial);
            return Err(e);
        }
        Ok(Self { shared })
    }

    pub fn reader(&self) -> io::Result<StreamReader> {
        let mut state = self.shared.lock();
        let file = File::open(&state.location)?;
        state.interest += 1;
        Ok(StreamReader {
            shared: Arc::clone(&self.shared),
            file,
            pos: 0,
            aborted: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn wait(&self) -> Result<PathBuf, String> {
        self.wait_while(|| false)
    }

    pub fn wait_while(&self, abandoned: impl Fn() -> bool) -> Result<PathBuf, String> {
        let mut state = self.shared.lock();
        loop {
            match &state.outcome {
                Outcome::Complete => return Ok(state.location.clone()),
                Outcome::Failed(message) => return Err(message.clone()),
                Outcome::Cancelled => return Err("the download was cancelled".into()),
                Outcome::Running if abandoned() => return Err("the download was abandoned".into()),
                Outcome::Running => {
                    state = self
                        .shared
                        .changed
                        .wait_timeout(state, WAIT_SLICE)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
            }
        }
    }
}

impl Clone for Download {
    fn clone(&self) -> Self {
        self.shared.lock().interest += 1;
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl Drop for Download {
    fn drop(&mut self) {
        self.shared.release();
    }
}

#[derive(Clone)]
pub struct AbortHandle {
    shared: Arc<Shared>,
    aborted: Arc<AtomicBool>,
}

impl AbortHandle {
    pub fn abort(&self) {
        self.aborted.store(true, Ordering::Release);
        let _state = self.shared.lock();
        self.shared.changed.notify_all();
    }
}

pub struct StreamReader {
    shared: Arc<Shared>,
    file: File,
    pos: u64,
    aborted: Arc<AtomicBool>,
}

impl StreamReader {
    pub fn abort_handle(&self) -> AbortHandle {
        AbortHandle {
            shared: Arc::clone(&self.shared),
            aborted: Arc::clone(&self.aborted),
        }
    }

    pub fn byte_len(&self) -> Option<u64> {
        self.shared.lock().total
    }

    fn wait_until<T>(&self, mut ready: impl FnMut(&State) -> Option<T>) -> io::Result<T> {
        let mut state = self.shared.lock();
        loop {
            if self.aborted.load(Ordering::Acquire) {
                return Err(io::Error::other("the stream was closed"));
            }
            if let Some(value) = ready(&state) {
                return Ok(value);
            }
            match &state.outcome {
                Outcome::Failed(message) => return Err(io::Error::other(message.clone())),
                Outcome::Cancelled => return Err(io::Error::other("the download was cancelled")),
                Outcome::Running | Outcome::Complete => {}
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, WAIT_SLICE)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

impl Read for StreamReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let pos = self.pos;
        {
            let mut state = self.shared.lock();
            state.reader_pos = Some(pos);
        }
        let available = self.wait_until(|state| {
            let available = state.ranges.available_at(pos);
            if available > 0 {
                return Some(available);
            }
            let at_end = state.total.is_some_and(|total| pos >= total);
            (at_end || state.outcome == Outcome::Complete).then_some(0)
        })?;
        if available == 0 {
            return Ok(0);
        }
        let n = (buf.len() as u64).min(available) as usize;
        self.file.seek(SeekFrom::Start(pos))?;
        self.file.read_exact(&mut buf[..n])?;
        self.pos = pos + n as u64;
        Ok(n)
    }
}

impl Seek for StreamReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let next = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta),
            SeekFrom::End(delta) => {
                let total = self.wait_until(|state| state.total)?;
                total.checked_add_signed(delta)
            }
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek out of range"))?;
        self.pos = next;
        self.shared.lock().reader_pos = Some(next);
        Ok(next)
    }
}

impl Drop for StreamReader {
    fn drop(&mut self) {
        self.shared.release();
    }
}

enum Stop {
    Cancelled,
    Failed(String),
}

struct Worker {
    shared: Arc<Shared>,
    fetch: Arc<dyn RangeFetch>,
    file: File,
    partial: PathBuf,
    dest: PathBuf,
    ranged: bool,
}

impl Worker {
    fn run(mut self, on_complete: Option<OnComplete>) {
        let result = self.download();
        let Worker {
            shared,
            file,
            partial,
            dest,
            ..
        } = self;
        drop(file);
        let mut state = shared.lock();
        let completed = match result {
            Ok(()) => {
                match std::fs::rename(&partial, &dest) {
                    Ok(()) => state.location = dest.clone(),
                    Err(e) => log::warn!("media stream: keeping {}: {e}", partial.display()),
                }
                state.outcome = Outcome::Complete;
                true
            }
            Err(stop) => {
                let _ = std::fs::remove_file(&partial);
                state.outcome = match stop {
                    Stop::Cancelled => Outcome::Cancelled,
                    Stop::Failed(message) => {
                        log::warn!("media stream: {} failed: {message}", dest.display());
                        Outcome::Failed(message)
                    }
                };
                false
            }
        };
        let location = state.location.clone();
        drop(state);
        shared.changed.notify_all();
        if completed
            && location == dest
            && let Some(on_complete) = on_complete
        {
            on_complete(&dest);
        }
    }

    fn download(&mut self) -> Result<(), Stop> {
        let mut failures = 0u32;
        loop {
            let Some((start, end)) = self.next_request()? else {
                return Ok(());
            };
            let error = match self.fetch.fetch(start, end) {
                Ok(fetched) => match self.receive(start, end, fetched)? {
                    (true, _) => {
                        failures = 0;
                        continue;
                    }
                    (false, None) => continue,
                    (false, Some(error)) => error,
                },
                Err(FetchError::Fatal(message)) => return Err(Stop::Failed(message)),
                Err(FetchError::Retry(message)) => message,
            };
            self.back_off(&mut failures, error)?;
        }
    }

    fn next_request(&self) -> Result<Option<(u64, Option<u64>)>, Stop> {
        let state = self.shared.lock();
        if state.total.is_some_and(|total| state.ranges.covers(total)) {
            return Ok(None);
        }
        if state.interest == 0 {
            return Err(Stop::Cancelled);
        }
        let Some(total) = state.total else {
            let start = if self.ranged {
                state.ranges.next_missing(0)
            } else {
                0
            };
            return Ok(Some((start, self.ranged.then_some(start + CHUNK_BYTES))));
        };
        if !self.ranged {
            return Ok(Some((0, None)));
        }
        let anchor = state.reader_pos.filter(|pos| *pos < total).unwrap_or(0);
        let mut start = state.ranges.next_missing(anchor);
        if start >= total {
            start = state.ranges.next_missing(0);
        }
        let gap_end = state.ranges.next_present(start).unwrap_or(total);
        Ok(Some((start, Some(gap_end.min(start + CHUNK_BYTES)))))
    }

    fn receive(
        &mut self,
        start: u64,
        end: Option<u64>,
        fetched: Fetched,
    ) -> Result<(bool, Option<String>), Stop> {
        let Fetched {
            mut body,
            offset,
            total,
            ranged,
        } = fetched;
        if !ranged {
            self.ranged = false;
        } else if offset != start {
            return Err(Stop::Failed(format!(
                "the server answered from byte {offset} instead of {start}"
            )));
        }
        {
            let mut state = self.shared.lock();
            match (state.total, total) {
                (Some(known), Some(new)) if known != new => {
                    return Err(Stop::Failed("the file changed on the server".into()));
                }
                (None, Some(new)) => {
                    self.file
                        .set_len(new)
                        .map_err(|e| Stop::Failed(e.to_string()))?;
                    state.total = Some(new);
                }
                _ => {}
            }
        }
        let limit = if self.ranged { end } else { None };
        let mut cur = offset;
        let mut progress = false;
        let mut buf = vec![0u8; READ_BUFFER];
        loop {
            {
                let state = self.shared.lock();
                if state.interest == 0 {
                    return Err(Stop::Cancelled);
                }
                if self.ranged && reader_starved_elsewhere(&state, cur) {
                    return Ok((progress, None));
                }
            }
            let want = limit.map_or(READ_BUFFER as u64, |limit| {
                limit.saturating_sub(cur).min(READ_BUFFER as u64)
            }) as usize;
            if want == 0 {
                return Ok((progress, None));
            }
            let n = match body.read(&mut buf[..want]) {
                Ok(0) => {
                    let mut state = self.shared.lock();
                    let Some(total) = state.total else {
                        let _ = self.file.set_len(cur);
                        state.total = Some(cur);
                        drop(state);
                        self.shared.changed.notify_all();
                        return Ok((true, None));
                    };
                    let expected = limit.unwrap_or(total).min(total);
                    let early = (cur < expected)
                        .then(|| "the server closed the connection early".to_string());
                    return Ok((progress, early));
                }
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Ok((progress, Some(e.to_string()))),
            };
            self.file
                .seek(SeekFrom::Start(cur))
                .and_then(|_| self.file.write_all(&buf[..n]))
                .map_err(|e| Stop::Failed(e.to_string()))?;
            let mut state = self.shared.lock();
            if cur + n as u64 > state.ranges.next_missing(cur) {
                progress = true;
            }
            state.ranges.insert(cur, cur + n as u64);
            cur += n as u64;
            let reached_known = self.ranged && state.ranges.available_at(cur) > 0;
            let at_end = state.total.is_some_and(|total| cur >= total);
            drop(state);
            self.shared.changed.notify_all();
            if reached_known || at_end {
                return Ok((true, None));
            }
        }
    }

    fn back_off(&self, failures: &mut u32, message: String) -> Result<(), Stop> {
        *failures += 1;
        if *failures > MAX_FAILURES {
            return Err(Stop::Failed(message));
        }
        log::warn!(
            "media stream: {message}; retry {failures} of {MAX_FAILURES} for {}",
            self.dest.display()
        );
        let delay = Duration::from_millis(250 << (*failures - 1).min(4));
        let deadline = Instant::now() + delay;
        let mut state = self.shared.lock();
        loop {
            if state.interest == 0 {
                return Err(Stop::Cancelled);
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(());
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

fn reader_starved_elsewhere(state: &State, cur: u64) -> bool {
    let Some(wanted) = state.reader_pos else {
        return false;
    };
    let in_file = state.total.is_none_or(|total| wanted < total);
    in_file
        && state.ranges.available_at(wanted) == 0
        && !(cur <= wanted && wanted < cur + LOOKAHEAD_BYTES)
}

#[cfg(test)]
mod tests;
