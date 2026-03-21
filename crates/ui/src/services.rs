use std::rc::Rc;

use audio_engine::{AudioEngine, EngineEvent};
use audio_output::Output;
use gpui::{App, AppContext, Entity, EventEmitter, Global};

pub struct Services {
    pub audio_engine: Rc<AudioEngine>,
    pub output: Output,
    pub engine_event_bus: Entity<EngineEventsBus>,
}

impl Services {
    pub fn initialize(cx: &mut App) -> Self {
        let output = Output::new();
        let audio_engine = Rc::new(AudioEngine::new());
        let engine_event_bus = cx.new(|_| EngineEventsBus);
        Services {
            output,
            audio_engine,
            engine_event_bus,
        }
    }
}

impl Global for Services {}

pub struct EngineEventsBus;

impl EngineEventsBus {}

impl EventEmitter<EngineEvent> for EngineEventsBus {}
impl Global for EngineEventsBus {}
