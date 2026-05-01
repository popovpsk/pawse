use std::{rc::Rc, sync::Arc};

use audio_engine::{AudioEngine, EngineEvent, EngineManager};
use audio_output::Output;
use gpui::{App, AppContext, AsyncApp, Entity, EventEmitter, Global};

use crate::library_service::{LibraryEvent, LibraryService};

pub struct Services {
    pub engine_manager: Rc<EngineManager>,
    pub output: Arc<Output>,
    pub engine_event_bus: Entity<EngineEventsBus>,
    pub library: Arc<LibraryService>,
    pub library_event_bus: Entity<LibraryEventsBus>,
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
        }
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
) {
    let rx = engine_manager.events();
    while let Ok(event) = rx.recv_async().await {
        cx.update(|cx| engine_event_bus.update(cx, |_, cx| cx.emit(event)))
            .expect("run_engine_events_bus:cx.update")
    }
}
