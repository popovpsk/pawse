use std::{path::PathBuf, sync::Arc, thread, time::Duration};

use crate::{Command, EngineEvent};
use audio_common::{AudioBatch, AudioSource};
use audio_decoder::Decoder;
use audio_output::{AudioOutput, Output};
use flume::TryRecvError;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum AudioEngineState {
    TrackNotSet,
    Paused,
    Playing,
}

const POSITION_UPDATE_INTERVAL_MS: u64 = 200;

pub struct AudioEngine {
    command_sender: flume::Sender<Command>,
    event_receiver: flume::Receiver<EngineEvent>,
}

impl AudioEngine {
    pub fn new(out: Arc<Output>) -> Self {
        let (event_sender, event_receiver) = flume::bounded(64);
        let (command_sender, command_receiver) = flume::bounded(64);

        AudioEngineLoop {
            output: out,
            decoder: None,
            state: AudioEngineState::TrackNotSet,
            command_receiver,
            event_sender,
            last_position_update: Duration::ZERO,
            current_position: Duration::ZERO,
        }
        .run();

        Self {
            command_sender,
            event_receiver,
        }
    }

    pub fn events(&self) -> flume::Receiver<EngineEvent> {
        self.event_receiver.clone()
    }

    pub fn pause(&self) {
        self.send_command(Command::Pause)
    }

    pub fn play(&self) {
        self.send_command(Command::Play)
    }

    pub fn seek(&self, point: f32) {
        self.send_command(Command::Seek(point))
    }

    pub fn set_track(&self, path: PathBuf) {
        self.send_command(Command::SetLocalTrack(path))
    }

    pub fn send_command(&self, command: Command) {
        self.command_sender
            .send(command)
            .expect("Failed to send command to audio-engine thread")
    }

    pub fn shutdown(&self) {
        self.send_command(Command::Shutdown)
    }
}

struct AudioEngineLoop {
    output: Arc<Output>,
    decoder: Option<Decoder>,
    state: AudioEngineState,
    command_receiver: flume::Receiver<Command>,
    event_sender: flume::Sender<EngineEvent>,
    last_position_update: Duration,
    current_position: Duration,
}

impl AudioEngineLoop {
    pub fn run(self) {
        thread::spawn(move || self.run_loop());
    }

    fn run_loop(mut self) {
        let mut current_audio_batch: Option<AudioBatch> = None;
        let mut should_shutdown = false;

        loop {
            if should_shutdown {
                return;
            }
            let command = {
                if self.state == AudioEngineState::Playing {
                    let command = self.command_receiver.try_recv();
                    match command {
                        Ok(c) => Some(c),
                        Err(err) => match err {
                            TryRecvError::Disconnected => {
                                return;
                            }
                            TryRecvError::Empty => None,
                        },
                    }
                } else {
                    let command = self.command_receiver.recv();
                    match command {
                        Ok(c) => Some(c),
                        Err(_) => return, // Disconnected
                    }
                }
            };

            if let Some(command) = command {
                if matches!(command, Command::Shutdown) {
                    self.handle_command(command);
                    should_shutdown = true;
                    continue;
                }
                self.handle_command(command);
                continue;
            }

            let batch_to_write = match current_audio_batch {
                Some(batch) => batch,
                None => match self.decode_next_batch() {
                    Some(batch) => batch,
                    None => continue,
                },
            };

            let written = self.output.write(&batch_to_write);
            if written == batch_to_write.data.len() {
                current_audio_batch = None;
            } else {
                current_audio_batch = Some(AudioBatch {
                    data: batch_to_write.data.copy_from_offset(written),
                    metadata: batch_to_write.metadata.clone(),
                });
            }

            self.update_current_position(written, &batch_to_write);
        }
    }

    fn update_current_position(&mut self, written: usize, b: &AudioBatch) {
        let params = &b.metadata;
        let channels = params.channels.to_u8() as f32;
        let written_secs = written as f32 / (params.sample_rate as f32 * channels);
        self.current_position += Duration::from_secs_f32(written_secs);

        if self.state == AudioEngineState::Playing
            && self
                .current_position
                .saturating_sub(self.last_position_update)
                >= Duration::from_millis(POSITION_UPDATE_INTERVAL_MS)
        {
            self.last_position_update = self.current_position;
            _ = self
                .event_sender
                .send(EngineEvent::PositionChanged(self.current_position));
        }
    }

    fn decode_next_batch(&mut self) -> Option<AudioBatch> {
        let decoder = self.decoder.as_mut()?;

        let next_buffer = decoder.next_buffer();
        let next_buffer = match next_buffer {
            Ok(buffer) => buffer,
            Err(err) => {
                self.set_state(AudioEngineState::TrackNotSet);
                self.decoder = None;
                _ = self.event_sender.send(EngineEvent::Error(err.to_string()));
                return None;
            }
        };

        let next_buffer = match next_buffer {
            Some(buffer) => buffer,
            None => {
                self.set_state(AudioEngineState::TrackNotSet);
                self.decoder = None;
                _ = self.event_sender.send(EngineEvent::TrackEnded);
                return None;
            }
        };

        Some(next_buffer)
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Play => self.handle_play(),
            Command::Pause => self.handle_pause(),
            Command::Seek(position) => self.handle_seek(position),
            Command::SetLocalTrack(path) => self.handle_set_local_track(path),
            Command::Shutdown => self.handle_shutdown(),
        }
    }

    fn handle_shutdown(&mut self) {
        self.output.pause();
        self.set_state(AudioEngineState::TrackNotSet);
    }

    fn handle_set_local_track(&mut self, path: PathBuf) {
        self.decoder = None;
        self.last_position_update = Duration::ZERO;
        self.current_position = Duration::ZERO;
        let decoder = match Decoder::open(path.as_path()) {
            Ok(decoder) => decoder,
            Err(_) => {
                self.handle_pause();
                self.set_state(AudioEngineState::TrackNotSet);
                self.decoder = None;
                return; //ToDo notification
            }
        };

        self.decoder = Some(decoder);

        self.event_sender
            .send(EngineEvent::Loaded {
                params: self.decoder.as_ref().unwrap().params(),
                duration: self
                    .decoder
                    .as_ref()
                    .unwrap()
                    .duration()
                    .unwrap_or_default(),
            })
            .unwrap();

        match self.state {
            AudioEngineState::TrackNotSet => self.set_state(AudioEngineState::Paused),
            AudioEngineState::Paused => {}
            AudioEngineState::Playing => {}
        }

        _ = self
            .event_sender
            .send(EngineEvent::PositionChanged(Duration::ZERO));
    }

    fn handle_seek(&mut self, position: f32) {
        match self.state {
            AudioEngineState::Paused | AudioEngineState::Playing => {
                let decoder = self.decoder.as_mut().unwrap();
                if let Err(e) = decoder.seek(position) {
                    eprintln!("Seek error: {}", e);
                    return;
                }
                let track_duration = decoder.duration().unwrap_or_default();
                let new_position = track_duration.mul_f32(position);
                self.current_position = new_position;
                self.last_position_update = new_position;
                _ = self
                    .event_sender
                    .send(EngineEvent::PositionChanged(new_position));
            }
            AudioEngineState::TrackNotSet => {}
        }
    }

    fn handle_play(&mut self) {
        match self.state {
            AudioEngineState::Playing => {}
            AudioEngineState::TrackNotSet => {}
            AudioEngineState::Paused => {
                self.set_state(AudioEngineState::Playing);
                self.output.resume();
                _ = self.event_sender.send(EngineEvent::Playing);
            }
        }
    }

    fn handle_pause(&mut self) {
        match self.state {
            AudioEngineState::Paused => {}
            AudioEngineState::Playing => {
                self.set_state(AudioEngineState::Paused);
                self.output.pause();
                _ = self.event_sender.send(EngineEvent::Paused);
            }
            AudioEngineState::TrackNotSet => {}
        }
    }

    fn set_state(&mut self, state: AudioEngineState) {
        println!("audio_engine: new state:{:?}", state);
        self.state = state
    }
}
