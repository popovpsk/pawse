use std::{path::PathBuf, sync::Arc, thread, time::Duration};

use crate::{Command, EngineEvent};
use audio_common::{AudioBatch, AudioSource};
use audio_decoder::Decoder;
use audio_output::{AudioOutput, FadeEvent, Output};
use flume::TryRecvError;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum AudioEngineState {
    TrackNotSet,
    Paused,
    Playing,
}

/// What the engine should do once the active fade ramp completes.
#[derive(Debug, Clone, Copy)]
enum FadeIntent {
    None,
    /// Fade-out running; pause the output when it lands at zero.
    PauseOut,
    /// Fade-out running; perform this seek when it lands at zero.
    SeekOut(f32),
    /// Fade-in running on resume; emit `Playing` when it lands at unity.
    PlayIn,
    /// Fade-in running after a seek; nothing to emit on completion.
    SeekIn,
}

const POSITION_UPDATE_INTERVAL_MS: u64 = 200;
const FADE_PAUSE_MS: u32 = 300;
const FADE_SEEK_MS: u32 = 160;

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
            track_start: Duration::ZERO,
            track_end: None,
            needs_flush: false,
            fade_intent: FadeIntent::None,
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
        self.send_command(Command::Play { fade_in: true })
    }

    pub fn play_gapless(&self) {
        self.send_command(Command::Play { fade_in: false })
    }

    pub fn seek(&self, point: f32) {
        self.send_command(Command::Seek(point))
    }

    pub fn stop(&self) {
        self.send_command(Command::Stop)
    }

    pub fn set_track(&self, path: PathBuf) {
        self.send_command(Command::SetLocalTrack {
            path,
            start_offset: None,
            track_duration: None,
        })
    }

    pub fn set_track_with_offset(
        &self,
        path: PathBuf,
        start_offset: Option<Duration>,
        track_duration: Option<Duration>,
    ) {
        self.send_command(Command::SetLocalTrack {
            path,
            start_offset,
            track_duration,
        })
    }

    pub fn send_command(&self, command: Command) {
        if self.command_sender.send(command).is_err() {
            log::error!("audio engine: command channel closed; dropping command");
        }
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
    track_start: Duration,
    track_end: Option<Duration>,
    needs_flush: bool,
    fade_intent: FadeIntent,
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
                        Err(_) => return,
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

            // A fade ramp may have just completed in the audio callback. When it
            // resolves into a pause we restart the loop so the top blocks on
            // `recv` instead of decoding ahead while parked. We deliberately keep
            // `current_audio_batch`: it is the already-decoded remainder that
            // didn't fit the ring buffer, and writing it first on resume is what
            // makes playback continue from the exact same sample (no skip).
            if self.handle_fade_event() {
                continue;
            }

            if self.needs_flush {
                self.output.clear();
                current_audio_batch = None;
                self.needs_flush = false;
            }

            if let Some(track_end) = self.track_end
                && self.current_position >= track_end
            {
                self.set_state(AudioEngineState::TrackNotSet);
                self.decoder = None;
                self.fade_intent = FadeIntent::None;
                _ = self.event_sender.send(EngineEvent::TrackEnded);
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

        if let Some(track_end) = self.track_end
            && self.current_position >= track_end
        {
            return;
        }

        // While a seek fade is running we keep playing the OLD position to ramp
        // it down, but the UI has already moved to the target. Stay quiet until
        // `do_seek` emits the authoritative new position, or the slider snaps
        // back for a moment.
        if self.state == AudioEngineState::Playing && !self.is_seeking() {
            let relative = self.current_position.saturating_sub(self.track_start);
            if relative.saturating_sub(self.last_position_update)
                >= Duration::from_millis(POSITION_UPDATE_INTERVAL_MS)
            {
                self.last_position_update = relative;
                _ = self
                    .event_sender
                    .send(EngineEvent::PositionChanged(relative));
            }
        }
    }

    fn is_seeking(&self) -> bool {
        matches!(
            self.fade_intent,
            FadeIntent::SeekOut(_) | FadeIntent::SeekIn
        )
    }

    fn decode_next_batch(&mut self) -> Option<AudioBatch> {
        let decoder = self.decoder.as_mut()?;

        let next_buffer = decoder.next_buffer();
        let next_buffer = match next_buffer {
            Ok(buffer) => buffer,
            Err(err) => {
                self.set_state(AudioEngineState::TrackNotSet);
                self.decoder = None;
                self.fade_intent = FadeIntent::None;
                _ = self.event_sender.send(EngineEvent::Error(err.to_string()));
                return None;
            }
        };

        let next_buffer = match next_buffer {
            Some(buffer) => buffer,
            None => {
                self.set_state(AudioEngineState::TrackNotSet);
                self.decoder = None;
                self.fade_intent = FadeIntent::None;
                _ = self.event_sender.send(EngineEvent::TrackEnded);
                return None;
            }
        };

        Some(next_buffer)
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Play { fade_in } => self.handle_play(fade_in),
            Command::Pause => self.handle_pause(),
            Command::Seek(position) => self.handle_seek(position),
            Command::SetLocalTrack {
                path,
                start_offset,
                track_duration,
            } => self.handle_set_local_track(path, start_offset, track_duration),
            Command::Stop => self.handle_stop(),
            Command::Shutdown => self.handle_shutdown(),
        }
    }

    fn handle_shutdown(&mut self) {
        self.output.pause();
        self.set_state(AudioEngineState::TrackNotSet);
    }

    fn handle_stop(&mut self) {
        self.output.pause();
        self.output.clear();
        self.output.reset_fade();
        self.fade_intent = FadeIntent::None;
        self.needs_flush = true;
        self.decoder = None;
        self.last_position_update = Duration::ZERO;
        self.current_position = Duration::ZERO;
        self.track_start = Duration::ZERO;
        self.track_end = None;
        self.set_state(AudioEngineState::TrackNotSet);
        _ = self.event_sender.send(EngineEvent::Stopped);
        _ = self
            .event_sender
            .send(EngineEvent::PositionChanged(Duration::ZERO));
    }

    fn handle_set_local_track(
        &mut self,
        path: PathBuf,
        start_offset: Option<Duration>,
        track_duration: Option<Duration>,
    ) {
        self.output.clear();
        self.output.reset_fade();
        self.fade_intent = FadeIntent::None;
        self.needs_flush = true;
        self.decoder = None;
        self.last_position_update = Duration::ZERO;
        self.current_position = Duration::ZERO;
        self.track_start = Duration::ZERO;
        self.track_end = None;

        let decoder = match Decoder::open(path.as_path()) {
            Ok(decoder) => decoder,
            Err(_) => {
                self.output.pause();
                self.set_state(AudioEngineState::TrackNotSet);
                self.decoder = None;
                return;
            }
        };

        let file_duration = decoder.duration().unwrap_or_default();
        let duration_for_ui = track_duration.unwrap_or(file_duration);

        let track_start = start_offset.unwrap_or(Duration::ZERO);
        let track_end = track_duration
            .map(|d| track_start + d)
            .unwrap_or(file_duration);

        self.decoder = Some(decoder);
        self.track_start = track_start;
        self.track_end = if track_end < file_duration {
            Some(track_end)
        } else {
            None
        };

        if track_start > Duration::ZERO {
            let file_dur = file_duration;
            let seek_point = if file_dur > Duration::ZERO {
                (track_start.as_secs_f64() / file_dur.as_secs_f64()) as f32
            } else {
                0.0
            };
            if let Some(decoder) = self.decoder.as_mut()
                && let Err(e) = decoder.seek(seek_point.clamp(0.0, 1.0))
            {
                log::error!("Seek to offset error: {}", e);
            }
            self.current_position = track_start;
        }

        if self
            .event_sender
            .send(EngineEvent::Loaded {
                params: self.decoder.as_ref().unwrap().params(),
                duration: duration_for_ui,
            })
            .is_err()
        {
            log::error!("audio engine: failed to emit Loaded event");
        }

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
            AudioEngineState::Playing => {
                // Duck out first; the actual seek + fade-in happens once the
                // ramp reaches zero (handled in handle_fade_event).
                self.output.begin_fade(None, 0.0, FADE_SEEK_MS);
                self.fade_intent = FadeIntent::SeekOut(position);
                // Pin the UI to the target right away. The real seek is deferred
                // ~160ms for the fade, so without this a PositionChanged that was
                // already in flight (old position) would land after the slider
                // moved and blink it back.
                if let Some(target) = self.seek_target(position) {
                    let relative = target.saturating_sub(self.track_start);
                    self.last_position_update = relative;
                    _ = self
                        .event_sender
                        .send(EngineEvent::PositionChanged(relative));
                }
            }
            AudioEngineState::Paused => {
                // Nothing is audible; seek straight away with no fade.
                self.do_seek(position);
            }
            AudioEngineState::TrackNotSet => {}
        }
    }

    /// Absolute decoder position for a seek `position` (a fraction of the
    /// effective track range). `None` if there is no decoder or its duration
    /// is unknown.
    fn seek_target(&self, position: f32) -> Option<Duration> {
        let file_duration = self.decoder.as_ref()?.duration().unwrap_or_default();
        if file_duration == Duration::ZERO {
            return None;
        }
        let effective_duration = self
            .track_end
            .unwrap_or(file_duration)
            .saturating_sub(self.track_start);
        Some(self.track_start + effective_duration.mul_f32(position))
    }

    /// Clears the output, seeks the decoder to `position` (a fraction of the
    /// effective track range), and emits the new position.
    fn do_seek(&mut self, position: f32) {
        self.output.clear();
        self.needs_flush = true;

        let Some(new_position) = self.seek_target(position) else {
            return;
        };
        let decoder = self.decoder.as_mut().unwrap();
        let file_duration = decoder.duration().unwrap_or_default();

        let seek_point = new_position.as_secs_f64() / file_duration.as_secs_f64();
        if let Err(e) = decoder.seek(seek_point as f32) {
            log::error!("Seek error: {}", e);
            return;
        }
        self.current_position = new_position;
        let relative = new_position.saturating_sub(self.track_start);
        self.last_position_update = relative;
        _ = self
            .event_sender
            .send(EngineEvent::PositionChanged(relative));
    }

    fn handle_play(&mut self, fade_in: bool) {
        match self.state {
            AudioEngineState::Playing => {
                // Reversal: a pause fade-out is in flight — fade back in instead.
                if matches!(self.fade_intent, FadeIntent::PauseOut) {
                    self.output.begin_fade(None, 1.0, FADE_PAUSE_MS);
                    self.fade_intent = FadeIntent::PlayIn;
                }
            }
            AudioEngineState::TrackNotSet => {}
            AudioEngineState::Paused => {
                self.set_state(AudioEngineState::Playing);
                _ = self.event_sender.send(EngineEvent::Playing);
                self.output.resume();
                if fade_in {
                    self.output.begin_fade(Some(0.0), 1.0, FADE_PAUSE_MS);
                    self.fade_intent = FadeIntent::PlayIn;
                }
            }
        }
    }

    fn handle_pause(&mut self) {
        match self.state {
            AudioEngineState::Paused => {}
            AudioEngineState::Playing => {
                if matches!(self.fade_intent, FadeIntent::PauseOut) {
                    return;
                }
                // If a seek fade-out hasn't applied its jump yet, apply it now so
                // we pause at the position the user actually selected rather than
                // the pre-seek one (the slider already moved there).
                if let FadeIntent::SeekOut(position) = self.fade_intent {
                    self.do_seek(position);
                }
                // Stay in Playing and keep feeding the buffer so the ramp is
                // smooth; the real pause + `Paused` event fire on completion.
                self.output.begin_fade(None, 0.0, FADE_PAUSE_MS);
                self.fade_intent = FadeIntent::PauseOut;
            }
            AudioEngineState::TrackNotSet => {}
        }
    }

    /// Acts on a fade ramp that just completed in the audio callback. Returns
    /// `true` if the engine transitioned to a parked (paused) state and the
    /// run loop should restart.
    fn handle_fade_event(&mut self) -> bool {
        let Some(event) = self.output.take_fade_event() else {
            return false;
        };

        match (event, self.fade_intent) {
            (FadeEvent::FadedOut, FadeIntent::PauseOut) => {
                self.fade_intent = FadeIntent::None;
                // No clear: the un-played buffered samples stay pristine so the
                // next fade-in resumes from the exact same spot.
                self.output.pause();
                self.set_state(AudioEngineState::Paused);
                _ = self.event_sender.send(EngineEvent::Paused);
                true
            }
            (FadeEvent::FadedOut, FadeIntent::SeekOut(position)) => {
                self.do_seek(position);
                self.output.begin_fade(Some(0.0), 1.0, FADE_SEEK_MS);
                self.fade_intent = FadeIntent::SeekIn;
                false
            }
            (FadeEvent::FadedIn, FadeIntent::PlayIn) => {
                self.fade_intent = FadeIntent::None;
                false
            }
            (FadeEvent::FadedIn, FadeIntent::SeekIn) => {
                self.fade_intent = FadeIntent::None;
                false
            }
            _ => false,
        }
    }

    fn set_state(&mut self, state: AudioEngineState) {
        log::trace!("audio_engine: new state: {:?}", state);
        self.state = state
    }
}
