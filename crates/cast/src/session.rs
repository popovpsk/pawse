use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::media::{Accepts, Delivery, Media, TrackInfo, plan, probe};
use crate::server::{Body, Entry, MediaServer};

const TICK: Duration = Duration::from_millis(200);
const POLL_EVERY: Duration = Duration::from_millis(1000);
const POLL_NEAR_END: Duration = Duration::from_millis(250);
const NEAR_END_POLLING: Duration = Duration::from_secs(3);
const FAILURES_UNTIL_LOST: u32 = 4;
const FAILING_FOR: Duration = Duration::from_secs(4);
const END_SLACK: Duration = Duration::from_secs(5);
const START_WAIT: Duration = Duration::from_secs(15);
const NEVER_STARTED: Duration = Duration::from_secs(30);
const SETTLE_FOR: Duration = Duration::from_secs(4);
const JITTER: Duration = Duration::from_millis(1500);
const AGREE_BELOW: Duration = Duration::from_millis(300);
const AGREE_ABOVE: Duration = Duration::from_millis(1300);
const STILL_FOR: Duration = Duration::from_millis(2500);
const STILL_POLLS: u32 = 3;
const END_HOLD: Duration = Duration::from_secs(10);
const TARGET_WAIT: Duration = Duration::from_secs(3);
const TARGET_SLACK: Duration = Duration::from_secs(1);
const QUIET_AFTER_SEEK: Duration = Duration::from_millis(300);

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
    fn silence_start(&mut self) {}
    fn restore_sound(&mut self) {}
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
            ended: false,
            settle: None,
            last_emitted: Duration::ZERO,
            seen_playing: false,
            loaded_at: Instant::now(),
            buffering: false,
            failures: 0,
            failing_since: None,
            last_poll: Instant::now() - POLL_EVERY,
            last_volume: None,
            hold_state_until: None,
            reported: None,
            started_at: Instant::now(),
            end_reached: None,
            ours_on_device: false,
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
    sent: Sent,
    on_device: bool,
}

#[derive(Clone)]
struct Sent {
    url: String,
    mime: String,
    features: String,
    cover_url: Option<String>,
    duration: Option<Duration>,
    size: Option<u64>,
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
    ended: bool,
    settle: Option<(Duration, Instant)>,
    last_emitted: Duration,
    seen_playing: bool,
    loaded_at: Instant,
    buffering: bool,
    failures: u32,
    failing_since: Option<Instant>,
    last_poll: Instant,
    last_volume: Option<f32>,
    hold_state_until: Option<Instant>,
    reported: Option<(Duration, Instant, u32, bool)>,
    started_at: Instant,
    end_reached: Option<Instant>,
    ours_on_device: bool,
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
            if self.last_poll.elapsed() >= self.poll_every() {
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

    fn on_device(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| current.on_device)
    }

    fn poll_every(&self) -> Duration {
        let ending = self.on_device()
            && self.state == State::Playing
            && self
                .duration
                .is_some_and(|duration| self.estimated_position() + NEAR_END_POLLING >= duration);
        if ending { POLL_NEAR_END } else { POLL_EVERY }
    }

    fn stop_ours(&mut self) -> Result<(), String> {
        if !std::mem::take(&mut self.ours_on_device) {
            return Ok(());
        }
        self.driver.stop()
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
                if self.on_device() {
                    self.driver.pause()
                } else {
                    Ok(())
                }
            }
            Command::Seek(position) => self.seek(position),
            Command::Stop => {
                self.current = None;
                self.state = State::Idle;
                self.clear_buffering();
                self.unpublish();
                self.stop_ours()
            }
            Command::Volume(volume) => {
                self.last_volume = Some(volume);
                self.driver.set_volume(volume)
            }
            Command::Close => {
                let _ = self.stop_ours();
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
        self.reported = None;
        self.started_at = Instant::now();
        self.end_reached = None;
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

    fn near_end(&self, position: Duration) -> bool {
        self.duration
            .is_some_and(|duration| position + END_SLACK >= duration)
    }

    fn reset_at_end(&self, reported: Duration) -> bool {
        self.end_reached.is_some_and(|at| at.elapsed() < END_HOLD)
            && reported + JITTER < self.estimated_position()
    }

    fn device_state(&mut self, reported: State, position: Option<Duration>) -> State {
        let Some(position) = position else {
            self.reported = None;
            return reported;
        };
        let now = Instant::now();
        let playing = reported == State::Playing;
        let (moved, still_since, still_polls) = match self.reported {
            Some((last, since, polls, true)) if last == position && playing => {
                (false, since, polls + 1)
            }
            Some((last, ..)) => (last != position, now, 0),
            None => (false, now, 0),
        };
        self.reported = Some((position, still_since, still_polls, playing));
        if reported != State::Playing {
            return reported;
        }
        match self.state {
            State::Paused if !moved => State::Paused,
            State::Playing
                if still_polls >= STILL_POLLS
                    && now.duration_since(still_since) >= STILL_FOR
                    && self.started_at.elapsed() >= SETTLE_FOR
                    && !self.near_end(position)
                    && !self.end_reached.is_some_and(|at| at.elapsed() < END_HOLD) =>
            {
                State::Paused
            }
            _ => State::Playing,
        }
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
        let deferred =
            (!current.on_device).then(|| (current.sent.clone(), current.load.media.info.clone()));
        let says_playing = self.state == State::Paused && matches!(self.reported, Some((.., true)));
        let start = self.estimated_position();
        self.position_at = Instant::now();
        self.started_at = Instant::now();
        if !self.seen_playing {
            self.loaded_at = Instant::now();
        }
        self.state = State::Playing;
        self.hold_state();
        self.emit(SessionEvent::Playing);
        let Some((sent, info)) = deferred else {
            if says_playing && let Err(e) = self.driver.pause() {
                log::debug!("cast: pausing before play failed: {e}");
            }
            return self.driver.play();
        };
        match self.send(&sent, &info, start, true) {
            Ok(()) => {
                if let Some(current) = &mut self.current {
                    current.on_device = true;
                }
                self.jump_to(start);
            }
            Err(e) => {
                log::warn!("cast: starting a track failed: {e}");
                self.state = State::Failed;
                self.emit(SessionEvent::Failed(e));
            }
        }
        Ok(())
    }

    fn send(
        &mut self,
        sent: &Sent,
        info: &TrackInfo,
        start: Duration,
        autoplay: bool,
    ) -> Result<(), String> {
        let seek_later = !start.is_zero() && !self.driver.seeks_on_load();
        let loading = Loading {
            url: &sent.url,
            mime: &sent.mime,
            features: &sent.features,
            info,
            cover_url: sent.cover_url.as_deref(),
            duration: sent.duration,
            size: sent.size,
            start,
            autoplay: autoplay && !seek_later,
        };
        if !(seek_later && autoplay) {
            self.driver.load(&loading)?;
            self.ours_on_device = true;
            return Ok(());
        }
        self.driver.silence_start();
        let loaded = self.driver.load(&loading);
        if loaded.is_ok() {
            self.ours_on_device = true;
        }
        let started = loaded.and_then(|()| self.play_and_seek(start));
        match &started {
            Ok(()) => std::thread::sleep(QUIET_AFTER_SEEK),
            Err(_) => {
                let _ = self.stop_ours();
            }
        }
        self.driver.restore_sound();
        started
    }

    fn play_and_seek(&mut self, position: Duration) -> Result<(), String> {
        self.driver.play()?;
        self.wait_until_started();
        self.driver.seek(position)?;
        if !self.wait_until_at(position) {
            log::debug!("cast: the device did not reach {position:?}, seeking again");
            self.driver.seek(position)?;
            self.wait_until_at(position);
        }
        Ok(())
    }

    fn wait_until_at(&mut self, target: Duration) -> bool {
        let deadline = Instant::now() + TARGET_WAIT;
        while Instant::now() < deadline {
            match self.driver.status() {
                Ok(status) => match status.position {
                    Some(position) if position + TARGET_SLACK >= target => return true,
                    Some(_) => {}
                    None => return true,
                },
                Err(e) => log::debug!("cast: waiting for the seek: {e}"),
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    fn wait_until_started(&mut self) {
        let deadline = Instant::now() + START_WAIT;
        while Instant::now() < deadline {
            match self.driver.status() {
                Ok(status) if matches!(status.state, Some(State::Playing | State::Paused)) => {
                    return;
                }
                Ok(_) => {}
                Err(e) => log::debug!("cast: waiting for playback: {e}"),
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
        if !self.on_device() || matches!(self.state, State::Idle | State::Finished | State::Failed)
        {
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
        self.clear_buffering();
        self.state = State::Loading;
        self.jump_to(load.start);
        let defer = !load.autoplay && !self.driver.seeks_on_load();
        if defer && let Err(e) = self.stop_ours() {
            log::warn!("cast: stopping the previous track failed: {e}");
        }
        let loaded = self
            .publish(&load)
            .and_then(|(sent, sample_rate, bit_depth)| {
                if !defer {
                    self.send(&sent, &load.media.info, load.start, load.autoplay)?;
                }
                Ok((sent, sample_rate, bit_depth))
            });
        match loaded {
            Ok((sent, sample_rate, bit_depth)) => {
                let duration = sent.duration;
                self.duration = duration;
                let autoplay = load.autoplay;
                self.current = Some(Current {
                    load,
                    sent,
                    on_device: !defer,
                });
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

    fn publish(&mut self, load: &Load) -> Result<(Sent, u32, u8), String> {
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
        Ok((
            Sent {
                url,
                mime,
                features,
                cover_url,
                duration,
                size,
            },
            probed.sample_rate,
            probed.bit_depth,
        ))
    }

    fn poll(&mut self) -> Result<(), String> {
        let status = match self.driver.status() {
            Ok(status) => {
                self.failures = 0;
                self.failing_since = None;
                status
            }
            Err(e) => {
                self.failures += 1;
                let since = *self.failing_since.get_or_insert_with(Instant::now);
                self.reported = None;
                log::debug!("cast: status poll failed ({}): {e}", self.failures);
                if self.failures >= FAILURES_UNTIL_LOST && since.elapsed() >= FAILING_FOR {
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
        if !self.on_device() {
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
        let Some(reported) = status.state else {
            return Ok(());
        };
        if reported == State::Playing {
            self.seen_playing = true;
        }
        if self.state == State::Playing
            && self.end_reached.is_none()
            && self.near_end(self.estimated_position())
        {
            self.end_reached = Some(Instant::now());
        }
        let state = self.device_state(reported, status.position);
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
                    && !held
                    && !self.settling_below(position)
                    && !self.reset_at_end(position)
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
                    self.freeze_position();
                    self.state = state;
                    if state == State::Paused
                        && let Some(position) = status.position
                        && !self.settling_below(position)
                    {
                        self.position = position;
                        self.emit_position(position);
                    }
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
