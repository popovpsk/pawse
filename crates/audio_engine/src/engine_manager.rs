use std::io::{self, Write};
use std::path::PathBuf;
use std::rc::Rc;

use crate::{AudioEngine, Command, EngineEvent};
use gpui::App;

pub struct EngineManager {
    audio_engine: Rc<AudioEngine>,
    _event_sender: flume::Sender<EngineEvent>,
    _event_receiver: flume::Receiver<EngineEvent>,
}

fn audio_path() -> PathBuf {
    PathBuf::from("/Users/popovaleksa/repo/other/gpui-test/fixtures/02 - Selfless.flac")
}

impl EngineManager {
    pub fn new(audio_engine: Rc<AudioEngine>) -> Self {
        let (event_sender, event_receiver) = flume::bounded(64);

        audio_engine.set_track(audio_path());

        let result = Self {
            audio_engine,
            _event_sender: event_sender,
            _event_receiver: event_receiver,
        };

        result
    }

    pub fn start(self, cx: &mut App) -> Self {
        let rx = self.audio_engine.events();
        let tx = self._event_sender.clone();

        cx.spawn(async move |_| loop {
            match rx.recv_async().await {
                Ok(event) => {
                    match event.clone() {
                        EngineEvent::Error(err) => {
                            io::stderr().write(err.as_bytes()).unwrap();
                        }
                        _ => {
                            println!("EngineManager recevied event: {:?}", event)
                        }
                    };

                    tx.send(event).expect("EngineManager:event_loop:tx.send");
                }
                Err(err) => panic!(
                    "EngineManager:event_loop:engine_event_rx.recv_async: {}",
                    err
                ),
            }
        })
        .detach();
        self
    }

    pub fn play(&self) {
        self.audio_engine.play();
    }

    pub fn pause(&self) {
        self.audio_engine.pause();
    }

    pub fn seek(&self, position: f32) {
        self.audio_engine.seek(position);
    }

    pub fn set_track(&self, path: PathBuf) {
        self.audio_engine.set_track(path);
    }

    pub fn set_volume(&self, volume: u8) {
        self.audio_engine.send_command(Command::SetVolume(volume));
    }

    pub fn events(&self) -> &flume::Receiver<EngineEvent> {
        &self._event_receiver
    }
}
