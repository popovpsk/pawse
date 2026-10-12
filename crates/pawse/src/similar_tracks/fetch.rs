use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

use audio_embedding::EmbedError;
use music_library::Track;
use music_library::remote::RemoteRef;

use super::worker::{Done, Lanes, Ready, prepare};
use crate::remote_media::RemoteMedia;

pub(super) const PER_SOURCE: usize = 2;
pub(super) const MAX_DOWNLOADS: usize = 8;
const GATE_POLL: Duration = Duration::from_millis(250);

pub(super) struct ServerFile {
    pub(super) reference: RemoteRef,
    pub(super) tracks: Vec<Track>,
}

struct Queue {
    source_id: i64,
    files: VecDeque<ServerFile>,
    active: usize,
    down: bool,
}

struct Board {
    queues: Vec<Queue>,
    per_source: usize,
}

impl Board {
    fn new(files: Vec<ServerFile>, last: impl Fn(i64) -> bool, per_source: usize) -> Self {
        let mut queues: Vec<Queue> = Vec::new();
        for file in files {
            let source_id = file.reference.source_id;
            match queues.iter_mut().find(|queue| queue.source_id == source_id) {
                Some(queue) => queue.files.push_back(file),
                None => queues.push(Queue {
                    source_id,
                    files: VecDeque::from([file]),
                    active: 0,
                    down: false,
                }),
            }
        }
        queues.sort_by_key(|queue| last(queue.source_id));
        Self { queues, per_source }
    }

    fn workers(&self, max: usize) -> usize {
        self.queues
            .iter()
            .map(|queue| queue.files.len().min(self.per_source))
            .sum::<usize>()
            .min(max)
    }

    fn take(&mut self) -> Option<(usize, ServerFile)> {
        let per_source = self.per_source;
        let (ix, queue) = self.queues.iter_mut().enumerate().find(|(_, queue)| {
            !queue.down && queue.active < per_source && !queue.files.is_empty()
        })?;
        let file = queue.files.pop_front()?;
        queue.active += 1;
        Some((ix, file))
    }

    fn finish(&mut self, ix: usize) {
        let queue = &mut self.queues[ix];
        queue.active = queue.active.saturating_sub(1);
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
    pub(super) per_source: usize,
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
        let board = Board::new(files, |id| self.media.is_torrent(id), self.per_source);
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
            let Some((ix, file)) = lock(board).take() else {
                return;
            };
            let fetch = || self.fetch(file, gate, ready, done);
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
            board.finish(ix);
            if matches!(outcome, Outcome::Stop) {
                return;
            }
        }
    }

    fn fetch(
        &self,
        file: ServerFile,
        gate: &Gate,
        ready: &flume::Sender<Ready>,
        done: &flume::Sender<Done>,
    ) -> Outcome {
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
            Err(e) if self.answers(source_id) => {
                for track in &file.tracks {
                    let failure = Done {
                        id: track.id,
                        path: PathBuf::from(&track.path),
                        result: Err(EmbedError::Network(e.clone())),
                    };
                    if done.send(failure).is_err() {
                        return Outcome::Stop;
                    }
                }
                return Outcome::Done;
            }
            Err(e) => {
                log::warn!(
                    "similar tracks: source {source_id} is unreachable, its tracks wait for the \
                     next pass: {e}"
                );
                return Outcome::Unreachable(file.tracks.len());
            }
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
        let locator = music_library::remote::locator(source_id, key, "flac");
        ServerFile {
            reference: music_library::remote::parse(&locator).unwrap(),
            tracks: ids
                .iter()
                .map(|&id| server_track(id, &locator, ids.len() > 1))
                .collect(),
        }
    }

    fn keys(board: &mut Board) -> Vec<(usize, String)> {
        std::iter::from_fn(|| board.take())
            .map(|(ix, file)| (ix, file.reference.key))
            .collect()
    }

    #[test]
    fn each_source_gets_its_own_workers_and_torrents_come_last() {
        let mut board = Board::new(
            vec![
                file(7, "t1", &[1]),
                file(1, "a1", &[2]),
                file(1, "a2", &[3]),
                file(1, "a3", &[4]),
                file(2, "b1", &[5]),
            ],
            |id| id == 7,
            2,
        );
        assert_eq!(board.workers(8), 4);
        assert_eq!(board.workers(3), 3);
        assert_eq!(
            keys(&mut board),
            vec![
                (0, "a1".into()),
                (0, "a2".into()),
                (1, "b1".into()),
                (2, "t1".into()),
            ]
        );
        board.finish(0);
        assert_eq!(keys(&mut board), vec![(0, "a3".into())]);
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
        );
        let (ix, _) = board.take().unwrap();
        assert_eq!(board.give_up(ix), 3);
        board.finish(ix);
        assert!(board.take().is_none());
    }

    struct Server {
        up: bool,
        refuses: &'static str,
        fetched: Mutex<Vec<String>>,
    }

    impl Server {
        fn new(up: bool, refuses: &'static str) -> Arc<Self> {
            Arc::new(Self {
                up,
                refuses,
                fetched: Mutex::new(Vec::new()),
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
            per_source,
            max_downloads: max,
            prepare_slots: 1,
        };
        let (ready_tx, ready) = flume::unbounded();
        let (done_tx, done) = flume::unbounded();
        fetcher.run(files, &ready_tx, &done_tx);
        assert!(ready.try_recv().is_err());
        let mut done: Vec<Done> = done.try_iter().collect();
        done.sort_by_key(|item| item.id);
        (unreachable.load(Ordering::Relaxed), done)
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
