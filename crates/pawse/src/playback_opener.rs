use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use audio_engine::{Command, StreamTrack, StreamingSource};
use music_library::Track;
use music_library::remote::{self, Location};

use crate::library_service::LibraryService;
use crate::remote_media::{PendingStream, RemoteMedia, StreamControl};

pub type Sink = Arc<dyn Fn(Command) + Send + Sync>;

pub trait OpenerBackend: Send + Sync {
    fn locators(&self, track_id: i64) -> Vec<(String, i64)>;
    fn cached(&self, locator: &Path) -> Option<PathBuf>;
    fn can_stream(&self, locator: &Path) -> bool;
    fn download(&self, locator: &Path, abandoned: &dyn Fn() -> bool) -> Result<PathBuf, String>;
    fn open_stream(&self, locator: &Path) -> Result<PendingStream, String>;
    fn unreachable(&self, locator: &str) -> Option<String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterLoad {
    Stay,
    Play,
    PlayGapless,
}

impl AfterLoad {
    fn autoplay(self) -> Option<bool> {
        match self {
            AfterLoad::Stay => None,
            AfterLoad::Play => Some(true),
            AfterLoad::PlayGapless => Some(false),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackRequest {
    pub id: i64,
    pub path: String,
    pub start_offset_ms: i64,
    pub duration: Option<Duration>,
}

impl From<&Track> for TrackRequest {
    fn from(track: &Track) -> Self {
        Self {
            id: track.id,
            path: track.path.clone(),
            start_offset_ms: i64::from(track.start_offset_ms),
            duration: track
                .duration_ms
                .map(|ms| Duration::from_millis(ms.max(0) as u64)),
        }
    }
}

type Slot = Arc<Mutex<Option<(u64, Arc<dyn StreamControl>)>>>;

enum Opened {
    Cached,
    Stream(StreamingSource),
}

#[derive(Clone)]
pub struct PlaybackOpener {
    backend: Arc<dyn OpenerBackend>,
    sink: Sink,
    generation: Arc<AtomicU64>,
    opening: Slot,
}

fn offset(start_ms: i64) -> Option<Duration> {
    (start_ms > 0).then(|| Duration::from_millis(start_ms as u64))
}

impl PlaybackOpener {
    pub fn new(backend: Arc<dyn OpenerBackend>, sink: Sink) -> Self {
        Self {
            backend,
            sink,
            generation: Arc::new(AtomicU64::new(0)),
            opening: Arc::new(Mutex::new(None)),
        }
    }

    pub fn stop(&self) {
        self.supersede();
        (self.sink)(Command::Stop);
    }

    pub fn start(&self, track: &TrackRequest, after: AfterLoad) {
        let generation = self.supersede();
        let path = PathBuf::from(&track.path);
        let ready = match remote::location(&track.path) {
            Location::File(file) => file.exists(),
            Location::Remote(_) => self.backend.cached(&path).is_some(),
            Location::Invalid => false,
        };
        if ready {
            (self.sink)(Command::SetLocalTrack {
                path,
                start_offset: offset(track.start_offset_ms),
                track_duration: track.duration,
                prepared: false,
            });
            if let Some(fade_in) = after.autoplay() {
                (self.sink)(Command::Play { fade_in });
            }
            return;
        }
        (self.sink)(Command::Prepare {
            play: after.autoplay(),
            track_duration: track.duration,
        });
        let opener = self.clone();
        let track = track.clone();
        let spawned = std::thread::Builder::new()
            .name("track-opener".into())
            .spawn(move || opener.open(&track, generation));
        if let Err(e) = spawned {
            (self.sink)(Command::Fail(e.to_string()));
        }
    }

    fn supersede(&self) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        if let Some((_, previous)) = self.lock_slot().take() {
            previous.abort();
        }
        generation
    }

    fn lock_slot(&self) -> std::sync::MutexGuard<'_, Option<(u64, Arc<dyn StreamControl>)>> {
        self.opening
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn is_stale(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) != generation
    }

    fn open(&self, track: &TrackRequest, generation: u64) {
        let Some(command) = self.resolve(track, generation) else {
            return;
        };
        let _slot = self.lock_slot();
        if !self.is_stale(generation) {
            (self.sink)(command);
        }
    }

    fn resolve(&self, track: &TrackRequest, generation: u64) -> Option<Command> {
        let mut candidates = self.backend.locators(track.id);
        if candidates.is_empty() {
            candidates.push((track.path.clone(), track.start_offset_ms));
        }
        let mut failure = None;
        for (locator, start_ms) in candidates {
            if self.is_stale(generation) {
                return None;
            }
            let candidate = PathBuf::from(&locator);
            match remote::location(&locator) {
                Location::File(file) if file.exists() => {
                    return Some(self.local(candidate, start_ms, track.duration));
                }
                Location::File(_) | Location::Invalid => continue,
                Location::Remote(_) => {}
            }
            match self.open_remote(&candidate, generation) {
                Ok(Opened::Cached) => {
                    return Some(self.local(candidate, start_ms, track.duration));
                }
                Ok(Opened::Stream(source)) => {
                    return Some(Command::SetStreamTrack(Box::new(StreamTrack {
                        source,
                        start_offset: offset(start_ms),
                        track_duration: track.duration,
                    })));
                }
                Err(_) if self.is_stale(generation) => return None,
                Err(e) => {
                    log::warn!("{locator}: {e}");
                    failure = Some(self.backend.unreachable(&locator).unwrap_or(e));
                }
            }
        }
        Some(match failure {
            Some(message) => Command::Fail(message),
            None => self.local(
                PathBuf::from(&track.path),
                track.start_offset_ms,
                track.duration,
            ),
        })
    }

    fn local(&self, path: PathBuf, start_ms: i64, duration: Option<Duration>) -> Command {
        Command::SetLocalTrack {
            path,
            start_offset: offset(start_ms),
            track_duration: duration,
            prepared: true,
        }
    }

    fn open_remote(&self, locator: &Path, generation: u64) -> Result<Opened, String> {
        if self.backend.cached(locator).is_some() {
            return Ok(Opened::Cached);
        }
        if !self.backend.can_stream(locator) {
            let stale = || self.is_stale(generation);
            return self
                .backend
                .download(locator, &stale)
                .map(|_| Opened::Cached);
        }
        let pending = self.backend.open_stream(locator)?;
        let control = pending.control.clone();
        {
            let mut slot = self.lock_slot();
            if let Some((_, previous)) = slot.replace((generation, control.clone())) {
                previous.abort();
            }
            if self.is_stale(generation) {
                control.abort();
            }
        }
        let abort = control.clone();
        let source = StreamingSource::open(
            pending.stream,
            Some(pending.extension),
            Box::new(move || abort.abort()),
        );
        let mut slot = self.lock_slot();
        if slot.as_ref().is_some_and(|(owner, _)| *owner == generation) {
            *slot = None;
        }
        drop(slot);
        source
            .map(Opened::Stream)
            .map_err(|error| control.failure().unwrap_or(error))
    }
}

pub struct LibraryBackend {
    pub media: RemoteMedia,
    pub library: Arc<LibraryService>,
}

impl OpenerBackend for LibraryBackend {
    fn locators(&self, track_id: i64) -> Vec<(String, i64)> {
        self.library.playback_locators(track_id)
    }

    fn cached(&self, locator: &Path) -> Option<PathBuf> {
        self.media.cached(locator)
    }

    fn can_stream(&self, locator: &Path) -> bool {
        RemoteMedia::can_stream(locator)
    }

    fn download(&self, locator: &Path, abandoned: &dyn Fn() -> bool) -> Result<PathBuf, String> {
        self.media.resolve(locator, abandoned)
    }

    fn open_stream(&self, locator: &Path) -> Result<PendingStream, String> {
        self.media.open_stream(locator)
    }

    fn unreachable(&self, locator: &str) -> Option<String> {
        let source_id = remote::parse(locator)?.source_id;
        let down = self.media.ping(source_id)?.err()?;
        self.library.mark_source_offline(source_id);
        Some(crate::library_sources::describe_error(&down).to_string())
    }
}

#[cfg(test)]
mod tests;
