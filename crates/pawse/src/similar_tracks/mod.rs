use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{App, Global};
use music_library::LibraryRepository;

use crate::library_service::LibraryEvent;
use crate::services::Services;
use crate::settings_store::SettingsStore;

mod actions;
mod menu;
mod mix;
mod neighbors;
mod pool;
mod radio;
mod rerank;
mod worker;

pub use actions::OnApplied;
pub use menu::queue_menu;
use neighbors::Neighbors;
pub use rerank::Familiarity;

#[derive(Clone)]
pub struct SimilarTracks {
    shared: Arc<Shared>,
}

struct Shared {
    repo: Arc<dyn LibraryRepository>,
    neighbors: Neighbors,
    stop: AtomicBool,
}

struct Running {
    similar: SimilarTracks,
    requests: flume::Sender<()>,
}

#[derive(Default)]
struct State {
    running: Option<Running>,
}

impl Global for State {}

pub fn setup(cx: &mut App) {
    cx.set_global(State::default());
    let bus = cx.global::<Services>().library_event_bus.clone();
    cx.subscribe(&bus, |_, event: &LibraryEvent, cx| {
        if matches!(
            event,
            LibraryEvent::ScanComplete
                | LibraryEvent::CatalogChanged
                | LibraryEvent::RemoteSyncFinished { .. }
        ) {
            request_pass(cx);
        }
    })
    .detach();
    if cx.global::<SettingsStore>().similar_tracks_enabled() {
        start(cx);
    }
}

pub fn set_enabled(enabled: bool, cx: &mut App) {
    if enabled {
        start(cx);
    } else {
        stop(cx);
    }
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
    if cx
        .try_global::<State>()
        .is_none_or(|state| state.running.is_some())
    {
        return;
    }
    let Some(model_dir) = dirs::data_dir().map(|dir| dir.join("pawse").join("models")) else {
        log::error!("similar tracks: no data directory for the model");
        return;
    };
    let services = cx.global::<Services>();
    let shared = Arc::new(Shared {
        repo: services.library.repo(),
        neighbors: Neighbors::default(),
        stop: AtomicBool::new(false),
    });
    let (requests, inbox) = flume::bounded(1);
    let _ = requests.try_send(());
    if let Err(e) = worker::spawn(
        shared.clone(),
        services.remote_media.clone(),
        model_dir,
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
