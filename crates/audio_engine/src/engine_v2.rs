use std::{path::PathBuf, thread};

use audio_common::{AudioBatch, AudioSource};
use audio_decoder::Decoder;
use audio_output::{AudioOutput, Output};
use tokio::sync::mpsc::{self, error::TryRecvError};

use crate::{Command, EngineEvent};

#[derive(PartialEq, Eq)]
enum AudioEngineState {
    TrackNotSet,
    Paused,
    Playing,
}
pub struct AudioEngine {
    command_sender: mpsc::Sender<Command>,
    event_receiver: mpsc::Receiver<EngineEvent>,
}

impl AudioEngine {
    pub fn new() -> Self {
        let out = Output::new();

        let (event_sender, event_receiver) = mpsc::channel(128);
        let (command_sender, command_receiver) = mpsc::channel(128);

        AudioEngineLoop {
            output: out,
            decoder: None,
            state: AudioEngineState::TrackNotSet,
            command_receiver: command_receiver,
            event_sender: event_sender,
        }
        .run();

        Self {
            command_sender: command_sender,
            event_receiver: event_receiver,
        }
    }

    pub fn events(&self) -> &mpsc::Receiver<EngineEvent> {
        &self.event_receiver
    }

    pub async fn pause(&self) {
        self.send_command(Command::Pause).await
    }

    pub async fn play(&self) {
        self.send_command(Command::Play).await
    }

    pub async fn seek(&self, point: f32) {
        self.send_command(Command::Seek(point)).await
    }

    pub async fn set_track(&self, path: PathBuf) {
        self.send_command(Command::SetLocalTrack(path)).await
    }

    async fn send_command(&self, command: Command) {
        self.command_sender
            .send(command)
            .await
            .expect("Failed to send command to audio-engine thread")
    }
}

pub struct AudioEngineLoop {
    output: Output,
    decoder: Option<Decoder>,
    state: AudioEngineState,
    command_receiver: mpsc::Receiver<Command>,
    event_sender: mpsc::Sender<EngineEvent>,
}

impl AudioEngineLoop {
    pub fn run(self) {
        thread::spawn(move || self.run_loop());
    }

    fn run_loop(mut self) {
        let mut current_audio_batch: Option<AudioBatch> = None;

        loop {
            let command = {
                if self.state == AudioEngineState::Playing {
                    let command = self.command_receiver.try_recv();
                    match command {
                        Ok(c) => Some(c),
                        Err(err) => {
                            if let TryRecvError::Disconnected = err {
                                return;
                            }
                            None
                        }
                    }
                } else {
                    let command = self.command_receiver.blocking_recv();
                    match command {
                        Some(c) => Some(c),
                        None => return, // Disconnected
                    }
                }
            };

            if let Some(command) = command {
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
        }
    }

    fn decode_next_batch(&mut self) -> Option<AudioBatch> {
        if self.decoder.is_none() {
            panic!("Decoder not initialized in decode_next_batch");
        }

        let decoder = self.decoder.as_mut().unwrap();

        let next_buffer = decoder.next_buffer();
        let next_buffer = match next_buffer {
            Ok(buffer) => buffer,
            Err(err) => {
                self.state = AudioEngineState::TrackNotSet;
                self.event_sender.send(EngineEvent::Error(err.to_string()));
                return None;
            }
        };

        let next_buffer = match next_buffer {
            Some(buffer) => buffer,
            None => {
                self.state = AudioEngineState::TrackNotSet;
                self.event_sender.send(EngineEvent::TrackEnded);
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
            Command::SetVolume(volume) => self.handle_set_volume(volume),
            Command::SetLocalTrack(path) => self.handle_set_local_track(path),
        }
    }

    fn handle_set_local_track(&mut self, path: PathBuf) {
        self.decoder = None;
        let decoder = match Decoder::open(path.as_path()) {
            Ok(decoder) => decoder,
            Err(_) => {
                self.handle_pause();
                self.state = AudioEngineState::TrackNotSet;
                self.decoder = None;
                return; //ToDo notification
            }
        };

        self.decoder = Some(decoder);
        self.handle_play();
    }

    fn handle_seek(&mut self, position: f32) {
        match self.state {
            AudioEngineState::Paused | AudioEngineState::Playing => {
                let decoder = self.decoder.as_mut().unwrap();
                decoder.seek(position);
            }
            AudioEngineState::TrackNotSet => {
                panic!("No track set!");
            }
        }
    }

    fn handle_set_volume(&mut self, volume: u8) {
        self.output.set_volume(volume);
    }

    fn handle_play(&mut self) {
        match self.state {
            AudioEngineState::Playing => {
                return;
            }
            AudioEngineState::TrackNotSet => {
                panic!("No track set!");
            }
            AudioEngineState::Paused => {
                self.state = AudioEngineState::Playing;
                self.output.resume()
            }
        }
    }

    fn handle_pause(&mut self) {
        match self.state {
            AudioEngineState::Paused => {
                return;
            }
            AudioEngineState::Playing => {
                self.state = AudioEngineState::Paused;
                self.output.pause();
            }
            AudioEngineState::TrackNotSet => {}
        }
    }
}
