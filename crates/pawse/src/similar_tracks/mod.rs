use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

use audio_embedding::EMBEDDING_VERSION;

use gpui::{App, AppContext, Global};
use music_library::LibraryRepository;

use crate::library_service::LibraryEvent;
use crate::services::Services;
use crate::settings_store::SettingsStore;

mod actions;
mod autoplay;
mod fetch;
mod indexing;
mod menu;
mod mix;
mod neighbors;
mod pool;
mod radio;
mod rerank;
mod worker;

pub use actions::OnApplied;
pub use autoplay::{continue_ended_queue, set_queue_refresh};
pub use indexing::open as open_indexing;
pub use menu::queue_menu;
use neighbors::Neighbors;
pub use rerank::Familiarity;

static INSTANCES: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone)]
pub struct SimilarTracks {
    shared: Arc<Shared>,
}

struct Shared {
    repo: Arc<dyn LibraryRepository>,
    neighbors: Neighbors,
    stop: AtomicBool,
    progress: Progress,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle = 0,
    LoadingModel = 1,
    Analysing = 2,
    NoModel = 3,
    Stopped = 4,
    NoGpu = 5,
    GpuFailed = 6,
    GpuRetry = 7,
}

const PHASES: [Phase; 8] = [
    Phase::Idle,
    Phase::LoadingModel,
    Phase::Analysing,
    Phase::NoModel,
    Phase::Stopped,
    Phase::NoGpu,
    Phase::GpuFailed,
    Phase::GpuRetry,
];

#[derive(Default)]
struct Progress {
    phase: AtomicU8,
    failed: AtomicUsize,
    unreachable: AtomicUsize,
}

impl Progress {
    fn set_phase(&self, phase: Phase) {
        self.phase.store(phase as u8, Ordering::Relaxed);
    }

    fn phase(&self) -> Phase {
        PHASES
            .get(self.phase.load(Ordering::Relaxed) as usize)
            .copied()
            .unwrap_or(Phase::Idle)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub phase: Phase,
    pub analysed: usize,
    pub total: usize,
    pub failed: usize,
    pub unreachable: usize,
}

impl SimilarTracks {
    pub fn status(&self) -> music_library::Result<Status> {
        let (analysed, total) = self.shared.repo.embedding_coverage(EMBEDDING_VERSION)?;
        let progress = &self.shared.progress;
        Ok(Status {
            phase: progress.phase(),
            analysed,
            total,
            failed: progress.failed.load(Ordering::Relaxed),
            unreachable: progress.unreachable.load(Ordering::Relaxed),
        })
    }
}

struct Running {
    similar: SimilarTracks,
    requests: flume::Sender<()>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Gpu {
    #[default]
    Unchecked,
    Checking,
    Found,
    Missing,
}

#[derive(Default)]
struct State {
    running: Option<Running>,
    queue_refresh: Option<OnApplied>,
    gpu: Gpu,
}

impl Global for State {}

pub fn setup(cx: &mut App) {
    cx.set_global(State::default());
    let bus = cx.global::<Services>().library_event_bus.clone();
    cx.subscribe(&bus, |_, event: &LibraryEvent, cx| {
        if matches!(
            event,
            LibraryEvent::CatalogChanged | LibraryEvent::RemoteSyncFinished { .. }
        ) {
            request_pass(cx);
        }
    })
    .detach();
    if cx.global::<SettingsStore>().similar_tracks_enabled() {
        start(cx);
    }
}

fn check_gpu(cx: &mut App) {
    cx.global_mut::<State>().gpu = Gpu::Checking;
    let check = cx.background_spawn(async { audio_embedding_gpu::adapter_name() });
    cx.spawn(async move |cx| {
        let adapter = check.await;
        cx.update(|cx| {
            match &adapter {
                Some(name) => log::info!("similar tracks: graphics adapter {name}"),
                None => log::warn!(
                    "similar tracks: no hardware graphics adapter, sound analysis is unavailable"
                ),
            }
            cx.global_mut::<State>().gpu = match adapter {
                Some(_) => Gpu::Found,
                None => Gpu::Missing,
            };
            if cx.global::<SettingsStore>().similar_tracks_enabled() {
                start(cx);
            }
            cx.refresh_windows();
        });
    })
    .detach();
}

pub fn set_enabled(enabled: bool, cx: &mut App) {
    if enabled {
        start(cx);
    } else {
        stop(cx);
    }
}

pub fn gpu_missing(cx: &App) -> bool {
    cx.try_global::<State>()
        .is_some_and(|state| state.gpu == Gpu::Missing)
}

pub fn is_running(cx: &App) -> bool {
    cx.try_global::<State>()
        .is_some_and(|state| state.running.is_some())
}

pub fn current(cx: &App) -> Option<SimilarTracks> {
    cx.try_global::<State>()?
        .running
        .as_ref()
        .map(|running| running.similar.clone())
}

pub fn shutdown(cx: &mut App) {
    stop(cx);
}

fn request_pass(cx: &App) {
    if let Some(running) = cx.try_global::<State>().and_then(|s| s.running.as_ref()) {
        let _ = running.requests.try_send(());
    }
}

fn start(cx: &mut App) {
    let Some(state) = cx.try_global::<State>() else {
        return;
    };
    if state.running.is_some() {
        return;
    }
    match state.gpu {
        Gpu::Found => {}
        Gpu::Unchecked => return check_gpu(cx),
        Gpu::Checking | Gpu::Missing => return,
    }
    let Some(model_dir) = dirs::data_dir().map(|dir| dir.join("pawse").join("models")) else {
        log::error!("similar tracks: no data directory for the model");
        return;
    };
    let scratch_dir = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("pawse")
        .join("analysis")
        .join(format!(
            "{}-{}",
            std::process::id(),
            INSTANCES.fetch_add(1, Ordering::Relaxed)
        ));
    let services = cx.global::<Services>();
    let shared = Arc::new(Shared {
        repo: services.library.repo(),
        neighbors: Neighbors::default(),
        stop: AtomicBool::new(false),
        progress: Progress::default(),
    });
    let (requests, inbox) = flume::bounded(1);
    let _ = requests.try_send(());
    if let Err(e) = worker::spawn(
        shared.clone(),
        services.remote_media.clone(),
        model_dir,
        scratch_dir,
        inbox,
    ) {
        log::error!("similar tracks: could not start the analysis thread: {e}");
        return;
    }
    cx.global_mut::<State>().running = Some(Running {
        similar: SimilarTracks { shared },
        requests,
    });
}

fn stop(cx: &mut App) {
    if cx.try_global::<State>().is_none() {
        return;
    }
    if let Some(running) = cx.global_mut::<State>().running.take() {
        running.similar.shared.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_phase_reads_back_as_itself() {
        let progress = Progress::default();
        assert_eq!(progress.phase(), Phase::Idle);
        for phase in PHASES {
            progress.set_phase(phase);
            assert_eq!(progress.phase(), phase);
        }
    }
}
