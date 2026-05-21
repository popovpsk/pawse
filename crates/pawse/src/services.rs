use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use audio_engine::{AudioEngine, EngineEvent, EngineManager};
use audio_output::Output;
use gpui::{App, AppContext, AsyncApp, Entity, EventEmitter, Global};
use music_library::Track;

use crate::cover_art_cache::CoverArtCache;
use crate::library_service::{LibraryEvent, LibraryService};

pub struct Services {
    pub engine_manager: Rc<EngineManager>,
    pub output: Arc<Output>,
    pub engine_event_bus: Entity<EngineEventsBus>,
    pub library: Arc<LibraryService>,
    pub library_event_bus: Entity<LibraryEventsBus>,
    pub playback_queue: Rc<RefCell<crate::playback_queue::PlaybackQueue>>,
    pub cover_art_cache: Rc<RefCell<CoverArtCache>>,
    pub current_position_ms: Arc<AtomicU64>,
}

impl Services {
    pub fn initialize(cx: &mut App) -> Self {
        let output = Arc::new(Output::new());
        let audio_engine = Rc::new(AudioEngine::new(output.clone()));
        let engine_manager = Rc::new(EngineManager::new(audio_engine).start(cx));
        let engine_event_bus = cx.new(|_| EngineEventsBus);

        let (library_event_tx, library_event_rx) = flume::unbounded();
        let library = Arc::new(LibraryService::new(library_event_tx));
        let library_event_bus = cx.new(|_| LibraryEventsBus);
        let library_event_bus_clone = library_event_bus.clone();

        cx.spawn(async move |cx| {
            while let Ok(event) = library_event_rx.recv_async().await {
                cx.update(|cx| {
                    library_event_bus_clone.update(cx, |_, cx| cx.emit(event));
                })
                .expect("run_library_events_bus:cx.update");
            }
        })
        .detach();

        Services {
            output,
            engine_manager,
            engine_event_bus,
            library,
            library_event_bus,
            playback_queue: Rc::new(RefCell::new(crate::playback_queue::PlaybackQueue::new())),
            cover_art_cache: Rc::new(RefCell::new(CoverArtCache::new())),
            current_position_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn play_track(&self, track: &Track) {
        let path = std::path::PathBuf::from(&track.path);
        let start_offset = if track.start_offset_ms > 0 {
            Some(Duration::from_millis(track.start_offset_ms as u64))
        } else {
            None
        };
        let track_duration = track.duration_ms.map(|ms| Duration::from_millis(ms as u64));
        self.engine_manager
            .set_track_with_offset(path, start_offset, track_duration);
        self.engine_manager.play();
    }
}

impl Services {
    pub fn shutdown(&self) {
        self.output.shutdown();
        self.engine_manager.shutdown();
    }

    pub fn snapshot_playback(&self) -> crate::settings_store::PlaybackState {
        let queue = self.playback_queue.borrow();
        crate::settings_store::PlaybackState {
            queue: queue.tracks_vec(),
            original_queue: queue.original_order_vec(),
            current_index: queue.current_index(),
            position_ms: self.current_position_ms.load(Ordering::Relaxed),
            shuffle: queue.shuffle(),
            repeat: queue.repeat().into(),
        }
    }
}

pub fn save_playback(cx: &mut App) {
    let state = cx.global::<Services>().snapshot_playback();
    if let Err(e) = cx
        .global_mut::<crate::settings_store::SettingsStore>()
        .set_playback(state)
    {
        crate::settings_store::notify_save_error(cx, e);
    }
}

impl Global for Services {}

pub struct EngineEventsBus;

impl EngineEventsBus {}

impl EventEmitter<EngineEvent> for EngineEventsBus {}
impl Global for EngineEventsBus {}

pub struct LibraryEventsBus;

impl LibraryEventsBus {}

impl EventEmitter<LibraryEvent> for LibraryEventsBus {}
impl Global for LibraryEventsBus {}

pub async fn run_engine_events_bus(
    cx: &mut AsyncApp,
    engine_manager: Rc<EngineManager>,
    engine_event_bus: Entity<EngineEventsBus>,
    current_position_ms: Arc<AtomicU64>,
) {
    let rx = engine_manager.events();
    while let Ok(event) = rx.recv_async().await {
        if let EngineEvent::PositionChanged(dur) = &event {
            current_position_ms.store(dur.as_millis() as u64, Ordering::Relaxed);
        }
        cx.update(|cx| engine_event_bus.update(cx, |_, cx| cx.emit(event)))
            .expect("run_engine_events_bus:cx.update")
    }
}
