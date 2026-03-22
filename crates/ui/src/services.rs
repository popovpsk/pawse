use std::{rc::Rc, sync::Arc};

use audio_engine::{AudioEngine, EngineEvent, EngineManager};
use audio_output::Output;
use gpui::{App, AppContext, AsyncApp, Entity, EventEmitter, Global};

pub struct Services {
    pub engine_manager: Rc<EngineManager>,
    pub output: Arc<Output>,
    pub engine_event_bus: Entity<EngineEventsBus>,
}

impl Services {
    pub fn initialize(cx: &mut App) -> Self {
        let output = Arc::new(Output::new());
        let audio_engine = Rc::new(AudioEngine::new(output.clone()));
        let engine_manager = Rc::new(EngineManager::new(audio_engine).start(cx));
        let engine_event_bus = cx.new(|_| EngineEventsBus);

        Services {
            output,
            engine_manager,
            engine_event_bus,
        }
    }
}

impl Global for Services {}

pub struct EngineEventsBus;

impl EngineEventsBus {}

impl EventEmitter<EngineEvent> for EngineEventsBus {}
impl Global for EngineEventsBus {}

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
