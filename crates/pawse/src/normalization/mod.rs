use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use audio_common::{AudioSource, ReplayGain};
use audio_decoder::Decoder;
use loudness::{Estimate, Span, Switch};
use music_library::remote::{self, Location};
use music_library::{LibraryRepository, StoredLoudness};

use crate::playback_opener::TrackRequest;
use crate::remote_media::RemoteMedia;

mod playback;

pub use playback::{Tracker, set_enabled};

const VERSION: u32 = 1;
const CLICK_BUDGET: Duration = Duration::from_millis(100);
const NEXT_BUDGET: Duration = Duration::from_secs(10);
const SLOW_TO_SAMPLE: [&str; 3] = ["ape", "dsf", "dff"];
const SHUTDOWN_WAIT: Duration = Duration::from_secs(1);

#[derive(Clone, Copy)]
struct Value {
    full: bool,
    gain: ReplayGain,
}

pub fn takes_file_tags(is_cue: bool, start_ms: i64) -> bool {
    !is_cue && start_ms <= 0
}

fn span_of(track: &TrackRequest, start_ms: i64) -> Span {
    if takes_file_tags(track.is_cue, start_ms) {
        return Span::default();
    }
    Span {
        start: Duration::from_millis(start_ms.max(0) as u64),
        length: track.duration,
    }
}

enum Job {
    Measure { track: TrackRequest, full: bool },
    Save { item_id: i64, estimate: Estimate },
}

pub type Spawner = Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    draining: bool,
}

#[derive(Clone)]
pub struct Normalization {
    inner: Arc<Inner>,
}

struct Inner {
    switch: Arc<Switch>,
    repo: Arc<dyn LibraryRepository>,
    media: RemoteMedia,
    values: Mutex<HashMap<i64, Value>>,
    tagged: Mutex<HashSet<i64>>,
    queue: Mutex<Queue>,
    drained: Condvar,
    stopped: AtomicBool,
    spawner: Spawner,
}

impl Normalization {
    pub fn new(
        switch: Arc<Switch>,
        repo: Arc<dyn LibraryRepository>,
        media: RemoteMedia,
        spawner: Spawner,
    ) -> Self {
        let inner = Arc::new(Inner {
            switch,
            repo,
            media,
            values: Mutex::default(),
            tagged: Mutex::default(),
            queue: Mutex::default(),
            drained: Condvar::new(),
            stopped: AtomicBool::new(false),
            spawner,
        });
        inner.load();
        Self { inner }
    }

    pub fn switch(&self) -> Arc<Switch> {
        self.inner.switch.clone()
    }

    pub fn is_on(&self) -> bool {
        self.inner.switch.is_on()
    }

    pub fn set_enabled(&self, on: bool) {
        self.inner.switch.set(on);
        if !on {
            self.inner
                .lock_queue()
                .jobs
                .retain(|job| matches!(job, Job::Save { .. }));
        }
    }

    pub fn shutdown(&self) {
        let queue = self.inner.lock_queue();
        self.inner.stopped.store(true, Ordering::Relaxed);
        let (queue, _) = self
            .inner
            .drained
            .wait_timeout_while(queue, SHUTDOWN_WAIT, |queue| queue.draining)
            .unwrap_or_else(|e| e.into_inner());
        if queue.draining {
            log::warn!("loudness: measurements did not stop in time");
        }
    }

    pub fn known(&self, item_id: i64) -> Option<ReplayGain> {
        self.inner.known(item_id)
    }

    pub fn wants_estimate(&self, item_id: i64) -> bool {
        self.is_on() && self.inner.known(item_id).is_none() && !self.inner.is_tagged(item_id)
    }

    pub fn wants_click_estimate(&self, track: &TrackRequest) -> bool {
        self.wants_estimate(track.id) && !slow_to_sample(Path::new(&track.path))
    }

    pub fn estimate_now(
        &self,
        track: &TrackRequest,
        file: &Path,
        start_ms: i64,
    ) -> Option<ReplayGain> {
        self.estimate_within(track, file, start_ms, CLICK_BUDGET)
    }

    fn estimate_within(
        &self,
        track: &TrackRequest,
        file: &Path,
        start_ms: i64,
        budget: Duration,
    ) -> Option<ReplayGain> {
        if !self.is_on() || slow_to_sample(file) {
            return None;
        }
        let deadline = Instant::now() + budget;
        let mut decoder = Decoder::open(file).ok()?;
        if self.inner.leaves_to_tags(track, start_ms, &decoder) {
            return None;
        }
        let estimate = loudness::quick(&mut decoder, span_of(track, start_ms), Some(deadline))?;
        self.inner.remember(track.id, estimate);
        self.inner.push(
            Job::Save {
                item_id: track.id,
                estimate,
            },
            true,
        );
        Some(estimate.measurement.replay_gain())
    }

    pub fn prepare(&self, track: TrackRequest) {
        if self.wants_estimate(track.id) {
            self.inner.push(Job::Measure { track, full: false }, true);
        }
    }

    pub fn measure(&self, track: TrackRequest) {
        if self.is_on() && !self.inner.has_full(track.id) && !self.inner.is_tagged(track.id) {
            self.inner.push(Job::Measure { track, full: true }, false);
        }
    }
}

impl Inner {
    fn lock_queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_values(&self) -> MutexGuard<'_, HashMap<i64, Value>> {
        self.values.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn known(&self, item_id: i64) -> Option<ReplayGain> {
        self.lock_values().get(&item_id).map(|value| value.gain)
    }

    fn is_tagged(&self, item_id: i64) -> bool {
        self.tagged
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&item_id)
    }

    fn leaves_to_tags(&self, track: &TrackRequest, start_ms: i64, decoder: &Decoder) -> bool {
        if !takes_file_tags(track.is_cue, start_ms) || decoder.replay_gain().is_none() {
            return false;
        }
        self.tagged
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(track.id);
        true
    }

    fn has_full(&self, item_id: i64) -> bool {
        self.lock_values()
            .get(&item_id)
            .is_some_and(|value| value.full)
    }

    fn remember(&self, item_id: i64, estimate: Estimate) -> bool {
        let mut values = self.lock_values();
        if values
            .get(&item_id)
            .is_some_and(|value| value.full && !estimate.full)
        {
            return false;
        }
        values.insert(
            item_id,
            Value {
                full: estimate.full,
                gain: estimate.measurement.replay_gain(),
            },
        );
        true
    }

    fn push(self: &Arc<Self>, job: Job, urgent: bool) {
        let mut queue = self.lock_queue();
        if self.stopped.load(Ordering::Relaxed) {
            return;
        }
        if let Job::Measure { track, full } = &job
            && queue.jobs.iter().any(|queued| {
                matches!(queued, Job::Measure { track: t, full: f } if t.id == track.id && f == full)
            })
        {
            return;
        }
        if urgent {
            queue.jobs.push_front(job);
        } else {
            queue.jobs.push_back(job);
        }
        if std::mem::replace(&mut queue.draining, true) {
            return;
        }
        drop(queue);
        let inner = self.clone();
        (self.spawner)(Box::new(move || inner.drain()));
    }

    fn next_job(&self) -> Option<Job> {
        let mut queue = self.lock_queue();
        let job = if self.stopped.load(Ordering::Relaxed) {
            queue
                .jobs
                .iter()
                .position(|job| matches!(job, Job::Save { .. }))
                .and_then(|save| queue.jobs.remove(save))
        } else {
            queue.jobs.pop_front()
        };
        if job.is_none() {
            queue.draining = false;
            self.drained.notify_all();
        }
        job
    }

    fn drain(&self) {
        while let Some(job) = self.next_job() {
            let run = || match job {
                Job::Save { item_id, estimate } => self.save(item_id, estimate),
                Job::Measure { track, full } => self.run_measure(&track, full),
            };
            if catch_unwind(AssertUnwindSafe(run)).is_err() {
                log::warn!("loudness: a measurement panicked");
            }
        }
    }

    fn load(&self) {
        if let Err(e) = self.repo.prune_loudness(VERSION) {
            log::warn!("loudness: pruning old values failed: {e}");
        }
        let rows = match self.repo.loudness_values(VERSION) {
            Ok(rows) => rows,
            Err(e) => {
                log::warn!("loudness: loading values failed: {e}");
                return;
            }
        };
        let mut values = self.lock_values();
        for row in rows {
            let stored = Value {
                full: row.full,
                gain: loudness::Measurement {
                    lufs: row.lufs,
                    peak: row.peak,
                }
                .replay_gain(),
            };
            values
                .entry(row.item_id)
                .and_modify(|value| {
                    if stored.full && !value.full {
                        *value = stored;
                    }
                })
                .or_insert(stored);
        }
    }

    fn save(&self, item_id: i64, estimate: Estimate) {
        let row = StoredLoudness {
            item_id,
            full: estimate.full,
            lufs: estimate.measurement.lufs,
            peak: estimate.measurement.peak,
        };
        if let Err(e) = self.repo.save_loudness(VERSION, &row) {
            log::warn!("loudness: saving item {item_id} failed: {e}");
        }
    }

    fn run_measure(&self, track: &TrackRequest, full: bool) {
        if !self.switch.is_on() {
            return;
        }
        let satisfied = if full {
            self.has_full(track.id)
        } else {
            self.known(track.id).is_some()
        };
        if satisfied || self.is_tagged(track.id) {
            return;
        }
        let Some(file) = self.local_file(&track.path) else {
            return;
        };
        let Ok(mut decoder) = Decoder::open(&file) else {
            return;
        };
        if self.leaves_to_tags(track, track.start_offset_ms, &decoder) {
            return;
        }
        let span = span_of(track, track.start_offset_ms);
        let started = Instant::now();
        let estimate = if full {
            let stop = || self.stopped.load(Ordering::Relaxed) || !self.switch.is_on();
            loudness::full(&mut decoder, span, &stop)
        } else {
            loudness::quick(&mut decoder, span, Some(Instant::now() + NEXT_BUDGET))
        };
        let Some(estimate) = estimate else {
            return;
        };
        log::debug!(
            "loudness: item {} {:.1} LUFS ({}) in {:?}",
            track.id,
            estimate.measurement.lufs,
            if estimate.full { "full" } else { "quick" },
            started.elapsed()
        );
        if self.remember(track.id, estimate) {
            self.save(track.id, estimate);
        }
    }

    fn local_file(&self, locator: &str) -> Option<PathBuf> {
        match remote::location(locator) {
            Location::File(file) => file.exists().then(|| file.to_path_buf()),
            Location::Remote(reference) => self.media.peek_cached(&reference),
            Location::Invalid => None,
        }
    }
}

fn slow_to_sample(file: &Path) -> bool {
    file.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            SLOW_TO_SAMPLE
                .iter()
                .any(|slow| extension.eq_ignore_ascii_case(slow))
        })
}

#[cfg(test)]
mod tests;
