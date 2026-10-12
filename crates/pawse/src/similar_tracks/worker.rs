use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use audio_decoder::Decoder;
use audio_embedding::{EMBEDDING_VERSION, EmbedError, Embedding, Job, Prepared, TrackRange};
use audio_embedding_gpu::{Availability, Device, Submitted};
use music_library::Track;
use music_library::remote::RemoteRef;

use super::fetch::{Fetcher, MAX_DOWNLOADS, PER_SOURCE, PER_SOURCE_RANGED, ServerFile, Stopped};
use super::{Phase, Shared};
use crate::remote_media::RemoteMedia;

const SAVE_BATCH: usize = 16;
const PROGRESS_EVERY: usize = 100;
const LOAD_RETRY: Duration = Duration::from_secs(5 * 60);
const GPU_RETRY: Duration = Duration::from_secs(30);
const MAX_PREPARE_THREADS: usize = 8;
const READY: usize = 4;
const IN_FLIGHT: usize = 2;
const FILL_WAIT: Duration = Duration::from_millis(300);

pub(super) fn spawn(
    shared: Arc<Shared>,
    remote_media: RemoteMedia,
    model_dir: PathBuf,
    scratch_dir: PathBuf,
    inbox: flume::Receiver<()>,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("similar-tracks".into())
        .spawn(move || {
            clear_leftovers(&scratch_dir);
            let mut worker = Worker {
                shared,
                remote_media,
                model_dir,
                scratch_dir,
                model: Model::Unloaded,
                failed: HashSet::new(),
            };
            loop {
                let next = match worker.model {
                    Model::Missing => inbox.recv_timeout(LOAD_RETRY),
                    Model::Faulted => inbox.recv_timeout(GPU_RETRY),
                    Model::Unloaded | Model::Unavailable(_) => inbox
                        .recv()
                        .map_err(|_| flume::RecvTimeoutError::Disconnected),
                };
                if matches!(next, Err(flume::RecvTimeoutError::Disconnected)) || worker.stopped() {
                    break;
                }
                worker.run_pass(&inbox);
            }
            let _ = std::fs::remove_dir_all(&worker.scratch_dir);
        })
        .map(drop)
}

struct Worker {
    shared: Arc<Shared>,
    remote_media: RemoteMedia,
    model_dir: PathBuf,
    scratch_dir: PathBuf,
    model: Model,
    failed: HashSet<i64>,
}

#[derive(Debug, PartialEq)]
enum Model {
    Unloaded,
    Missing,
    Faulted,
    Unavailable(Phase),
}

enum Ended {
    Finished,
    Faulted { analysed: usize },
}

struct Work {
    todo: Vec<(Track, PathBuf)>,
    groups: Vec<ServerFile>,
}

impl Worker {
    fn stopped(&self) -> bool {
        self.shared.stop.load(Ordering::Relaxed)
    }

    fn run_pass(&mut self, inbox: &flume::Receiver<()>) {
        self.shared.neighbors.invalidate();
        let shared = self.shared.clone();
        let progress = &shared.progress;
        if let Model::Unavailable(phase) = self.model {
            progress.set_phase(phase);
            return;
        }
        let work = match self.work(inbox) {
            Ok(Some(work)) => work,
            Ok(None) => {
                if matches!(self.model, Model::Missing | Model::Faulted) {
                    self.model = Model::Unloaded;
                }
                progress.set_phase(Phase::Idle);
                return;
            }
            Err(e) => {
                log::error!("similar tracks: analysis stopped: {e:#}");
                progress.set_phase(Phase::Stopped);
                return;
            }
        };
        progress.set_phase(Phase::LoadingModel);
        let device = match self.load() {
            Ok(device) => device,
            Err(model) => {
                let phase = match model {
                    Model::Unavailable(phase) => phase,
                    _ => Phase::NoModel,
                };
                self.model = model;
                progress.set_phase(phase);
                return;
            }
        };
        self.model = Model::Unloaded;
        progress.set_phase(Phase::Analysing);
        let (model, phase) = after_analysis(self.analyse(&device, work, inbox));
        drop(device);
        self.model = model;
        progress.set_phase(phase);
    }

    fn load(&self) -> Result<Box<Device>, Model> {
        let started = Instant::now();
        if audio_embedding_gpu::adapter_name().is_none() {
            log::warn!("similar tracks: no hardware graphics adapter, the analysis is unavailable");
            return Err(Model::Unavailable(Phase::NoGpu));
        }
        let path = match audio_embedding::model_file::ensure(&self.model_dir, &|| self.stopped()) {
            Ok(path) => path,
            Err(EmbedError::Cancelled) => return Err(Model::Missing),
            Err(e) => {
                log::error!("similar tracks: the model is unavailable: {e}");
                return Err(Model::Missing);
            }
        };
        if self.stopped() {
            return Err(Model::Missing);
        }
        let device = match audio_embedding_gpu::open(&path) {
            Ok(Availability::Ready(device)) => device,
            Ok(Availability::NoGpu(reason)) => {
                log::warn!("similar tracks: the analysis is unavailable: {reason}");
                return Err(Model::Unavailable(Phase::NoGpu));
            }
            Ok(Availability::Failed(reason)) => {
                log::error!("similar tracks: no graphics adapter passed the check: {reason}");
                return Err(Model::Unavailable(Phase::GpuFailed));
            }
            Err(e) => {
                log::error!("similar tracks: the model does not load: {e}");
                match audio_embedding::model_file::discard_if_corrupt(&self.model_dir) {
                    Ok(true) => {
                        log::warn!("similar tracks: the model file was corrupt and is removed")
                    }
                    Ok(false) => {}
                    Err(e) => log::warn!("similar tracks: could not check the model file: {e}"),
                }
                return Err(Model::Missing);
            }
        };
        log::info!(
            "similar tracks: model ready on {} in {} ms (agreement {:.7})",
            device.name(),
            started.elapsed().as_millis(),
            device.agreement()
        );
        match self.shared.repo.prune_embeddings(EMBEDDING_VERSION) {
            Ok(0) => {}
            Ok(dropped) => {
                log::info!("similar tracks: dropped {dropped} vectors of older versions");
                self.shared.neighbors.invalidate();
            }
            Err(e) => {
                log::error!("similar tracks: could not drop old vectors: {e}");
                return Err(Model::Missing);
            }
        }
        Ok(device)
    }

    fn work(&self, inbox: &flume::Receiver<()>) -> anyhow::Result<Option<Work>> {
        let shared = &self.shared;
        let failed_ids = &self.failed;
        while inbox.try_recv().is_ok() {}
        let candidates = shared.repo.embedding_candidates(EMBEDDING_VERSION)?;
        let known_failed = candidates
            .iter()
            .filter(|track| failed_ids.contains(&track.id))
            .count();
        shared
            .progress
            .failed
            .store(known_failed, Ordering::Relaxed);
        shared.progress.unreachable.store(0, Ordering::Relaxed);
        let (todo, groups) = split(
            candidates
                .into_iter()
                .filter(|track| !failed_ids.contains(&track.id)),
            &self.remote_media,
        );
        Ok((!todo.is_empty() || !groups.is_empty()).then_some(Work { todo, groups }))
    }

    fn analyse(
        &mut self,
        device: &Device,
        work: Work,
        inbox: &flume::Receiver<()>,
    ) -> anyhow::Result<Ended> {
        let Work { todo, groups } = work;
        let shared = &self.shared;
        let failed_ids = &mut self.failed;
        let media = &self.remote_media;
        let scratch = self.scratch_dir.as_path();
        let superseded = || !inbox.is_empty();
        let from_servers: usize = groups.iter().map(|file| file.tracks.len()).sum();
        let total = todo.len() + from_servers;
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        let preparing = prepare_threads(cores);
        log::info!(
            "similar tracks: {total} tracks to analyse, {from_servers} of them downloaded from \
             servers, {preparing} preparing"
        );
        let started = Instant::now();
        let halt = Arc::new(AtomicBool::new(false));
        let stopped: Stopped = {
            let (shared, halt) = (self.shared.clone(), halt.clone());
            Arc::new(move || shared.stop.load(Ordering::Relaxed) || halt.load(Ordering::Relaxed))
        };
        let (jobs_tx, jobs) = flume::unbounded();
        for job in todo {
            let _ = jobs_tx.send(job);
        }
        drop(jobs_tx);
        let (ready_tx, ready) = flume::bounded(READY);
        let (done_tx, done) = flume::unbounded();
        std::thread::scope(|scope| {
            let lanes = Lanes {
                stop: &shared.stop,
                halt: &halt,
            };
            let spawned = (|| -> std::io::Result<()> {
                let mut preparers = Vec::with_capacity(preparing);
                for n in 0..preparing {
                    let (prepare_jobs, prepare_ready, prepare_done) =
                        (jobs.clone(), ready_tx.clone(), done_tx.clone());
                    preparers.push(
                        std::thread::Builder::new()
                            .name(format!("similar-prepare-{n}"))
                            .spawn_scoped(scope, move || {
                                lanes.prepare(&prepare_jobs, &prepare_ready, &prepare_done)
                            })?,
                    );
                }
                let (model_ready, model_done) = (ready.clone(), done_tx.clone());
                std::thread::Builder::new()
                    .name("similar-gpu".into())
                    .spawn_scoped(scope, move || {
                        lanes.drive(device, &model_ready, &model_done)
                    })?;
                if groups.is_empty() {
                    return Ok(());
                }
                let (fetch_ready, fetch_done) = (ready_tx.clone(), done_tx.clone());
                let fetcher = Fetcher {
                    lanes,
                    media,
                    scratch,
                    unreachable: &shared.progress.unreachable,
                    superseded: &superseded,
                    stopped: stopped.clone(),
                    per_source: PER_SOURCE,
                    per_source_ranged: PER_SOURCE_RANGED,
                    max_downloads: MAX_DOWNLOADS,
                    prepare_slots: preparing,
                };
                std::thread::Builder::new()
                    .name("similar-fetch".into())
                    .spawn_scoped(scope, move || {
                        for preparer in preparers {
                            let _ = preparer.join();
                        }
                        fetcher.run(groups, &fetch_ready, &fetch_done);
                    })?;
                Ok(())
            })();
            drop((jobs, ready_tx, ready, done_tx));
            if let Err(e) = spawned {
                halt.store(true, Ordering::Relaxed);
                return Err(anyhow::anyhow!("could not start the analysis threads: {e}"));
            }
            let collected = collect(shared, failed_ids, &done, total, started, &halt);
            halt.store(true, Ordering::Relaxed);
            collected
        })
    }
}

fn after_analysis(ended: anyhow::Result<Ended>) -> (Model, Phase) {
    match ended {
        Ok(Ended::Finished) => (Model::Unloaded, Phase::Idle),
        Ok(Ended::Faulted { analysed: 0 }) => {
            log::error!(
                "similar tracks: the graphics card failed before any track was analysed, the \
                 analysis is unavailable until the app restarts"
            );
            (Model::Unavailable(Phase::GpuFailed), Phase::GpuFailed)
        }
        Ok(Ended::Faulted { .. }) => {
            log::warn!(
                "similar tracks: trying the graphics card again in {} s",
                GPU_RETRY.as_secs()
            );
            (Model::Faulted, Phase::GpuRetry)
        }
        Err(e) => {
            log::error!("similar tracks: analysis stopped: {e:#}");
            (Model::Unloaded, Phase::Stopped)
        }
    }
}

fn clear_leftovers(scratch: &Path) {
    let own = format!("{}-", std::process::id());
    let Some(Ok(entries)) = scratch.parent().map(std::fs::read_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&own) {
            continue;
        }
        let path = entry.path();
        let _ = match path.is_dir() {
            true => std::fs::remove_dir_all(&path),
            false => std::fs::remove_file(&path),
        };
    }
}

fn prepare_threads(cores: usize) -> usize {
    (cores / 2).clamp(1, MAX_PREPARE_THREADS)
}

pub(super) struct Ready {
    pub(super) id: i64,
    pub(super) path: PathBuf,
    pub(super) prepared: Prepared,
}

pub(super) struct Done {
    pub(super) id: i64,
    pub(super) path: PathBuf,
    pub(super) result: Result<Vec<f32>, EmbedError>,
}

#[derive(Clone, Copy)]
pub(super) struct Lanes<'a> {
    pub(super) stop: &'a AtomicBool,
    pub(super) halt: &'a AtomicBool,
}

impl Lanes<'_> {
    pub(super) fn halted(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || self.halt.load(Ordering::Relaxed)
    }

    fn prepare(
        &self,
        jobs: &flume::Receiver<(Track, PathBuf)>,
        ready: &flume::Sender<Ready>,
        done: &flume::Sender<Done>,
    ) {
        while !self.halted() {
            let Ok((track, path)) = jobs.try_recv() else {
                return;
            };
            let sent = match prepare(&path, &track) {
                Ok(prepared) => ready
                    .send(Ready {
                        id: track.id,
                        path,
                        prepared,
                    })
                    .is_ok(),
                Err(e) => done
                    .send(Done {
                        id: track.id,
                        path,
                        result: Err(e),
                    })
                    .is_ok(),
            };
            if !sent {
                return;
            }
        }
    }

    fn drive(&self, device: &Device, ready: &flume::Receiver<Ready>, done: &flume::Sender<Done>) {
        let capacity = device.patches_per_run();
        let mut flight: VecDeque<InFlight> = VecDeque::new();
        let mut carry: Option<Ready> = None;
        loop {
            if self.halted() {
                break;
            }
            let first = match carry.take() {
                Some(item) => Some(item),
                None if flight.is_empty() => ready.recv().ok(),
                None => ready.try_recv().ok(),
            };
            let Some(first) = first else {
                match flight.pop_front() {
                    Some(oldest) => {
                        if !finish(oldest, done) {
                            return;
                        }
                        continue;
                    }
                    None => break,
                }
            };
            let (batch, next) = gather(
                first,
                ready,
                capacity,
                |item: &Ready| item.prepared.patch_count(),
                FILL_WAIT,
            );
            carry = next;
            if self.halted() {
                break;
            }
            flight.push_back(submit(device, batch));
            if flight.len() > IN_FLIGHT
                && let Some(oldest) = flight.pop_front()
                && !finish(oldest, done)
            {
                return;
            }
        }
        for item in flight {
            if !finish(item, done) {
                return;
            }
        }
    }
}

struct InFlight<'a> {
    tracks: Vec<(i64, PathBuf)>,
    submitted: Submission<'a>,
}

enum Submission<'a> {
    Pending(Submitted<'a>),
    Done(Vec<Result<Embedding, EmbedError>>),
}

fn submit(device: &Device, batch: Vec<Ready>) -> InFlight<'_> {
    let (tracks, prepared): (Vec<(i64, PathBuf)>, Vec<Prepared>) = batch
        .into_iter()
        .map(|item| ((item.id, item.path), item.prepared))
        .unzip();
    let submitted = match catch_unwind(AssertUnwindSafe(|| device.submit(&prepared))) {
        Ok(submitted) => Submission::Pending(submitted),
        Err(_) => Submission::Done(prepared.iter().map(|one| one_by_one(device, one)).collect()),
    };
    InFlight { tracks, submitted }
}

fn one_by_one(device: &Device, prepared: &Prepared) -> Result<Embedding, EmbedError> {
    catch_unwind(AssertUnwindSafe(|| {
        device.submit(std::slice::from_ref(prepared))
    }))
    .map_err(|_| EmbedError::Decode("the analysis panicked".into()))
    .and_then(|submitted| {
        catch_unwind(AssertUnwindSafe(|| submitted.wait()))
            .map_err(|_| EmbedError::Model("the GPU run panicked".into()))
    })
    .and_then(|mut results| {
        results
            .pop()
            .unwrap_or_else(|| Err(EmbedError::Model("no result".into())))
    })
}

fn finish(item: InFlight<'_>, done: &flume::Sender<Done>) -> bool {
    let results = match item.submitted {
        Submission::Pending(submitted) => catch_unwind(AssertUnwindSafe(|| submitted.wait()))
            .unwrap_or_else(|_| {
                item.tracks
                    .iter()
                    .map(|_| Err(EmbedError::Model("the GPU run panicked".into())))
                    .collect()
            }),
        Submission::Done(results) => results,
    };
    for ((id, path), result) in item.tracks.into_iter().zip(results) {
        let result = result.map(|embedding| embedding.vector.into_vec());
        if done.send(Done { id, path, result }).is_err() {
            return false;
        }
    }
    true
}

fn gather<T>(
    first: T,
    ready: &flume::Receiver<T>,
    capacity: usize,
    size: impl Fn(&T) -> usize,
    wait: Duration,
) -> (Vec<T>, Option<T>) {
    let deadline = Instant::now() + wait;
    let mut filled = size(&first);
    let mut batch = vec![first];
    while filled < capacity {
        let Ok(next) = ready.recv_deadline(deadline) else {
            break;
        };
        let len = size(&next);
        if filled + len > capacity {
            return (batch, Some(next));
        }
        filled += len;
        batch.push(next);
    }
    (batch, None)
}

fn collect(
    shared: &Shared,
    failed_ids: &mut HashSet<i64>,
    done: &flume::Receiver<Done>,
    total: usize,
    started: Instant,
    halt: &AtomicBool,
) -> anyhow::Result<Ended> {
    let mut pending = Vec::with_capacity(SAVE_BATCH);
    let (mut analysed, mut failed) = (0usize, 0usize);
    let mut model_error = None;
    for item in done.iter() {
        match item.result {
            Ok(vector) => {
                pending.push((item.id, vector));
                analysed += 1;
            }
            Err(EmbedError::Model(e)) => {
                halt.store(true, Ordering::Relaxed);
                model_error.get_or_insert(e);
            }
            Err(e) => {
                log::warn!("similar tracks: {}: {e}", item.path.display());
                failed_ids.insert(item.id);
                shared.progress.failed.fetch_add(1, Ordering::Relaxed);
                failed += 1;
            }
        }
        if pending.len() >= SAVE_BATCH {
            save(shared, &mut pending)?;
        }
        let attempted = analysed + failed;
        if attempted > 0 && attempted % PROGRESS_EVERY == 0 {
            log::info!(
                "similar tracks: {attempted} of {total} done in {} s",
                started.elapsed().as_secs()
            );
        }
    }
    save(shared, &mut pending)?;
    if let Some(e) = model_error {
        log::error!(
            "similar tracks: the graphics card failed after {analysed} tracks in {} s: {e}",
            started.elapsed().as_secs()
        );
        return Ok(Ended::Faulted { analysed });
    }
    log::info!(
        "similar tracks: pass finished in {} s: {analysed} analysed, {failed} failed, {} waiting \
         for their server",
        started.elapsed().as_secs(),
        shared.progress.unreachable.load(Ordering::Relaxed)
    );
    Ok(Ended::Finished)
}

fn save(shared: &Shared, pending: &mut Vec<(i64, Vec<f32>)>) -> anyhow::Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    shared.repo.save_embeddings(EMBEDDING_VERSION, pending)?;
    shared.neighbors.invalidate();
    pending.clear();
    Ok(())
}

#[derive(Debug, PartialEq)]
enum Source {
    File(PathBuf),
    Server(RemoteRef),
}

fn source_of(track: &Track, remote_media: &RemoteMedia) -> Option<Source> {
    if let Some(path) = track.local_file() {
        return Some(Source::File(path.to_path_buf()));
    }
    let reference = track.remote()?;
    Some(match remote_media.peek_cached(&reference) {
        Some(path) => Source::File(path),
        None => Source::Server(reference),
    })
}

fn split(
    candidates: impl IntoIterator<Item = Track>,
    media: &RemoteMedia,
) -> (Vec<(Track, PathBuf)>, Vec<ServerFile>) {
    let mut files = Vec::new();
    let mut servers: Vec<ServerFile> = Vec::new();
    let mut server_of: HashMap<String, usize> = HashMap::new();
    for track in candidates {
        match source_of(&track, media) {
            Some(Source::File(path)) => files.push((track, path)),
            Some(Source::Server(reference)) => {
                let ix = *server_of.entry(track.path.clone()).or_insert_with(|| {
                    servers.push(ServerFile {
                        reference,
                        tracks: Vec::new(),
                    });
                    servers.len() - 1
                });
                servers[ix].tracks.push(track);
            }
            None => {}
        }
    }
    (files, servers)
}

fn range_of(track: &Track) -> Option<TrackRange> {
    track.is_cue.then(|| TrackRange {
        start: Duration::from_millis(track.start_offset_ms.max(0) as u64),
        length: track
            .duration_ms
            .filter(|ms| *ms > 0)
            .map(|ms| Duration::from_millis(ms as u64)),
    })
}

pub(super) fn prepare(path: &Path, track: &Track) -> Result<Prepared, EmbedError> {
    prepare_with(|| Decoder::open(path), track)
}

pub(super) fn prepare_with<E: std::fmt::Display>(
    open: impl FnOnce() -> Result<Decoder, E>,
    track: &Track,
) -> Result<Prepared, EmbedError> {
    let run = || {
        let decoder = open().map_err(|e| EmbedError::Decode(e.to_string()))?;
        audio_embedding::prepare(Job {
            source: Box::new(decoder),
            range: range_of(track),
        })
    };
    catch_unwind(AssertUnwindSafe(run))
        .unwrap_or_else(|_| Err(EmbedError::Decode("the analysis panicked".into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(path: &str, is_cue: bool, start: i32, duration: Option<i64>) -> Track {
        Track {
            id: 1,
            path: path.into(),
            title: "t".into(),
            album_id: None,
            track_number: None,
            disc_number: 1,
            duration_ms: duration,
            year: None,
            cover_art_id: None,
            start_offset_ms: start,
            liked: false,
            bitrate: None,
            is_cue,
            available: true,
        }
    }

    #[rstest::rstest]
    #[case::finished(Ok(Ended::Finished), Model::Unloaded, Phase::Idle)]
    #[case::gpu_fault_after_progress(
        Ok(Ended::Faulted { analysed: 3 }),
        Model::Faulted,
        Phase::GpuRetry
    )]
    #[case::gpu_fault_without_progress(
        Ok(Ended::Faulted { analysed: 0 }),
        Model::Unavailable(Phase::GpuFailed),
        Phase::GpuFailed
    )]
    #[case::database_error(Err(anyhow::anyhow!("disk full")), Model::Unloaded, Phase::Stopped)]
    fn a_gpu_fault_is_retried_only_after_progress(
        #[case] ended: anyhow::Result<Ended>,
        #[case] model: Model,
        #[case] phase: Phase,
    ) {
        assert_eq!(after_analysis(ended), (model, phase));
    }

    #[rstest::rstest]
    #[case::one_core(1, 1)]
    #[case::four_cores(4, 2)]
    #[case::m1(8, 4)]
    #[case::m3_max(14, 7)]
    #[case::workstation(32, 8)]
    fn decoding_takes_half_the_cores(#[case] cores: usize, #[case] preparing: usize) {
        assert_eq!(prepare_threads(cores), preparing);
    }

    #[test]
    fn a_batch_takes_what_is_ready_up_to_the_capacity() {
        let (tx, rx) = flume::unbounded();
        for size in [16, 16, 8] {
            tx.send(size).unwrap();
        }
        let (batch, carry) = gather(32, &rx, 64, |s| *s, Duration::ZERO);
        assert_eq!(batch, vec![32, 16, 16]);
        assert_eq!(carry, None);
        assert_eq!(rx.try_recv(), Ok(8));
    }

    #[test]
    fn a_track_that_does_not_fit_waits_for_the_next_batch() {
        let (tx, rx) = flume::unbounded();
        tx.send(16).unwrap();
        tx.send(32).unwrap();
        let (batch, carry) = gather(32, &rx, 64, |s| *s, Duration::ZERO);
        assert_eq!(batch, vec![32, 16]);
        assert_eq!(carry, Some(32));
    }

    #[test]
    fn a_half_batch_runs_after_a_short_wait_or_at_the_end() {
        let (tx, rx) = flume::unbounded::<usize>();
        let started = Instant::now();
        let (batch, carry) = gather(32, &rx, 64, |s| *s, Duration::from_millis(20));
        assert_eq!((batch, carry), (vec![32], None));
        assert!(started.elapsed() >= Duration::from_millis(20));

        drop(tx);
        let started = Instant::now();
        let (batch, _) = gather(32, &rx, 64, |s| *s, Duration::from_secs(10));
        assert_eq!(batch, vec![32]);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn the_wait_for_more_tracks_is_one_deadline_for_the_whole_batch() {
        let (tx, rx) = flume::unbounded();
        let feeder = std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(15));
                if tx.send(1).is_err() {
                    return;
                }
            }
        });
        let started = Instant::now();
        let (batch, _) = gather(32, &rx, 64, |s| *s, Duration::from_millis(60));
        assert!(started.elapsed() < Duration::from_millis(200));
        assert!(batch.len() < 21);
        drop(rx);
        feeder.join().unwrap();
    }

    #[test]
    fn a_cue_track_is_analysed_over_its_own_range() {
        assert_eq!(
            range_of(&track("/m/image.flac", true, 61_500, Some(200_000))),
            Some(TrackRange {
                start: Duration::from_millis(61_500),
                length: Some(Duration::from_millis(200_000)),
            })
        );
        assert_eq!(
            range_of(&track("/m/image.flac", true, 0, None)),
            Some(TrackRange {
                start: Duration::ZERO,
                length: None,
            })
        );
        assert_eq!(range_of(&track("/m/song.flac", false, 0, Some(1))), None);
    }

    #[test]
    fn tracks_of_one_server_file_are_grouped_and_files_go_to_the_prepare_queue() {
        let dir = tempfile::tempdir().unwrap();
        let media =
            RemoteMedia::with_cache(crate::remote_media::CacheStore::new(dir.path().into()));
        let image = music_library::remote::locator(4, "image", "flac");
        let song = music_library::remote::locator(4, "song", "flac");
        let with_id = |id, path: &str| Track {
            id,
            ..track(path, false, 0, None)
        };
        let (files, servers) = split(
            [
                with_id(2, &image),
                with_id(1, "/m/local.flac"),
                with_id(4, &song),
                with_id(3, &image),
            ],
            &media,
        );
        assert_eq!(files.iter().map(|(t, _)| t.id).collect::<Vec<_>>(), vec![1]);
        let grouped: Vec<Vec<i64>> = servers
            .iter()
            .map(|file| file.tracks.iter().map(|t| t.id).collect())
            .collect();
        assert_eq!(grouped, vec![vec![2, 3], vec![4]]);
    }

    #[test]
    fn leftovers_of_earlier_runs_go_and_this_process_folders_stay() {
        let root = tempfile::tempdir().unwrap();
        let own = format!("{}-", std::process::id());
        for name in [
            format!("{own}0"),
            format!("{own}1"),
            "1-0".into(),
            "9.flac".into(),
        ] {
            let path = root.path().join(&name);
            match name.ends_with(".flac") {
                true => std::fs::write(&path, b"x").unwrap(),
                false => std::fs::create_dir(&path).unwrap(),
            }
        }
        clear_leftovers(&root.path().join(format!("{own}2")));
        let mut left: Vec<String> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, vec![format!("{own}0"), format!("{own}1")]);
    }

    #[test]
    fn a_server_track_is_read_from_the_cache_or_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let media =
            RemoteMedia::with_cache(crate::remote_media::CacheStore::new(dir.path().into()));
        let remote = track(
            &music_library::remote::locator(4, "song-1", "flac"),
            false,
            0,
            None,
        );
        assert_eq!(
            source_of(&remote, &media),
            Some(Source::Server(remote.remote().unwrap()))
        );

        let cached = crate::remote_media::CacheStore::new(dir.path().into())
            .path_for(&remote.remote().unwrap());
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, b"x").unwrap();
        assert_eq!(source_of(&remote, &media), Some(Source::File(cached)));

        let local = track("/m/song.flac", false, 0, None);
        assert_eq!(
            source_of(&local, &media),
            Some(Source::File(PathBuf::from("/m/song.flac")))
        );
    }
}
