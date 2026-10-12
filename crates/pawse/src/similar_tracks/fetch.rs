use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use audio_decoder::{Decoder, MediaStream};
use audio_embedding::EmbedError;
use media_stream::{RangeError, RangeReader};
use music_library::Track;
use music_library::remote::RemoteRef;

use super::worker::{Done, Lanes, Ready, prepare, prepare_with};
use crate::remote_media::RemoteMedia;

pub(super) const PER_SOURCE: usize = 2;
pub(super) const PER_SOURCE_RANGED: usize = 4;
pub(super) const MAX_DOWNLOADS: usize = 8;
const GATE_POLL: Duration = Duration::from_millis(250);

pub(super) type Stopped = Arc<dyn Fn() -> bool + Send + Sync>;

pub(super) struct ServerFile {
    pub(super) reference: RemoteRef,
    pub(super) tracks: Vec<Track>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Ranges,
    Whole,
}

struct Queue {
    source_id: i64,
    files: VecDeque<ServerFile>,
    ranges: bool,
    active: usize,
    whole: usize,
    down: bool,
}

impl Queue {
    fn mode(&self, file: &ServerFile) -> Mode {
        match self.ranges && read_in_part(&file.reference.suffix) {
            true => Mode::Ranges,
            false => Mode::Whole,
        }
    }
}

fn read_in_part(suffix: &str) -> bool {
    matches!(
        suffix.to_ascii_lowercase().as_str(),
        "flac" | "wav" | "wave" | "aif" | "aiff"
    )
}

struct Board {
    queues: Vec<Queue>,
    per_source: usize,
    per_source_ranged: usize,
}

impl Board {
    fn new(
        files: Vec<ServerFile>,
        torrent: impl Fn(i64) -> bool,
        per_source: usize,
        per_source_ranged: usize,
    ) -> Self {
        let mut queues: Vec<Queue> = Vec::new();
        for file in files {
            let source_id = file.reference.source_id;
            match queues.iter_mut().find(|queue| queue.source_id == source_id) {
                Some(queue) => queue.files.push_back(file),
                None => queues.push(Queue {
                    source_id,
                    files: VecDeque::from([file]),
                    ranges: !torrent(source_id),
                    active: 0,
                    whole: 0,
                    down: false,
                }),
            }
        }
        queues.sort_by_key(|queue| !queue.ranges);
        Self {
            queues,
            per_source,
            per_source_ranged,
        }
    }

    fn limit(&self, queue: &Queue) -> usize {
        match queue.ranges {
            true => self.per_source_ranged,
            false => self.per_source,
        }
    }

    fn workers(&self, max: usize) -> usize {
        self.queues
            .iter()
            .map(|queue| queue.files.len().min(self.limit(queue)))
            .sum::<usize>()
            .min(max)
    }

    fn take(&mut self) -> Option<(usize, ServerFile, Mode)> {
        for ix in 0..self.queues.len() {
            let limit = self.limit(&self.queues[ix]);
            let per_source = self.per_source;
            let queue = &mut self.queues[ix];
            if queue.down || queue.active >= limit {
                continue;
            }
            let fits = queue
                .files
                .iter()
                .position(|file| queue.mode(file) == Mode::Ranges || queue.whole < per_source);
            let Some(file) = fits.and_then(|at| queue.files.remove(at)) else {
                continue;
            };
            let mode = queue.mode(&file);
            queue.active += 1;
            if mode == Mode::Whole {
                queue.whole += 1;
            }
            return Some((ix, file, mode));
        }
        None
    }

    fn finish(&mut self, ix: usize, mode: Mode) {
        let queue = &mut self.queues[ix];
        queue.active = queue.active.saturating_sub(1);
        if mode == Mode::Whole {
            queue.whole = queue.whole.saturating_sub(1);
        }
    }

    fn no_ranges(&mut self, ix: usize, file: ServerFile) -> bool {
        let queue = &mut self.queues[ix];
        queue.files.push_front(file);
        std::mem::replace(&mut queue.ranges, false)
    }

    fn give_up(&mut self, ix: usize) -> usize {
        let queue = &mut self.queues[ix];
        queue.down = true;
        queue.files.drain(..).map(|file| file.tracks.len()).sum()
    }
}

enum Outcome {
    Done,
    Unreachable(usize),
    Stop,
}

struct Gate {
    free: Mutex<usize>,
    freed: Condvar,
}

impl Gate {
    fn new(slots: usize) -> Self {
        Self {
            free: Mutex::new(slots.max(1)),
            freed: Condvar::new(),
        }
    }

    fn enter(&self, halted: impl Fn() -> bool) -> Option<Permit<'_>> {
        let mut free = self.free.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if halted() {
                return None;
            }
            if *free > 0 {
                *free -= 1;
                return Some(Permit(self));
            }
            free = self
                .freed
                .wait_timeout(free, GATE_POLL)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

struct Permit<'a>(&'a Gate);

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        *self.0.free.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.0.freed.notify_one();
    }
}

pub(super) struct Fetcher<'a> {
    pub(super) lanes: Lanes<'a>,
    pub(super) media: &'a RemoteMedia,
    pub(super) scratch: &'a Path,
    pub(super) unreachable: &'a AtomicUsize,
    pub(super) superseded: &'a (dyn Fn() -> bool + Sync),
    pub(super) stopped: Stopped,
    pub(super) per_source: usize,
    pub(super) per_source_ranged: usize,
    pub(super) max_downloads: usize,
    pub(super) prepare_slots: usize,
}

impl Fetcher<'_> {
    pub(super) fn run(
        &self,
        files: Vec<ServerFile>,
        ready: &flume::Sender<Ready>,
        done: &flume::Sender<Done>,
    ) {
        if files.is_empty() {
            return;
        }
        if let Err(e) = std::fs::create_dir_all(self.scratch) {
            log::error!(
                "similar tracks: no folder for downloads at {}: {e}",
                self.scratch.display()
            );
            let tracks = files.iter().map(|file| file.tracks.len()).sum();
            self.unreachable.fetch_add(tracks, Ordering::Relaxed);
            return;
        }
        let board = Board::new(
            files,
            |id| self.media.is_torrent(id),
            self.per_source,
            self.per_source_ranged,
        );
        let workers = board.workers(self.max_downloads);
        let board = Mutex::new(board);
        let gate = Gate::new(self.prepare_slots);
        std::thread::scope(|scope| {
            for n in 0..workers {
                let spawned = std::thread::Builder::new()
                    .name(format!("similar-fetch-{n}"))
                    .spawn_scoped(scope, || self.work(&board, &gate, ready, done));
                if let Err(e) = spawned {
                    log::error!("similar tracks: could not start a download thread: {e}");
                    break;
                }
            }
        });
        if !self.lanes.halted() && (self.superseded)() {
            log::info!("similar tracks: downloads give way to a new pass");
        }
    }

    fn work(
        &self,
        board: &Mutex<Board>,
        gate: &Gate,
        ready: &flume::Sender<Ready>,
        done: &flume::Sender<Done>,
    ) {
        loop {
            if self.lanes.halted() || (self.superseded)() {
                return;
            }
            let Some((ix, file, mode)) = lock(board).take() else {
                return;
            };
            let fetch = || self.fetch(board, ix, file, mode, gate, ready, done);
            let outcome = catch_unwind(AssertUnwindSafe(fetch)).unwrap_or_else(|_| {
                log::error!("similar tracks: a download stopped on a panic");
                Outcome::Done
            });
            let mut board = lock(board);
            if let Outcome::Unreachable(tracks) = outcome {
                let dropped = board.give_up(ix);
                self.unreachable
                    .fetch_add(tracks + dropped, Ordering::Relaxed);
            }
            board.finish(ix, mode);
            if matches!(outcome, Outcome::Stop) {
                return;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn fetch(
        &self,
        board: &Mutex<Board>,
        ix: usize,
        file: ServerFile,
        mode: Mode,
        gate: &Gate,
        ready: &flume::Sender<Ready>,
        done: &flume::Sender<Done>,
    ) -> Outcome {
        if mode == Mode::Ranges
            && self.media.peek_cached(&file.reference).is_none()
            && !self.media.is_downloading(&file.reference)
        {
            return match self.read_ranges(file, ready, done) {
                Ok(outcome) => outcome,
                Err(rest) => {
                    let source_id = rest.reference.source_id;
                    if lock(board).no_ranges(ix, rest) {
                        log::info!(
                            "similar tracks: source {source_id} does not serve byte ranges, its \
                             files are downloaded whole"
                        );
                    }
                    Outcome::Done
                }
            };
        }
        let source_id = file.reference.source_id;
        let scratch = self.scratch.join(scratch_name(&file));
        let _downloaded = Scratch(scratch.clone());
        let halted = || self.lanes.halted();
        let fetched = match self.media.peek_cached(&file.reference) {
            Some(path) => Ok(path),
            None if self.media.is_downloading(&file.reference) => {
                let locator = file.tracks.first().map_or("", |track| track.path.as_str());
                self.media.resolve(Path::new(locator), &halted)
            }
            None => self.media.download_to(&file.reference, &scratch, &halted),
        };
        let path = match fetched {
            Ok(path) => path,
            Err(_) if halted() => return Outcome::Stop,
            Err(e) => return self.network_failure(source_id, &file.tracks, e, done),
        };
        let Some(_permit) = gate.enter(halted) else {
            return Outcome::Stop;
        };
        for track in file.tracks {
            if halted() {
                return Outcome::Stop;
            }
            let locator = PathBuf::from(&track.path);
            let sent = match prepare(&path, &track) {
                Ok(prepared) => ready
                    .send(Ready {
                        id: track.id,
                        path: locator,
                        prepared,
                    })
                    .is_ok(),
                Err(e) => done
                    .send(Done {
                        id: track.id,
                        path: locator,
                        result: Err(e),
                    })
                    .is_ok(),
            };
            if !sent {
                return Outcome::Stop;
            }
        }
        Outcome::Done
    }

    fn read_ranges(
        &self,
        file: ServerFile,
        ready: &flume::Sender<Ready>,
        done: &flume::Sender<Done>,
    ) -> Result<Outcome, ServerFile> {
        let source_id = file.reference.source_id;
        for n in 0..file.tracks.len() {
            if self.lanes.halted() {
                return Ok(Outcome::Stop);
            }
            let track = &file.tracks[n];
            let rest = &file.tracks[n..];
            let stopped = self.stopped.clone();
            let reader = match self
                .media
                .read_ranges(&file.reference, Box::new(move || stopped()))
            {
                Ok(reader) => reader,
                Err(RangeError::Unsupported) => {
                    return Err(ServerFile {
                        tracks: rest.to_vec(),
                        reference: file.reference,
                    });
                }
                Err(RangeError::Stopped) => return Ok(Outcome::Stop),
                Err(RangeError::Failed(e)) => {
                    return Ok(self.network_failure(source_id, rest, e, done));
                }
            };
            let failure = reader.failure();
            let suffix = file.reference.suffix.as_str();
            let stream = || Decoder::open_stream(Box::new(RangeStream(reader)), Some(suffix));
            let prepared = prepare_with(stream, track);
            if self.lanes.halted() {
                return Ok(Outcome::Stop);
            }
            if let Some(network) = failure.get() {
                return Ok(self.network_failure(source_id, rest, network, done));
            }
            let locator = PathBuf::from(&track.path);
            let sent = match prepared {
                Ok(prepared) => ready
                    .send(Ready {
                        id: track.id,
                        path: locator,
                        prepared,
                    })
                    .is_ok(),
                Err(e) => done
                    .send(Done {
                        id: track.id,
                        path: locator,
                        result: Err(e),
                    })
                    .is_ok(),
            };
            if !sent {
                return Ok(Outcome::Stop);
            }
        }
        Ok(Outcome::Done)
    }

    fn network_failure(
        &self,
        source_id: i64,
        tracks: &[Track],
        error: String,
        done: &flume::Sender<Done>,
    ) -> Outcome {
        if !self.answers(source_id) {
            log::warn!(
                "similar tracks: source {source_id} is unreachable, its tracks wait for the next \
                 pass: {error}"
            );
            return Outcome::Unreachable(tracks.len());
        }
        for track in tracks {
            let failure = Done {
                id: track.id,
                path: PathBuf::from(&track.path),
                result: Err(EmbedError::Network(error.clone())),
            };
            if done.send(failure).is_err() {
                return Outcome::Stop;
            }
        }
        Outcome::Done
    }

    fn answers(&self, source_id: i64) -> bool {
        !self.media.is_torrent(source_id) && matches!(self.media.ping(source_id), Some(Ok(())))
    }
}

fn lock(board: &Mutex<Board>) -> MutexGuard<'_, Board> {
    board.lock().unwrap_or_else(|e| e.into_inner())
}

fn scratch_name(file: &ServerFile) -> String {
    let id = file.tracks.first().map_or(0, |track| track.id);
    match file.reference.suffix.as_str() {
        "" => id.to_string(),
        suffix => format!("{id}.{suffix}"),
    }
}

struct RangeStream(RangeReader);

impl Read for RangeStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for RangeStream {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.0.seek(pos)
    }
}

impl MediaStream for RangeStream {
    fn byte_len(&self) -> Option<u64> {
        Some(self.0.byte_len())
    }
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::remote_media::{KeepAlive, PendingStream, SourceMedia};
    use crate::servers::RemoteError;

    fn server_track(id: i64, path: &str, is_cue: bool) -> Track {
        Track {
            id,
            path: path.into(),
            title: "t".into(),
            album_id: None,
            track_number: None,
            disc_number: 1,
            duration_ms: None,
            year: None,
            cover_art_id: None,
            start_offset_ms: 0,
            liked: false,
            bitrate: None,
            is_cue,
            available: true,
        }
    }

    fn file(source_id: i64, key: &str, ids: &[i64]) -> ServerFile {
        file_of(source_id, key, "flac", ids)
    }

    fn file_of(source_id: i64, key: &str, suffix: &str, ids: &[i64]) -> ServerFile {
        let locator = music_library::remote::locator(source_id, key, suffix);
        ServerFile {
            reference: music_library::remote::parse(&locator).unwrap(),
            tracks: ids
                .iter()
                .map(|&id| server_track(id, &locator, ids.len() > 1))
                .collect(),
        }
    }

    fn keys(board: &mut Board) -> Vec<(usize, String, Mode)> {
        std::iter::from_fn(|| board.take())
            .map(|(ix, file, mode)| (ix, file.reference.key, mode))
            .collect()
    }

    #[test]
    fn each_source_gets_its_own_workers_and_torrents_come_last() {
        let mut board = Board::new(
            vec![
                file(7, "t1", &[1]),
                file(7, "t2", &[6]),
                file(7, "t3", &[7]),
                file(1, "a1", &[2]),
                file(1, "a2", &[3]),
                file(1, "a3", &[4]),
                file(2, "b1", &[5]),
            ],
            |id| id == 7,
            2,
            3,
        );
        assert_eq!(board.workers(8), 6);
        assert_eq!(board.workers(3), 3);
        let (r, w) = (Mode::Ranges, Mode::Whole);
        assert_eq!(
            keys(&mut board),
            vec![
                (0, "a1".into(), r),
                (0, "a2".into(), r),
                (0, "a3".into(), r),
                (1, "b1".into(), r),
                (2, "t1".into(), w),
                (2, "t2".into(), w),
            ]
        );
        board.finish(2, w);
        assert_eq!(keys(&mut board), vec![(2, "t3".into(), w)]);
    }

    #[test]
    fn whole_downloads_keep_their_own_limit_inside_a_ranged_source() {
        let mut board = Board::new(
            vec![
                file_of(1, "x1", "ape", &[1]),
                file_of(1, "x2", "mp3", &[2]),
                file_of(1, "x3", "m4a", &[3]),
                file(1, "f1", &[4]),
                file(1, "f2", &[5]),
            ],
            |_| false,
            2,
            4,
        );
        let (r, w) = (Mode::Ranges, Mode::Whole);
        assert_eq!(
            keys(&mut board),
            vec![
                (0, "x1".into(), w),
                (0, "x2".into(), w),
                (0, "f1".into(), r),
                (0, "f2".into(), r),
            ]
        );
        board.finish(0, r);
        board.finish(0, r);
        assert!(board.no_ranges(0, file(1, "f1", &[4])));
        assert!(!board.no_ranges(0, file(1, "f2", &[5])));
        board.finish(0, w);
        assert_eq!(keys(&mut board), vec![(0, "f2".into(), w)]);
        board.finish(0, w);
        assert_eq!(keys(&mut board), vec![(0, "f1".into(), w)]);
    }

    #[test]
    fn preparing_waits_for_a_free_slot_unless_the_pass_is_stopping() {
        let gate = Gate::new(1);
        let held = gate.enter(|| false).unwrap();
        assert!(gate.enter(|| true).is_none());
        drop(held);
        assert!(gate.enter(|| false).is_some());
    }

    #[test]
    fn an_unreachable_source_hands_back_what_it_had_left() {
        let mut board = Board::new(
            vec![
                file(1, "a1", &[1]),
                file(1, "image", &[2, 3]),
                file(1, "a3", &[4]),
            ],
            |_| false,
            1,
            1,
        );
        let (ix, _, mode) = board.take().unwrap();
        assert_eq!(board.give_up(ix), 3);
        board.finish(ix, mode);
        assert!(board.take().is_none());
    }

    struct Server {
        up: bool,
        refuses: &'static str,
        fetched: Mutex<Vec<String>>,
        ranges: Option<Arc<Bytes>>,
        active: AtomicUsize,
        peak: AtomicUsize,
    }

    impl Server {
        fn new(up: bool, refuses: &'static str) -> Arc<Self> {
            Arc::new(Self {
                up,
                refuses,
                fetched: Mutex::new(Vec::new()),
                ranges: None,
                active: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
            })
        }

        fn with_ranges(up: bool, bytes: Bytes) -> Arc<Self> {
            Arc::new(Self {
                up,
                refuses: "",
                fetched: Mutex::new(Vec::new()),
                ranges: Some(Arc::new(bytes)),
                active: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
            })
        }
    }

    struct Bytes {
        data: Vec<u8>,
        ranged: bool,
        gone_after_first: bool,
        served: AtomicUsize,
    }

    impl Bytes {
        fn of(data: Vec<u8>) -> Self {
            Self {
                data,
                ranged: true,
                gone_after_first: false,
                served: AtomicUsize::new(0),
            }
        }
    }

    impl media_stream::RangeFetch for Bytes {
        fn fetch(
            &self,
            start: u64,
            end: Option<u64>,
        ) -> Result<media_stream::Fetched, media_stream::FetchError> {
            if self.gone_after_first && start > 0 {
                return Err(media_stream::FetchError::Fatal("gone".into()));
            }
            let len = self.data.len();
            let (offset, stop) = match self.ranged {
                true => (
                    start as usize,
                    end.map_or(len, |end| (end as usize).min(len)),
                ),
                false => (0, len),
            };
            self.served.fetch_add(stop - offset, Ordering::Relaxed);
            Ok(media_stream::Fetched {
                body: Box::new(std::io::Cursor::new(self.data[offset..stop].to_vec())),
                offset: offset as u64,
                total: Some(len as u64),
                ranged: self.ranged,
            })
        }
    }

    impl SourceMedia for Server {
        fn ping(&self) -> Result<(), RemoteError> {
            match self.up {
                true => Ok(()),
                false => Err(RemoteError::Unreachable("down".into())),
            }
        }

        fn open(&self, _: &RemoteRef, _: &Path) -> Result<PendingStream, String> {
            Err("not used".into())
        }

        fn fetch_whole(
            &self,
            reference: &RemoteRef,
            dest: &Path,
            _: &dyn Fn() -> bool,
        ) -> Result<PathBuf, String> {
            self.fetched.lock().unwrap().push(reference.key.clone());
            let now = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(30));
            self.active.fetch_sub(1, Ordering::SeqCst);
            if !self.up || reference.key == self.refuses {
                return Err("refused".into());
            }
            std::fs::write(dest, b"not audio").unwrap();
            Ok(dest.to_path_buf())
        }

        fn prefetch(&self, _: &RemoteRef, _: &Path) -> Result<KeepAlive, String> {
            Ok(Box::new(()))
        }

        fn cover(&self, _: &str, _: u32) -> Result<Vec<u8>, RemoteError> {
            Ok(Vec::new())
        }

        fn range_fetch(&self, _: &RemoteRef) -> Option<Arc<dyn media_stream::RangeFetch>> {
            self.ranges
                .clone()
                .map(|bytes| bytes as Arc<dyn media_stream::RangeFetch>)
        }
    }

    struct Rig {
        _cache: tempfile::TempDir,
        scratch: tempfile::TempDir,
        media: RemoteMedia,
    }

    fn rig(sources: Vec<(i64, Arc<Server>)>) -> Rig {
        let cache = tempfile::tempdir().unwrap();
        let media =
            RemoteMedia::with_cache(crate::remote_media::CacheStore::new(cache.path().into()));
        media.set_sources(
            sources
                .into_iter()
                .map(|(id, server)| (id, server as Arc<dyn SourceMedia>))
                .collect::<HashMap<_, _>>(),
        );
        Rig {
            _cache: cache,
            scratch: tempfile::tempdir().unwrap(),
            media,
        }
    }

    fn run(rig: &Rig, files: Vec<ServerFile>, per_source: usize, max: usize) -> (usize, Vec<Done>) {
        let (unreachable, ready, done) = run_all(rig, files, (per_source, per_source), max);
        assert!(ready.is_empty());
        (unreachable, done)
    }

    fn run_all(
        rig: &Rig,
        files: Vec<ServerFile>,
        (per_source, per_source_ranged): (usize, usize),
        max: usize,
    ) -> (usize, Vec<Ready>, Vec<Done>) {
        let (stop, halt) = (AtomicBool::new(false), AtomicBool::new(false));
        let unreachable = AtomicUsize::new(0);
        let fetcher = Fetcher {
            lanes: Lanes {
                stop: &stop,
                halt: &halt,
            },
            media: &rig.media,
            scratch: rig.scratch.path(),
            unreachable: &unreachable,
            superseded: &|| false,
            stopped: Arc::new(|| false),
            per_source,
            per_source_ranged,
            max_downloads: max,
            prepare_slots: 1,
        };
        let (ready_tx, ready) = flume::unbounded();
        let (done_tx, done) = flume::unbounded();
        fetcher.run(files, &ready_tx, &done_tx);
        let mut ready: Vec<Ready> = ready.try_iter().collect();
        ready.sort_by_key(|item| item.id);
        let mut done: Vec<Done> = done.try_iter().collect();
        done.sort_by_key(|item| item.id);
        (unreachable.load(Ordering::Relaxed), ready, done)
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("../../fixtures")
            .join(name)
    }

    fn patches(prepared: &audio_embedding::Prepared) -> Vec<(usize, usize, u64, Vec<f32>)> {
        (0..prepared.patch_count())
            .map(|ix| {
                let patch = prepared.patch(ix);
                (
                    patch.first_frame,
                    patch.frames,
                    patch.start,
                    patch.samples.to_vec(),
                )
            })
            .collect()
    }

    #[test]
    fn a_ranged_server_track_reads_part_of_the_file_and_matches_a_local_read() {
        let path = fixture("long_44_stereo.flac");
        let data = std::fs::read(&path).expect("run `make generate`");
        let len = data.len();
        let server = Server::with_ranges(true, Bytes::of(data));
        let rig = rig(vec![(1, server.clone())]);
        let file = file(1, "song", &[1]);
        let local = prepare(&path, &file.tracks[0]).unwrap();

        let (unreachable, ready, done) = run_all(&rig, vec![file], (2, 2), 2);

        assert_eq!((unreachable, done.len(), ready.len()), (0, 0, 1));
        assert!(server.fetched.lock().unwrap().is_empty());
        assert_eq!(ready[0].prepared.rate(), 44_100);
        assert!(patches(&ready[0].prepared) == patches(&local));
        let served = server
            .ranges
            .as_ref()
            .unwrap()
            .served
            .load(Ordering::Relaxed);
        assert!(served < len, "{served} of {len} bytes");
    }

    #[test]
    fn a_server_that_ignores_ranges_gets_whole_downloads_within_their_limit() {
        let mut bytes = Bytes::of(vec![0; 100_000]);
        bytes.ranged = false;
        let server = Server::with_ranges(true, bytes);
        let rig = rig(vec![(1, server.clone())]);
        let files = ["a", "b", "c", "d", "e"]
            .iter()
            .zip(1..)
            .map(|(key, id)| file(1, key, &[id]))
            .collect();

        let (unreachable, _, done) = run_all(&rig, files, (2, 4), 8);

        assert_eq!(unreachable, 0);
        let mut fetched = server.fetched.lock().unwrap().clone();
        fetched.sort();
        assert_eq!(fetched, vec!["a", "b", "c", "d", "e"]);
        assert!(server.peak.load(Ordering::SeqCst) <= 2);
        assert_eq!(done.len(), 5);
        let decode = |item: &Done| matches!(item.result, Err(EmbedError::Decode(_)));
        assert!(done.iter().all(decode));
    }

    #[test]
    fn lossy_server_files_are_downloaded_whole() {
        let server = Server::with_ranges(true, Bytes::of(vec![0; 100_000]));
        let rig = rig(vec![(1, server.clone())]);

        let (_, done) = run(&rig, vec![file_of(1, "song", "mp3", &[1])], 2, 2);

        assert_eq!(*server.fetched.lock().unwrap(), vec!["song"]);
        let served = server
            .ranges
            .as_ref()
            .unwrap()
            .served
            .load(Ordering::Relaxed);
        assert_eq!(served, 0);
        assert_eq!(done.len(), 1);
    }

    #[rstest::rstest]
    #[case::server_answers(true, 0, true)]
    #[case::server_down(false, 2, false)]
    fn a_ranged_read_that_loses_the_server_is_a_network_failure(
        #[case] up: bool,
        #[case] waiting: usize,
        #[case] failed: bool,
    ) {
        let data = std::fs::read(fixture("long_44_stereo.flac")).expect("run `make generate`");
        let mut bytes = Bytes::of(data);
        bytes.gone_after_first = true;
        let rig = rig(vec![(1, Server::with_ranges(up, bytes))]);

        let (unreachable, done) = run(&rig, vec![file(1, "image", &[1, 2])], 1, 1);

        assert_eq!(unreachable, waiting);
        let network: Vec<bool> = done
            .iter()
            .map(|item| matches!(item.result, Err(EmbedError::Network(_))))
            .collect();
        assert_eq!(network, if failed { vec![true, true] } else { vec![] });
    }

    #[test]
    fn server_files_are_downloaded_once_and_an_unreachable_server_waits() {
        let (down, up) = (Server::new(false, ""), Server::new(true, "gone"));
        let rig = rig(vec![(1, down.clone()), (2, up.clone())]);

        let (unreachable, done) = run(
            &rig,
            vec![
                file(1, "image", &[10, 11]),
                file(2, "song", &[20]),
                file(1, "other", &[12]),
                file(2, "gone", &[21]),
            ],
            1,
            1,
        );

        assert_eq!(*down.fetched.lock().unwrap(), vec!["image"]);
        assert_eq!(*up.fetched.lock().unwrap(), vec!["song", "gone"]);
        assert_eq!(unreachable, 3);
        let failures: Vec<(i64, bool)> = done
            .iter()
            .map(|item| (item.id, matches!(item.result, Err(EmbedError::Network(_)))))
            .collect();
        assert_eq!(failures, vec![(20, false), (21, true)]);
        assert_eq!(std::fs::read_dir(rig.scratch.path()).unwrap().count(), 0);
    }

    #[test]
    fn every_source_downloads_in_parallel_and_leaves_no_files_behind() {
        let (a, b) = (Server::new(true, ""), Server::new(true, ""));
        let rig = rig(vec![(1, a.clone()), (2, b.clone())]);
        let files = (0..3)
            .flat_map(|n| {
                [
                    file(1, &format!("a{n}"), &[n]),
                    file(2, &format!("b{n}"), &[10 + n]),
                ]
            })
            .collect();

        let (unreachable, done) = run(&rig, files, PER_SOURCE, MAX_DOWNLOADS);

        assert_eq!(unreachable, 0);
        assert_eq!(a.fetched.lock().unwrap().len(), 3);
        assert_eq!(b.fetched.lock().unwrap().len(), 3);
        assert_eq!(
            done.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![0, 1, 2, 10, 11, 12]
        );
        assert_eq!(std::fs::read_dir(rig.scratch.path()).unwrap().count(), 0);
    }
}
