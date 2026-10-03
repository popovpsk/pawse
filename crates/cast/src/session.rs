use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::media::{Accepts, Delivery, Media, TrackInfo, plan, probe};
use crate::server::{Body, Entry, MediaServer};

const TICK: Duration = Duration::from_millis(200);
const POLL_EVERY: Duration = Duration::from_millis(1000);
const FAILURES_UNTIL_LOST: u32 = 4;
const END_SLACK: Duration = Duration::from_secs(5);
const START_WAIT: Duration = Duration::from_secs(8);
const NEVER_STARTED: Duration = Duration::from_secs(30);
const SETTLE_FOR: Duration = Duration::from_secs(4);
const JITTER: Duration = Duration::from_millis(1500);
const AGREE_BELOW: Duration = Duration::from_millis(300);
const AGREE_ABOVE: Duration = Duration::from_millis(1300);

#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    Loaded {
        duration: Option<Duration>,
        sample_rate: u32,
        bit_depth: u8,
    },
    Playing,
    Paused,
    Buffering(bool),
    Position(Duration),
    Ended,
    Failed(String),
    Lost(String),
    Volume(f32),
}

pub struct Load {
    pub media: Media,
    pub start: Duration,
    pub autoplay: bool,
}

enum Command {
    Load(Box<Load>),
    Play,
    Pause,
    Seek(Duration),
    Stop,
    Volume(f32),
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Idle,
    Loading,
    Buffering,
    Playing,
    Paused,
    Finished,
    Failed,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Status {
    pub state: Option<State>,
    pub position: Option<Duration>,
    pub duration: Option<Duration>,
    pub volume: Option<f32>,
    pub error: Option<String>,
}

pub(crate) struct Loading<'a> {
    pub url: &'a str,
    pub mime: &'a str,
    pub features: &'a str,
    pub info: &'a TrackInfo,
    pub cover_url: Option<&'a str>,
    pub duration: Option<Duration>,
    pub size: Option<u64>,
    pub start: Duration,
    pub autoplay: bool,
}

pub(crate) trait Driver: Send + Accepts {
    fn peer(&self) -> IpAddr;
    fn load(&mut self, loading: &Loading) -> Result<(), String>;
    fn play(&mut self) -> Result<(), String>;
    fn pause(&mut self) -> Result<(), String>;
    fn seek(&mut self, position: Duration) -> Result<(), String>;
    fn stop(&mut self) -> Result<(), String>;
    fn set_volume(&mut self, volume: f32) -> Result<(), String>;
    fn status(&mut self) -> Result<Status, String>;
    fn lost(&mut self) -> Option<String>;
    fn volume(&mut self) -> Option<f32>;
    fn seeks_on_load(&self) -> bool;
    fn close(&mut self);
}

pub struct Session {
    commands: flume::Sender<Command>,
    events: flume::Receiver<SessionEvent>,
    finished: flume::Receiver<()>,
}

impl Session {
    pub(crate) fn start(driver: Box<dyn Driver>, server: Arc<MediaServer>) -> Self {
        let (commands, receiver) = flume::unbounded();
        let (sender, events) = flume::unbounded();
        let (running, finished) = flume::bounded::<()>(1);
        let worker = Worker {
            _running: running,
            driver,
            server,
            commands: receiver,
            events: sender,
            current: None,
            published: Vec::new(),
            state: State::Idle,
            position: Duration::ZERO,
            position_at: Instant::now(),
            duration: None,
            pending_seek: None,
            ended: false,
            settle: None,
            last_emitted: Duration::ZERO,
            seen_playing: false,
            loaded_at: Instant::now(),
            buffering: false,
            failures: 0,
            last_poll: Instant::now() - POLL_EVERY,
            last_volume: None,
            hold_state_until: None,
        };
        let spawned = std::thread::Builder::new()
            .name("cast-session".into())
            .spawn(move || worker.run());
        if let Err(e) = spawned {
            log::error!("cast: session thread failed to start: {e}");
        }
        Self {
            commands,
            events,
            finished,
        }
    }

    pub fn close(&self, wait: Duration) {
        let _ = self.commands.send(Command::Close);
        let _ = self.finished.recv_timeout(wait);
    }

    pub fn events(&self) -> flume::Receiver<SessionEvent> {
        self.events.clone()
    }

    pub fn load(&self, load: Load) {
        let _ = self.commands.send(Command::Load(Box::new(load)));
    }

    pub fn play(&self) {
        let _ = self.commands.send(Command::Play);
    }

    pub fn pause(&self) {
        let _ = self.commands.send(Command::Pause);
    }

    pub fn seek(&self, position: Duration) {
        let _ = self.commands.send(Command::Seek(position));
    }

    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self.commands.send(Command::Volume(volume));
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Close);
    }
}

struct Current {
    load: Load,
}

struct Worker {
    _running: flume::Sender<()>,
    driver: Box<dyn Driver>,
    server: Arc<MediaServer>,
    commands: flume::Receiver<Command>,
    events: flume::Sender<SessionEvent>,
    current: Option<Current>,
    published: Vec<String>,
    state: State,
    position: Duration,
    position_at: Instant,
    duration: Option<Duration>,
    pending_seek: Option<Duration>,
    ended: bool,
    settle: Option<(Duration, Instant)>,
    last_emitted: Duration,
    seen_playing: bool,
    loaded_at: Instant,
    buffering: bool,
    failures: u32,
    last_poll: Instant,
    last_volume: Option<f32>,
    hold_state_until: Option<Instant>,
}

impl Worker {
    fn emit(&self, event: SessionEvent) {
        let _ = self.events.send(event);
    }

    fn run(mut self) {
        if let Some(volume) = self.driver.volume() {
            self.last_volume = Some(volume);
            self.emit(SessionEvent::Volume(volume));
        }
        loop {
            if let Some(reason) = self.driver.lost() {
                self.lose(reason);
                return;
            }
            let command = match self.commands.recv_timeout(TICK) {
                Ok(command) => Some(command),
                Err(flume::RecvTimeoutError::Timeout) => None,
                Err(flume::RecvTimeoutError::Disconnected) => Some(Command::Close),
            };
            let mut queue: Vec<Command> = command.into_iter().collect();
            queue.extend(self.commands.try_iter());
            for command in coalesce(queue) {
                if !self.handle(command) {
                    self.driver.close();
                    return;
                }
            }
            if self.last_poll.elapsed() >= POLL_EVERY {
                self.last_poll = Instant::now();
                if let Err(reason) = self.poll() {
                    self.lose(reason);
                    return;
                }
            } else {
                self.tick_position();
            }
        }
    }

    fn lose(&mut self, reason: String) {
        log::warn!("cast: session lost: {reason}");
        self.unpublish();
        self.emit(SessionEvent::Lost(reason));
    }

    fn handle(&mut self, command: Command) -> bool {
        let result = match command {
            Command::Load(load) => {
                self.load(*load);
                Ok(())
            }
            Command::Play => self.play(),
            Command::Pause => {
                if self.current.is_none() {
                    return true;
                }
                self.freeze_position();
                self.state = State::Paused;
                self.hold_state();
                self.emit(SessionEvent::Paused);
                self.driver.pause()
            }
            Command::Seek(position) => self.seek(position),
            Command::Stop => {
                self.current = None;
                self.state = State::Idle;
                self.pending_seek = None;
                self.clear_buffering();
                self.unpublish();
                self.driver.stop()
            }
            Command::Volume(volume) => {
                self.last_volume = Some(volume);
                self.driver.set_volume(volume)
            }
            Command::Close => {
                let _ = self.driver.stop();
                self.unpublish();
                return false;
            }
        };
        if let Err(e) = result {
            log::warn!("cast: device command failed: {e}");
        }
        true
    }

    fn unpublish(&mut self) {
        self.server.remove(&self.published);
        self.published.clear();
    }

    fn clear_buffering(&mut self) {
        if std::mem::take(&mut self.buffering) {
            self.emit(SessionEvent::Buffering(false));
        }
    }

    fn hold_state(&mut self) {
        self.hold_state_until = Some(Instant::now() + Duration::from_millis(1500));
    }

    fn freeze_position(&mut self) {
        self.position = self.estimated_position();
        self.position_at = Instant::now();
    }

    fn estimated_position(&self) -> Duration {
        let mut position = self.position;
        if self.state == State::Playing {
            position += self.position_at.elapsed();
        }
        match self.duration {
            Some(duration) => position.min(duration),
            None => position,
        }
    }

    fn tick_position(&mut self) {
        if self.current.is_some() && self.state == State::Playing && !self.buffering {
            self.emit_position(self.estimated_position());
        }
    }

    fn emit_position(&mut self, position: Duration) {
        let position = if self.state == State::Playing
            && position < self.last_emitted
            && self.last_emitted - position < JITTER
        {
            self.last_emitted
        } else {
            position
        };
        self.last_emitted = position;
        self.emit(SessionEvent::Position(position));
    }

    fn jump_to(&mut self, position: Duration) {
        self.position = position;
        self.position_at = Instant::now();
        self.last_emitted = position;
        self.settle = (!position.is_zero()).then(|| (position, Instant::now() + SETTLE_FOR));
        self.emit(SessionEvent::Position(position));
    }

    fn agrees_with(&self, reported: Duration) -> bool {
        let estimate = self.estimated_position();
        estimate + AGREE_BELOW >= reported && estimate <= reported + AGREE_ABOVE
    }

    fn settling_below(&self, reported: Duration) -> bool {
        self.settle
            .is_some_and(|(target, until)| Instant::now() < until && reported + JITTER < target)
    }

    fn play(&mut self) -> Result<(), String> {
        let Some(current) = &self.current else {
            return Ok(());
        };
        if matches!(self.state, State::Idle | State::Finished | State::Failed) {
            let reload = Load {
                media: current.load.media.clone(),
                start: self.estimated_position(),
                autoplay: true,
            };
            self.load(reload);
            return Ok(());
        }
        self.position_at = Instant::now();
        if !self.seen_playing {
            self.loaded_at = Instant::now();
        }
        self.state = State::Playing;
        self.hold_state();
        self.emit(SessionEvent::Playing);
        self.driver.play()?;
        if let Some(position) = self.pending_seek.take() {
            self.wait_until_started();
            self.driver.seek(position)?;
        }
        Ok(())
    }

    fn wait_until_started(&mut self) {
        let deadline = Instant::now() + START_WAIT;
        while Instant::now() < deadline {
            match self.driver.status() {
                Ok(status) if matches!(status.state, Some(State::Playing | State::Paused)) => {
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    log::debug!("cast: waiting for playback: {e}");
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    fn seek(&mut self, position: Duration) -> Result<(), String> {
        if self.current.is_none() {
            return Ok(());
        }
        self.ended = false;
        self.jump_to(position);
        if self.state == State::Paused && self.pending_seek.is_some() {
            self.pending_seek = Some(position);
            return Ok(());
        }
        if matches!(self.state, State::Idle | State::Finished | State::Failed) {
            self.pending_seek = None;
            return Ok(());
        }
        self.hold_state();
        self.driver.seek(position)
    }

    fn load(&mut self, load: Load) {
        self.current = None;
        self.ended = false;
        self.seen_playing = false;
        self.loaded_at = Instant::now();
        self.pending_seek = None;
        self.clear_buffering();
        self.state = State::Loading;
        self.jump_to(load.start);
        match self.publish_and_load(&load) {
            Ok((duration, sample_rate, bit_depth)) => {
                self.duration = duration;
                self.current = Some(Current { load });
                let autoplay = self.current.as_ref().is_some_and(|c| c.load.autoplay);
                self.state = if autoplay {
                    State::Playing
                } else {
                    State::Paused
                };
                self.hold_state();
                self.failures = 0;
                self.emit(SessionEvent::Loaded {
                    duration,
                    sample_rate,
                    bit_depth,
                });
                self.jump_to(self.position);
                self.emit(if autoplay {
                    SessionEvent::Playing
                } else {
                    SessionEvent::Paused
                });
            }
            Err(e) => {
                log::warn!("cast: loading a track failed: {e}");
                self.state = State::Failed;
                self.unpublish();
                self.emit(SessionEvent::Failed(e));
            }
        }
    }

    fn publish_and_load(&mut self, load: &Load) -> Result<(Option<Duration>, u32, u8), String> {
        let media = &load.media;
        let probed = probe(&media.source, &media.extension)?;
        let accepts: &dyn Accepts = self.driver.as_ref();
        let delivery = plan(media, &probed, accepts)?;
        let peer = self.driver.peer();
        let (entry, size, duration) = match delivery {
            Delivery::Original { mime } => {
                let size = media.source.byte_len().ok().flatten();
                let duration = media.length.or(probed.duration);
                (
                    Entry {
                        body: match &media.source {
                            crate::media::Source::File(path) => Body::File(path.clone()),
                            crate::media::Source::Stream(open) => Body::Stream(open.clone()),
                        },
                        mime,
                    },
                    size,
                    duration,
                )
            }
            Delivery::Pcm(spec) => {
                let size = Some(spec.len());
                let duration = Some(spec.duration());
                let mime = spec.mime();
                (
                    Entry {
                        body: Body::Pcm(spec),
                        mime,
                    },
                    size,
                    duration,
                )
            }
        };
        let extension = match &entry.body {
            Body::Pcm(spec) if spec.container == crate::pcm::Container::Wav => "wav".to_string(),
            Body::Pcm(_) => "pcm".to_string(),
            _ => media.extension.clone(),
        };
        let features = entry.content_features();
        let mime = entry.mime.clone();
        let path = self.server.publish(entry, &extension);
        let mut keep = vec![path.clone()];
        let cover_path = media.cover.as_ref().map(|cover| {
            let extension = if cover.mime.contains("png") {
                "png"
            } else {
                "jpg"
            };
            self.server.publish(
                Entry {
                    body: Body::Bytes(cover.bytes.clone()),
                    mime: cover.mime.clone(),
                },
                extension,
            )
        });
        keep.extend(cover_path.clone());
        self.unpublish();
        self.published = keep;
        let url = self.server.url(peer, &path).map_err(|e| e.to_string())?;
        let cover_url = match &cover_path {
            Some(path) => self.server.url(peer, path).ok(),
            None => None,
        };
        log::info!("cast: loading {url} as {mime}");
        let seek_later = !load.start.is_zero() && !self.driver.seeks_on_load();
        self.driver.load(&Loading {
            url: &url,
            mime: &mime,
            features: &features,
            info: &media.info,
            cover_url: cover_url.as_deref(),
            duration,
            size,
            start: load.start,
            autoplay: load.autoplay,
        })?;
        if seek_later {
            if load.autoplay {
                self.wait_until_started();
                self.driver.seek(load.start)?;
            } else {
                self.pending_seek = Some(load.start);
            }
        }
        Ok((duration, probed.sample_rate, probed.bit_depth))
    }

    fn poll(&mut self) -> Result<(), String> {
        let status = match self.driver.status() {
            Ok(status) => {
                self.failures = 0;
                status
            }
            Err(e) => {
                self.failures += 1;
                log::debug!("cast: status poll failed ({}): {e}", self.failures);
                if self.failures >= FAILURES_UNTIL_LOST {
                    return Err(e);
                }
                return Ok(());
            }
        };
        if let Some(volume) = status.volume
            && self
                .last_volume
                .is_none_or(|last| (last - volume).abs() > 0.01)
        {
            self.last_volume = Some(volume);
            self.emit(SessionEvent::Volume(volume));
        }
        if self.current.is_none() {
            return Ok(());
        }
        if let Some(duration) = status.duration.filter(|d| !d.is_zero())
            && self.duration.is_none()
        {
            self.duration = Some(duration);
        }
        let held = self
            .hold_state_until
            .is_some_and(|until| Instant::now() < until);
        let Some(state) = status.state else {
            return Ok(());
        };
        if state == State::Playing {
            self.seen_playing = true;
        }
        if !self.seen_playing && matches!(state, State::Idle | State::Finished) {
            if !held && self.state == State::Playing && self.loaded_at.elapsed() > NEVER_STARTED {
                self.state = State::Failed;
                self.emit(SessionEvent::Failed(
                    "the device did not start playing".into(),
                ));
            }
            return Ok(());
        }
        match state {
            State::Playing | State::Paused => {
                if let Some(position) = status.position
                    && self.pending_seek.is_none()
                    && !held
                    && !self.settling_below(position)
                    && !self.agrees_with(position)
                {
                    self.position = position;
                    self.position_at = Instant::now();
                }
                if self.buffering {
                    self.buffering = false;
                    self.emit(SessionEvent::Buffering(false));
                }
                if !held && state != self.state {
                    self.state = state;
                    self.emit(if state == State::Playing {
                        SessionEvent::Playing
                    } else {
                        SessionEvent::Paused
                    });
                }
                if state == State::Playing {
                    self.emit_position(self.estimated_position());
                }
            }
            State::Buffering | State::Loading => {
                if !self.buffering && self.state == State::Playing {
                    self.buffering = true;
                    self.emit(SessionEvent::Buffering(true));
                }
            }
            State::Finished => {
                if !held && !self.ended && self.state == State::Playing {
                    self.finish();
                }
            }
            State::Idle => {
                if !held && !self.ended && self.state == State::Playing {
                    let near_end = self
                        .duration
                        .is_none_or(|duration| self.estimated_position() + END_SLACK >= duration);
                    if near_end {
                        self.finish();
                    } else {
                        self.freeze_position();
                        self.state = State::Idle;
                        self.emit(SessionEvent::Paused);
                    }
                }
            }
            State::Failed => {
                if !held && self.state != State::Failed {
                    self.state = State::Failed;
                    self.clear_buffering();
                    let reason = status
                        .error
                        .unwrap_or_else(|| "the device could not play this track".into());
                    self.emit(SessionEvent::Failed(reason));
                }
            }
        }
        Ok(())
    }

    fn finish(&mut self) {
        self.ended = true;
        self.state = State::Finished;
        if let Some(duration) = self.duration {
            self.last_emitted = duration;
            self.emit(SessionEvent::Position(duration));
        }
        self.emit(SessionEvent::Ended);
    }
}

fn coalesce(queue: Vec<Command>) -> Vec<Command> {
    let last_load = queue
        .iter()
        .rposition(|command| matches!(command, Command::Load(_)));
    let mut out: Vec<Command> = Vec::with_capacity(queue.len());
    for (index, command) in queue.into_iter().enumerate() {
        let superseded = last_load.is_some_and(|last| index < last);
        if superseded && !matches!(command, Command::Volume(_) | Command::Close) {
            continue;
        }
        if let (Command::Seek(_), Some(Command::Seek(_)))
        | (Command::Volume(_), Some(Command::Volume(_))) = (&command, out.last())
        {
            out.pop();
        }
        out.push(command);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(commands: &[Command]) -> Vec<&'static str> {
        commands
            .iter()
            .map(|command| match command {
                Command::Load(_) => "load",
                Command::Play => "play",
                Command::Pause => "pause",
                Command::Seek(_) => "seek",
                Command::Stop => "stop",
                Command::Volume(_) => "volume",
                Command::Close => "close",
            })
            .collect()
    }

    #[test]
    fn only_the_newest_load_and_the_last_of_adjacent_seeks_are_kept() {
        let media = || Media {
            source: crate::media::Source::File("/x.flac".into()),
            extension: "flac".into(),
            start: Duration::ZERO,
            length: None,
            info: TrackInfo::default(),
            cover: None,
        };
        let load = || {
            Command::Load(Box::new(Load {
                media: media(),
                start: Duration::ZERO,
                autoplay: true,
            }))
        };
        let out = coalesce(vec![
            load(),
            Command::Seek(Duration::from_secs(1)),
            Command::Seek(Duration::from_secs(2)),
            load(),
            Command::Volume(0.1),
            Command::Volume(0.2),
            Command::Pause,
            Command::Seek(Duration::from_secs(3)),
        ]);
        assert_eq!(kinds(&out), vec!["load", "volume", "pause", "seek"]);
        let Command::Volume(volume) = out[1] else {
            panic!()
        };
        assert_eq!(volume, 0.2);
        let Command::Seek(position) = out[3] else {
            panic!()
        };
        assert_eq!(position, Duration::from_secs(3));
    }
}
