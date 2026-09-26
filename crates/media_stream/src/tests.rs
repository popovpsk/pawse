use std::sync::atomic::AtomicU32;
use std::sync::mpsc;

use super::*;

struct Throttled {
    data: Arc<Vec<u8>>,
    pos: usize,
    end: usize,
    step: usize,
    delay: Duration,
    cut_at: Option<usize>,
    linger: Duration,
}

impl Drop for Throttled {
    fn drop(&mut self) {
        std::thread::sleep(self.linger);
    }
}

impl Read for Throttled {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cut_at.is_some_and(|cut| self.pos >= cut) {
            return Err(io::Error::new(io::ErrorKind::ConnectionReset, "reset"));
        }
        if self.pos >= self.end {
            return Ok(0);
        }
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        let mut n = buf.len().min(self.step).min(self.end - self.pos);
        if let Some(cut) = self.cut_at {
            n = n.min(cut - self.pos);
        }
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

struct Fake {
    data: Arc<Vec<u8>>,
    ranged: bool,
    report_total: bool,
    step: usize,
    delay: Duration,
    retry_first: AtomicU32,
    cut_first_at: Mutex<Option<usize>>,
    fatal: bool,
    cut_always_at: Option<usize>,
    linger: Duration,
    requests: Mutex<Vec<(u64, Option<u64>)>>,
}

impl Fake {
    fn new(len: usize) -> Self {
        Self {
            data: Arc::new((0..len).map(|i| (i * 31 % 251) as u8).collect()),
            ranged: true,
            report_total: true,
            step: READ_BUFFER,
            delay: Duration::ZERO,
            retry_first: AtomicU32::new(0),
            cut_first_at: Mutex::new(None),
            fatal: false,
            cut_always_at: None,
            linger: Duration::ZERO,
            requests: Mutex::new(Vec::new()),
        }
    }

    fn slow(mut self, step: usize, delay_ms: u64) -> Self {
        self.step = step;
        self.delay = Duration::from_millis(delay_ms);
        self
    }

    fn requests(&self) -> Vec<(u64, Option<u64>)> {
        self.requests.lock().unwrap().clone()
    }
}

impl RangeFetch for Fake {
    fn fetch(&self, start: u64, end: Option<u64>) -> Result<Fetched, FetchError> {
        self.requests.lock().unwrap().push((start, end));
        if self.fatal {
            return Err(FetchError::Fatal("gone".into()));
        }
        if self
            .retry_first
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(FetchError::Retry("busy".into()));
        }
        let len = self.data.len();
        let (offset, stop) = if self.ranged {
            (
                start as usize,
                end.map_or(len, |end| (end as usize).min(len)),
            )
        } else {
            (0, len)
        };
        Ok(Fetched {
            body: Box::new(Throttled {
                data: Arc::clone(&self.data),
                pos: offset,
                end: stop,
                step: self.step,
                delay: self.delay,
                cut_at: self
                    .cut_first_at
                    .lock()
                    .unwrap()
                    .take()
                    .or(self.cut_always_at),
                linger: self.linger,
            }),
            offset: offset as u64,
            total: self.report_total.then_some(len as u64),
            ranged: self.ranged,
        })
    }
}

fn temp_dest(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pawse-media-stream-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dest = dir.join(name);
    let _ = std::fs::remove_file(&dest);
    dest
}

fn leftovers(dest: &Path) -> Vec<PathBuf> {
    let name = dest.file_name().unwrap().to_string_lossy().to_string();
    std::fs::read_dir(dest.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let file = p.file_name().unwrap().to_string_lossy();
            file.starts_with(&name) && file.ends_with(".partial")
        })
        .collect()
}

fn start(fake: &Arc<Fake>, dest: &Path) -> Download {
    Downloads::default()
        .start(dest, Arc::clone(fake) as Arc<dyn RangeFetch>, None)
        .unwrap()
}

#[test]
fn ranges_merge_overlapping_and_adjacent_spans() {
    let mut ranges = Ranges::default();
    ranges.insert(10, 20);
    ranges.insert(30, 40);
    ranges.insert(20, 25);
    assert_eq!(ranges.0, vec![(10, 25), (30, 40)]);
    ranges.insert(0, 5);
    ranges.insert(24, 31);
    assert_eq!(ranges.0, vec![(0, 5), (10, 40)]);
    assert_eq!(ranges.available_at(12), 28);
    assert_eq!(ranges.available_at(5), 0);
    assert_eq!(ranges.next_missing(3), 5);
    assert_eq!(ranges.next_missing(7), 7);
    assert_eq!(ranges.next_present(7), Some(10));
    assert!(!ranges.covers(40));
    ranges.insert(5, 10);
    assert!(ranges.covers(40));
}

#[test]
fn a_sequential_read_returns_the_file_and_leaves_it_in_the_cache() {
    let fake = Arc::new(Fake::new(9 * 1024 * 1024 + 123));
    let dest = temp_dest("sequential.flac");
    let (tx, rx) = mpsc::channel();
    let download = Downloads::default()
        .start(
            &dest,
            Arc::clone(&fake) as Arc<dyn RangeFetch>,
            Some(Box::new(move |path: &Path| {
                tx.send(path.to_path_buf()).unwrap();
            })),
        )
        .unwrap();
    let mut reader = download.reader().unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, *fake.data);
    assert_eq!(reader.byte_len(), Some(fake.data.len() as u64));
    assert_eq!(download.wait().unwrap(), dest);
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), dest);
    assert_eq!(std::fs::read(&dest).unwrap(), *fake.data);
    assert!(leftovers(&dest).is_empty());
    assert!(
        fake.requests()
            .iter()
            .all(|(start, end)| end.is_some_and(|end| end - start <= CHUNK_BYTES))
    );
}

#[test]
fn a_seek_ahead_fetches_from_the_new_position_and_the_gap_is_filled_later() {
    let fake = Arc::new(Fake::new(12 * 1024 * 1024).slow(8 * 1024, 2));
    let dest = temp_dest("seek.flac");
    let download = start(&fake, &dest);
    let mut reader = download.reader().unwrap();
    let mut head = [0u8; 1000];
    reader.read_exact(&mut head).unwrap();
    assert_eq!(head[..], fake.data[..1000]);

    let target = 10 * 1024 * 1024 + 7;
    let began = Instant::now();
    reader.seek(SeekFrom::Start(target)).unwrap();
    let mut middle = [0u8; 4096];
    reader.read_exact(&mut middle).unwrap();
    assert!(began.elapsed() < Duration::from_secs(3));
    assert_eq!(
        middle[..],
        fake.data[target as usize..target as usize + 4096]
    );
    assert!(fake.requests().iter().any(|(start, _)| *start == target));

    assert_eq!(download.wait().unwrap(), dest);
    assert_eq!(std::fs::read(&dest).unwrap(), *fake.data);
}

#[test]
fn a_server_that_ignores_ranges_is_read_from_the_start() {
    let mut fake = Fake::new(6 * 1024 * 1024);
    fake.ranged = false;
    let fake = Arc::new(fake);
    let dest = temp_dest("unranged.mp3");
    let download = start(&fake, &dest);
    let mut reader = download.reader().unwrap();
    reader.seek(SeekFrom::Start(5 * 1024 * 1024)).unwrap();
    let mut tail = Vec::new();
    reader.read_to_end(&mut tail).unwrap();
    assert_eq!(tail[..], fake.data[5 * 1024 * 1024..]);
    assert_eq!(download.wait().unwrap(), dest);
    assert_eq!(std::fs::read(&dest).unwrap(), *fake.data);
    assert_eq!(fake.requests(), vec![(0, Some(CHUNK_BYTES))]);
}

#[test]
fn an_unknown_length_ends_where_the_body_ends() {
    let mut fake = Fake::new(300_000);
    fake.ranged = false;
    fake.report_total = false;
    let fake = Arc::new(fake);
    let dest = temp_dest("unknown.ogg");
    let download = start(&fake, &dest);
    let mut reader = download.reader().unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, *fake.data);
    assert_eq!(reader.byte_len(), Some(300_000));
    assert_eq!(reader.seek(SeekFrom::End(-10)).unwrap(), 299_990);
    assert_eq!(download.wait().unwrap(), dest);
}

#[test]
fn transient_failures_and_dropped_connections_are_retried() {
    let fake = Fake::new(5 * 1024 * 1024);
    fake.retry_first.store(2, Ordering::SeqCst);
    *fake.cut_first_at.lock().unwrap() = Some(100_000);
    let fake = Arc::new(fake);
    let dest = temp_dest("retry.flac");
    let download = start(&fake, &dest);
    let mut reader = download.reader().unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, *fake.data);
    assert_eq!(download.wait().unwrap(), dest);
    assert!(fake.requests().len() >= 4);
}

#[test]
fn a_fatal_error_fails_readers_and_leaves_nothing_behind() {
    let mut fake = Fake::new(1000);
    fake.fatal = true;
    let fake = Arc::new(fake);
    let dest = temp_dest("fatal.flac");
    let download = start(&fake, &dest);
    let mut reader = download.reader().unwrap();
    let mut buf = [0u8; 10];
    let error = reader.read(&mut buf).unwrap_err();
    assert!(error.to_string().contains("gone"));
    assert!(download.wait().is_err());
    assert!(!dest.exists());
    assert!(leftovers(&dest).is_empty());
}

#[test]
fn a_stream_that_already_delivered_bytes_retries_longer_than_one_that_never_started() {
    let fake = Fake::new(300_000).slow(10_000, 20);
    *fake.cut_first_at.lock().unwrap() = Some(100_000);
    let fake = Arc::new(fake);
    let dest = temp_dest("late-retry.flac");
    let download = start(&fake, &dest);
    let mut reader = download.reader().unwrap();
    let mut first = vec![0u8; 50_000];
    reader.read_exact(&mut first).unwrap();
    fake.retry_first
        .store(FIRST_BYTE_FAILURES + 1, Ordering::SeqCst);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    first.extend(bytes);
    assert_eq!(first, *fake.data);
    assert_eq!(download.wait().unwrap(), dest);
}

#[test]
fn giving_up_after_repeated_transient_failures() {
    let fake = Fake::new(1000);
    fake.retry_first.store(u32::MAX, Ordering::SeqCst);
    let fake = Arc::new(fake);
    let dest = temp_dest("offline.flac");
    let download = start(&fake, &dest);
    assert_eq!(download.wait().unwrap_err(), "busy");
    assert_eq!(fake.requests().len() as u32, FIRST_BYTE_FAILURES + 1);
    assert!(leftovers(&dest).is_empty());
}

#[test]
fn dropping_every_handle_cancels_the_download() {
    let fake = Arc::new(Fake::new(20 * 1024 * 1024).slow(4 * 1024, 2));
    let dest = temp_dest("cancel.flac");
    let download = start(&fake, &dest);
    let mut reader = download.reader().unwrap();
    let mut buf = [0u8; 100];
    reader.read_exact(&mut buf).unwrap();
    drop(reader);
    drop(download);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !leftovers(&dest).is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(leftovers(&dest).is_empty());
    assert!(!dest.exists());
    let seen = fake.requests().len();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fake.requests().len(), seen);
}

#[test]
fn aborting_wakes_a_reader_that_waits_for_data() {
    let fake = Arc::new(Fake::new(10 * 1024 * 1024).slow(1024, 50));
    let dest = temp_dest("abort.flac");
    let download = start(&fake, &dest);
    let mut reader = download.reader().unwrap();
    reader.seek(SeekFrom::Start(9 * 1024 * 1024)).unwrap();
    let abort = reader.abort_handle();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 8 * 1024 * 1024];
        let _ = tx.send(reader.read_exact(&mut buf).map_err(|e| e.to_string()));
    });
    std::thread::sleep(Duration::from_millis(100));
    abort.abort();
    let result = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(result.is_err());
}

#[test]
fn a_superseded_reader_stops_waiting_but_reads_what_is_already_there() {
    let fake = Arc::new(Fake::new(10 * 1024 * 1024).slow(1024, 1000));
    let dest = temp_dest("superseded.flac");
    let download = start(&fake, &dest);
    let mut reader = download.reader().unwrap();
    let newer = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&newer);
    reader.give_up_waiting_when(Box::new(move || flag.load(Ordering::SeqCst)));
    let mut head = [0u8; 1];
    reader.read_exact(&mut head).unwrap();
    reader.seek(SeekFrom::Start(9 * 1024 * 1024)).unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        let far = reader.read(&mut buf).map_err(|e| e.to_string());
        reader.seek(SeekFrom::Start(0)).unwrap();
        let near = reader.read(&mut buf).map_err(|e| e.to_string());
        let _ = tx.send((far, near));
    });
    std::thread::sleep(Duration::from_millis(100));
    newer.store(true, Ordering::SeqCst);
    let (far, near) = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(far.is_err());
    assert!(near.unwrap() > 0);
}

#[test]
fn a_second_start_for_the_same_file_joins_the_running_download() {
    let fake = Arc::new(Fake::new(3 * 1024 * 1024).slow(64 * 1024, 5));
    let dest = temp_dest("shared.flac");
    let downloads = Downloads::default();
    let first = downloads
        .start(&dest, Arc::clone(&fake) as Arc<dyn RangeFetch>, None)
        .unwrap();
    assert!(downloads.is_active(&dest));
    let second = downloads
        .start(&dest, Arc::clone(&fake) as Arc<dyn RangeFetch>, None)
        .unwrap();
    drop(first);
    let mut bytes = Vec::new();
    second.reader().unwrap().read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, *fake.data);
    assert_eq!(second.wait().unwrap(), dest);
    assert_eq!(fake.requests()[0], (0, Some(CHUNK_BYTES)));
    assert_eq!(fake.requests().iter().filter(|(s, _)| *s == 0).count(), 1);
    assert!(!downloads.is_active(&dest));
}

#[test]
fn a_file_read_to_the_end_is_kept_even_when_the_reader_leaves_first() {
    let mut fake = Fake::new(200_000);
    fake.linger = Duration::from_millis(200);
    let fake = Arc::new(fake);
    let dest = temp_dest("race.flac");
    let download = start(&fake, &dest);
    let mut reader = download.reader().unwrap();
    drop(download);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    drop(reader);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !dest.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(dest.exists());
    assert_eq!(std::fs::read(&dest).unwrap(), *fake.data);
}

#[test]
fn a_server_without_ranges_that_keeps_dropping_at_the_same_byte_gives_up() {
    let mut fake = Fake::new(1_000_000);
    fake.ranged = false;
    fake.cut_always_at = Some(300_000);
    let fake = Arc::new(fake);
    let dest = temp_dest("unranged-drop.flac");
    let download = start(&fake, &dest);
    assert_eq!(download.wait().unwrap_err(), "reset");
    assert!(fake.requests().len() as u32 <= MAX_FAILURES + 2);
    assert!(leftovers(&dest).is_empty());
}

#[test]
fn a_finished_download_whose_file_was_evicted_starts_over() {
    let fake = Arc::new(Fake::new(100_000));
    let dest = temp_dest("evicted.flac");
    let downloads = Downloads::default();
    let first = downloads
        .start(&dest, Arc::clone(&fake) as Arc<dyn RangeFetch>, None)
        .unwrap();
    assert_eq!(first.wait().unwrap(), dest);
    std::fs::remove_file(&dest).unwrap();
    let second = downloads
        .start(&dest, Arc::clone(&fake) as Arc<dyn RangeFetch>, None)
        .unwrap();
    let mut bytes = Vec::new();
    second.reader().unwrap().read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, *fake.data);
    assert_eq!(second.wait().unwrap(), dest);
    assert_eq!(fake.requests().len(), 2);
}

#[test]
fn an_abandoned_wait_returns_and_lets_the_download_go() {
    let fake = Arc::new(Fake::new(20 * 1024 * 1024).slow(4 * 1024, 5));
    let dest = temp_dest("abandoned.flac");
    let download = start(&fake, &dest);
    let gave_up = AtomicBool::new(false);
    let began = Instant::now();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(100));
            gave_up.store(true, Ordering::SeqCst);
        });
        assert!(
            download
                .wait_while(|| gave_up.load(Ordering::SeqCst))
                .is_err()
        );
    });
    assert!(began.elapsed() < Duration::from_secs(2));
    drop(download);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !leftovers(&dest).is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(leftovers(&dest).is_empty());
}
