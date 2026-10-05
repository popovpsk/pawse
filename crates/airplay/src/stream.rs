use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::handshake::TimingServer;
use crate::metadata::{self, NowPlaying, RemoteCommand};
use crate::rtsp::Rtsp;
use crate::secure::AudioCipher;
use crate::{
    CHANNELS, Device, Error, FRAMES_PER_PACKET, LATENCY_FRAMES, Protocol, SAMPLE_RATE, alac,
    handshake, rtp, volume_db,
};

const HISTORY: usize = 1024;
const SYNC_EVERY: Duration = Duration::from_secs(1);
const KEEPALIVE_EVERY: Duration = Duration::from_secs(15);
const FEEDBACK_EVERY: Duration = Duration::from_secs(2);
const UDP_POLL: Duration = Duration::from_millis(100);
const MAX_LAG: i64 = SAMPLE_RATE as i64;
const SEND_FAILURES_LIMIT: u32 = 200;

pub trait Render: Send + 'static {
    fn render(&mut self, out: &mut [i16]);
    fn rewind(&mut self, frames: usize);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    Lost(String),
    Remote(RemoteCommand),
}

enum Control {
    Play,
    Pause { flush: bool },
    Flush,
    Volume,
    NowPlaying,
    Progress,
    Close,
}

#[derive(Clone, Copy)]
struct Progress {
    heard_ms: i64,
    duration_ms: u64,
}

#[derive(Clone, Copy)]
struct Clock {
    anchor: Instant,
    anchor_frame: i64,
}

impl Clock {
    fn playhead(&self, now: Instant) -> i64 {
        let elapsed = now.saturating_duration_since(self.anchor);
        self.anchor_frame + (elapsed.as_secs_f64() * f64::from(SAMPLE_RATE)) as i64
    }
}

struct Timing {
    clock: Option<Clock>,
    next_frame: i64,
}

impl Timing {
    fn unheard(&self, now: Instant, device_latency: u32) -> u64 {
        let Some(clock) = self.clock else {
            return 0;
        };
        let latency = i64::from(LATENCY_FRAMES) + i64::from(device_latency);
        let queued = self.next_frame - clock.playhead(now) + i64::from(device_latency);
        let sent = self.next_frame - (clock.anchor_frame + i64::from(LATENCY_FRAMES));
        queued.min(sent).clamp(0, latency) as u64
    }
}

struct Shared {
    timing: Mutex<Timing>,
    device_latency: u32,
    volume: AtomicU32,
    volume_pending: AtomicBool,
    now_playing: Mutex<Option<NowPlaying>>,
    progress: Mutex<Option<Progress>>,
    alive: AtomicBool,
    closed: AtomicBool,
    history: Mutex<VecDeque<(u16, Vec<u8>)>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct Stream {
    control: flume::Sender<Control>,
    shared: Arc<Shared>,
    events: flume::Receiver<StreamEvent>,
    threads: Vec<JoinHandle<()>>,
    active_remote: u32,
    _timing: TimingServer,
}

impl Stream {
    pub fn start(device: &Device, volume: f32, render: Box<dyn Render>) -> Result<Self, Error> {
        let link = match device.protocol {
            Protocol::Raop => handshake::raop(device, volume)?,
            Protocol::AirPlay2 => handshake::airplay2(device, volume)?,
        };
        link.control.set_read_timeout(Some(UDP_POLL)).ok();

        let shared = Arc::new(Shared {
            timing: Mutex::new(Timing {
                clock: None,
                next_frame: 0,
            }),
            alive: AtomicBool::new(true),
            closed: AtomicBool::new(false),
            device_latency: link.device_latency,
            volume: AtomicU32::new(volume.to_bits()),
            volume_pending: AtomicBool::new(false),
            now_playing: Mutex::new(None),
            progress: Mutex::new(None),
            history: Mutex::new(VecDeque::with_capacity(HISTORY)),
        });
        let (events_tx, events) = flume::unbounded();
        let remote = events_tx.clone();
        let timing_stop = link.timing.stopper();
        let (control_tx, control_rx) = flume::unbounded();
        let resends = link
            .control
            .try_clone()
            .map_err(|e| Error::Io(e.to_string()))?;
        let resends_as_audio = (device.protocol == Protocol::AirPlay2)
            .then(|| link.audio.try_clone())
            .transpose()
            .map_err(|e| Error::Io(e.to_string()))?;
        let sender = Sender {
            rtsp: link.rtsp,
            audio: link.audio,
            control: link.control,
            control_target: link.control_target,
            cipher: link.cipher,
            timing_stop,
            last_progress: None,
            ahead: None,
            held: None,
            shows: device.shows,
            feedback: device.protocol == Protocol::AirPlay2,
            ssrc: link.ssrc,
            base_seq: link.base_seq,
            base_ts: link.base_ts,
            packets: 0,
            render,
            shared: shared.clone(),
            events: events_tx,
            commands: control_rx,
            first: true,
            last_sync: Instant::now(),
            last_keepalive: Instant::now(),
            send_failures: 0,
            payload: vec![0; FRAMES_PER_PACKET * CHANNELS],
        };
        let mut threads = Vec::new();
        if let Some(channel) = link.events {
            let watched = shared.clone();
            threads.push(
                std::thread::Builder::new()
                    .name("airplay-events".into())
                    .spawn(move || {
                        channel.serve(
                            || {
                                watched.closed.load(Ordering::Acquire)
                                    || !watched.alive.load(Ordering::Acquire)
                            },
                            |command| {
                                let _ = remote.send(StreamEvent::Remote(command));
                            },
                        );
                    })
                    .map_err(|e| Error::Io(e.to_string()))?,
            );
        }
        let resend_shared = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("airplay-control".into())
                .spawn(move || serve_resends(resends, resends_as_audio, resend_shared))
                .map_err(|e| Error::Io(e.to_string()))?,
        );
        threads.push(
            std::thread::Builder::new()
                .name("airplay-sender".into())
                .spawn(move || sender.run())
                .map_err(|e| Error::Io(e.to_string()))?,
        );
        log::info!(
            "AirPlay: streaming to {} at {} ({:?})",
            device.name,
            device.address,
            device.protocol
        );
        Ok(Self {
            control: control_tx,
            shared,
            events,
            threads,
            active_remote: link.active_remote,
            _timing: link.timing,
        })
    }

    pub fn play(&self) {
        let _ = self.control.send(Control::Play);
    }

    pub fn pause(&self, flush: bool) {
        let _ = self.control.send(Control::Pause { flush });
    }

    pub fn flush(&self) {
        let _ = self.control.send(Control::Flush);
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared
            .volume
            .store(volume.to_bits(), Ordering::Release);
        if !self.shared.volume_pending.swap(true, Ordering::AcqRel) {
            let _ = self.control.send(Control::Volume);
        }
    }

    pub fn set_now_playing(&self, now: NowPlaying) {
        if lock(&self.shared.now_playing).replace(now).is_none() {
            let _ = self.control.send(Control::NowPlaying);
        }
    }

    pub fn set_progress(&self, heard_ms: i64, duration_ms: u64) {
        let progress = Progress {
            heard_ms,
            duration_ms,
        };
        if lock(&self.shared.progress).replace(progress).is_none() {
            let _ = self.control.send(Control::Progress);
        }
    }

    pub fn active_remote(&self) -> u32 {
        self.active_remote
    }

    pub fn is_alive(&self) -> bool {
        self.shared.alive.load(Ordering::Acquire)
    }

    pub fn events(&self) -> flume::Receiver<StreamEvent> {
        self.events.clone()
    }

    pub fn unheard_frames(&self) -> u64 {
        lock(&self.shared.timing).unheard(Instant::now(), self.shared.device_latency)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.control.send(Control::Close);
        self.shared.closed.store(true, Ordering::Release);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

struct Sender {
    rtsp: Rtsp,
    audio: UdpSocket,
    control: UdpSocket,
    control_target: SocketAddr,
    cipher: Option<AudioCipher>,
    timing_stop: Arc<AtomicBool>,
    last_progress: Option<(i64, i64)>,
    ahead: Option<i64>,
    held: Option<NowPlaying>,
    shows: crate::Shows,
    feedback: bool,
    ssrc: u32,
    base_seq: u16,
    base_ts: u32,
    packets: u64,
    render: Box<dyn Render>,
    shared: Arc<Shared>,
    events: flume::Sender<StreamEvent>,
    commands: flume::Receiver<Control>,
    first: bool,
    last_sync: Instant,
    last_keepalive: Instant,
    send_failures: u32,
    payload: Vec<i16>,
}

impl Sender {
    fn next_frame(&self) -> i64 {
        (self.packets * FRAMES_PER_PACKET as u64) as i64
    }

    fn timestamp(&self, frame: i64) -> u32 {
        self.base_ts.wrapping_add(frame as u32)
    }

    fn seq(&self) -> u16 {
        self.base_seq.wrapping_add(self.packets as u16)
    }

    fn clock(&self) -> Option<Clock> {
        lock(&self.shared.timing).clock
    }

    fn set_clock(&self, clock: Option<Clock>) {
        let mut timing = lock(&self.shared.timing);
        timing.clock = clock;
        timing.next_frame = self.next_frame();
    }

    fn run(mut self) {
        let mut playing = false;
        let reason = loop {
            let mut wait = Duration::from_secs(1);
            if playing {
                match self.pump() {
                    Ok(next) => wait = next,
                    Err(e) => break e,
                }
            }
            match self.keep_alive() {
                Ok(next) => wait = wait.min(next),
                Err(e) => break e.to_string(),
            }
            if let Err(e) = self.release_when_heard() {
                break e.to_string();
            }
            let command = match self.commands.recv_timeout(wait) {
                Ok(command) => command,
                Err(flume::RecvTimeoutError::Timeout) => continue,
                Err(flume::RecvTimeoutError::Disconnected) => Control::Close,
            };
            let outcome = match command {
                Control::Play => {
                    if !playing {
                        playing = true;
                        self.first = true;
                        self.set_clock(Some(Clock {
                            anchor: Instant::now(),
                            anchor_frame: self.next_frame() - i64::from(LATENCY_FRAMES),
                        }));
                    }
                    Ok(())
                }
                Control::Pause { flush } => {
                    let was_playing = std::mem::replace(&mut playing, false);
                    if was_playing && flush {
                        let unheard = {
                            let mut timing = lock(&self.shared.timing);
                            timing.next_frame = self.next_frame();
                            timing.unheard(Instant::now(), self.shared.device_latency)
                        };
                        self.set_clock(None);
                        self.render.rewind(unheard as usize);
                        self.send_flush()
                    } else {
                        if !was_playing {
                            self.set_clock(None);
                        }
                        Ok(())
                    }
                }
                Control::Flush => {
                    self.first = true;
                    let result = self.send_flush();
                    if playing {
                        self.set_clock(Some(Clock {
                            anchor: Instant::now(),
                            anchor_frame: self.next_frame() - i64::from(LATENCY_FRAMES),
                        }));
                    } else {
                        self.set_clock(None);
                    }
                    result
                }
                Control::Volume => {
                    self.shared.volume_pending.store(false, Ordering::Release);
                    let volume = f32::from_bits(self.shared.volume.load(Ordering::Acquire));
                    let body = format!("volume: {:.6}\r\n", volume_db(volume));
                    self.rtsp
                        .request(
                            "SET_PARAMETER",
                            None,
                            &[],
                            Some(("text/parameters", body.as_bytes())),
                        )
                        .map(|_| ())
                }
                Control::NowPlaying => {
                    let now = lock(&self.shared.now_playing).take();
                    match now {
                        Some(now) if self.ahead.is_some() => {
                            self.held = Some(now);
                            Ok(())
                        }
                        Some(now) => self.send_now_playing(&now),
                        None => Ok(()),
                    }
                }
                Control::Progress => {
                    let progress = lock(&self.shared.progress).take();
                    match progress {
                        Some(progress) => self.send_progress(progress),
                        None => Ok(()),
                    }
                }
                Control::Close => {
                    let _ = self.rtsp.request("TEARDOWN", None, &[], None);
                    self.rtsp.shutdown();
                    self.shared.alive.store(false, Ordering::Release);
                    self.timing_stop.store(true, Ordering::Release);
                    return;
                }
            };
            if let Err(e) = outcome {
                break e.to_string();
            }
        };
        self.rtsp.shutdown();
        self.shared.alive.store(false, Ordering::Release);
        self.timing_stop.store(true, Ordering::Release);
        if playing {
            log::warn!("AirPlay: stream lost: {reason}");
            let _ = self.events.send(StreamEvent::Lost(reason));
        } else {
            log::info!("AirPlay: the idle session ended ({reason}); it reconnects on play");
        }
    }

    fn keep_alive(&mut self) -> Result<Duration, Error> {
        let (every, method, target) = if self.feedback {
            (FEEDBACK_EVERY, "POST", "/feedback")
        } else {
            (KEEPALIVE_EVERY, "OPTIONS", "*")
        };
        let elapsed = self.last_keepalive.elapsed();
        if elapsed < every {
            return Ok(every - elapsed);
        }
        self.last_keepalive = Instant::now();
        let answered = self
            .rtsp
            .request(method, Some(target), &[], None)
            .map(|_| ());
        self.optional("the keep-alive", answered)?;
        Ok(every)
    }

    fn optional(&mut self, what: &str, outcome: Result<(), Error>) -> Result<(), Error> {
        match outcome {
            Err(Error::Io(e)) => Err(Error::Io(e)),
            Err(e) => {
                log::debug!("AirPlay: the device did not take {what}: {e}");
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    fn rtp_info(&self) -> (&'static str, String) {
        (
            "RTP-Info",
            format!("rtptime={}", self.timestamp(self.heard_frame())),
        )
    }

    fn send_now_playing(&mut self, now: &NowPlaying) -> Result<(), Error> {
        let info = [self.rtp_info()];
        if self.shows.text {
            let items = metadata::dmap(now);
            let sent = self
                .rtsp
                .request(
                    "SET_PARAMETER",
                    None,
                    &info,
                    Some(("application/x-dmap-tagged", &items)),
                )
                .map(|_| ());
            self.optional("the track's metadata", sent)?;
        }
        if self.shows.artwork {
            let (kind, bytes) = match &now.cover {
                Some(cover) => (cover.mime.as_str(), cover.bytes.as_slice()),
                None => ("image/none", &[][..]),
            };
            let sent = self
                .rtsp
                .request("SET_PARAMETER", None, &info, Some((kind, bytes)))
                .map(|_| ());
            self.optional("the cover", sent)?;
        }
        match self.last_progress {
            Some((start, end)) => self.send_progress_line(start, end),
            None => Ok(()),
        }
    }

    fn send_progress(&mut self, progress: Progress) -> Result<(), Error> {
        let to_frames = |ms: i64| ms.saturating_mul(i64::from(SAMPLE_RATE)) / 1000;
        let heard = self.heard_frame();
        let start = heard - to_frames(progress.heard_ms);
        let end = start + to_frames(progress.duration_ms.min(i64::MAX as u64) as i64);
        self.last_progress = (progress.duration_ms > 0).then_some((start, end));
        if start > heard {
            self.ahead = Some(start);
            return Ok(());
        }
        self.ahead = None;
        match (self.held.take(), self.last_progress) {
            (Some(now), _) => self.send_now_playing(&now),
            (None, Some((start, end))) => self.send_progress_line(start, end),
            (None, None) => Ok(()),
        }
    }

    fn release_when_heard(&mut self) -> Result<(), Error> {
        let Some(start) = self.ahead else {
            return Ok(());
        };
        if self.heard_frame() < start {
            return Ok(());
        }
        self.ahead = None;
        match (self.held.take(), self.last_progress) {
            (Some(now), _) => self.send_now_playing(&now),
            (None, Some((start, end))) => self.send_progress_line(start, end),
            (None, None) => Ok(()),
        }
    }

    fn heard_frame(&self) -> i64 {
        let unheard = {
            let mut timing = lock(&self.shared.timing);
            timing.next_frame = self.next_frame();
            timing.unheard(Instant::now(), self.shared.device_latency)
        };
        self.next_frame() - unheard as i64
    }

    fn send_progress_line(&mut self, start: i64, end: i64) -> Result<(), Error> {
        if !self.shows.progress {
            return Ok(());
        }
        let line = metadata::progress(
            self.timestamp(start),
            self.timestamp(self.heard_frame().max(start)),
            self.timestamp(end),
        );
        let sent = self
            .rtsp
            .request(
                "SET_PARAMETER",
                None,
                &[],
                Some(("text/parameters", line.as_bytes())),
            )
            .map(|_| ());
        self.optional("the progress", sent)
    }

    fn send_flush(&mut self) -> Result<(), Error> {
        self.last_progress = None;
        self.ahead = None;
        lock(&self.shared.history).clear();
        self.first = true;
        let info = format!(
            "seq={};rtptime={}",
            self.seq(),
            self.timestamp(self.next_frame())
        );
        self.rtsp
            .request("FLUSH", None, &[("RTP-Info", info)], None)
            .map(|_| ())
    }

    fn pump(&mut self) -> Result<Duration, String> {
        let Some(mut clock) = self.clock() else {
            return Ok(Duration::from_millis(50));
        };
        let now = Instant::now();
        let mut playhead = clock.playhead(now);
        if self.next_frame() + MAX_LAG < playhead + i64::from(LATENCY_FRAMES) {
            log::debug!("AirPlay: fell behind, restarting the clock");
            clock = Clock {
                anchor: now,
                anchor_frame: self.next_frame() - i64::from(LATENCY_FRAMES),
            };
            self.first = true;
            self.set_clock(Some(clock));
            playhead = clock.playhead(now);
        }
        if self.first || self.last_sync.elapsed() >= SYNC_EVERY {
            self.send_sync(playhead);
        }
        while self.next_frame() <= playhead + i64::from(LATENCY_FRAMES) {
            self.send_packet()?;
        }
        lock(&self.shared.timing).next_frame = self.next_frame();
        let ahead = self.next_frame() - (playhead + i64::from(LATENCY_FRAMES));
        let wait = Duration::from_secs_f64(ahead.max(1) as f64 / f64::from(SAMPLE_RATE));
        Ok(wait.min(Duration::from_millis(20)))
    }

    fn send_sync(&mut self, playhead: i64) {
        let packet = rtp::sync_packet(
            self.first,
            self.timestamp(playhead),
            rtp::ntp_now(),
            self.timestamp(playhead + i64::from(LATENCY_FRAMES)),
        );
        self.last_sync = Instant::now();
        if let Err(e) = self.control.send_to(&packet, self.control_target) {
            log::debug!("AirPlay: sync packet failed: {e}");
        }
    }

    fn send_packet(&mut self) -> Result<(), String> {
        self.render.render(&mut self.payload);
        let frame = alac::encode_uncompressed(&self.payload);
        let seq = self.seq();
        let header = rtp::audio_header(
            self.first,
            seq,
            self.timestamp(self.next_frame()),
            self.ssrc,
        );
        let packet = match &self.cipher {
            Some(cipher) => cipher.seal_packet(&header, seq, &frame),
            None => [&header[..], &frame].concat(),
        };
        self.first = false;
        self.packets += 1;
        match self.audio.send(&packet) {
            Ok(_) => self.send_failures = 0,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => {
                self.send_failures += 1;
                if self.send_failures >= SEND_FAILURES_LIMIT {
                    return Err(format!("audio packets fail: {e}"));
                }
            }
        }
        let mut history = lock(&self.shared.history);
        if history.len() == HISTORY {
            history.pop_front();
        }
        history.push_back((seq, packet));
        Ok(())
    }
}

fn serve_resends(socket: UdpSocket, audio: Option<UdpSocket>, shared: Arc<Shared>) {
    let mut buffer = [0u8; 128];
    while !shared.closed.load(Ordering::Acquire) && shared.alive.load(Ordering::Acquire) {
        let Ok((len, from)) = socket.recv_from(&mut buffer) else {
            continue;
        };
        let Some((first, count)) = rtp::resend_request(&buffer[..len]) else {
            continue;
        };
        let wanted: Vec<(u16, Option<Vec<u8>>)> = {
            let history = lock(&shared.history);
            (0..count.min(HISTORY as u16))
                .map(|offset| {
                    let seq = first.wrapping_add(offset);
                    let stored = history.iter().find(|(stored, _)| *stored == seq);
                    (seq, stored.map(|(_, packet)| packet.clone()))
                })
                .collect()
        };
        for (seq, stored) in wanted {
            let _ = match (stored, &audio) {
                (Some(packet), Some(audio)) => audio.send(&packet),
                (Some(packet), None) => socket.send_to(&rtp::resend_packet(&packet), from),
                (None, Some(_)) => socket.send_to(&rtp::futile_resend(seq), from),
                (None, None) => Ok(0),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::FakeReceiver;

    struct Ramp(i16);

    impl Render for Ramp {
        fn render(&mut self, out: &mut [i16]) {
            for sample in out {
                *sample = self.0;
                self.0 = self.0.wrapping_add(1);
            }
        }

        fn rewind(&mut self, _frames: usize) {}
    }

    fn fake_device(address: SocketAddr) -> Device {
        Device {
            id: "FA4E00000001".into(),
            name: "Fake".into(),
            model: None,
            address,
            protocol: Protocol::AirPlay2,
            shows: crate::Shows::ALL,
        }
    }

    #[test]
    fn an_airplay_2_session_pairs_sets_up_and_streams_encrypted_audio() {
        let fake = FakeReceiver::start("3939");
        let stream = Stream::start(&fake_device(fake.address), 0.5, Box::new(Ramp(0))).unwrap();
        stream.play();
        assert!(fake.wait_for(|heard| heard.payloads.len() >= 20 && heard.syncs >= 1));
        assert!(fake.wait_for(|heard| heard.requests.iter().any(|r| r == "POST /feedback")));
        stream.set_volume(0.25);
        assert!(fake.wait_for(|heard| heard.parameters.len() == 2));
        drop(stream);
        assert!(fake.wait_for(|heard| {
            heard
                .requests
                .last()
                .is_some_and(|request| request.starts_with("TEARDOWN"))
        }));
        let heard = fake.heard();
        let methods: Vec<&str> = heard
            .requests
            .iter()
            .filter_map(|request| request.split(' ').next())
            .collect();
        assert_eq!(
            methods[..7],
            [
                "GET",
                "POST",
                "POST",
                "SETUP",
                "RECORD",
                "SETUP",
                "SET_PARAMETER"
            ]
        );
        assert!(heard.events_connected);
        assert_eq!(
            heard.parameters,
            ["volume: -15.000000", "volume: -22.500000"]
        );
        let first: Vec<i16> = (0..(FRAMES_PER_PACKET * CHANNELS) as i16).collect();
        assert_eq!(heard.payloads[0].1, alac::encode_uncompressed(&first));
        let (seq, _) = heard.payloads[0];
        assert_eq!(heard.payloads[1].0, seq.wrapping_add(1));
    }

    #[test]
    fn now_playing_progress_and_remote_buttons_travel_over_the_session() {
        let fake = FakeReceiver::start("3939");
        let stream = Stream::start(&fake_device(fake.address), 0.5, Box::new(Ramp(0))).unwrap();
        let events = stream.events();
        stream.set_now_playing(NowPlaying {
            title: "Tarantula".into(),
            artist: Some("Gorillaz".into()),
            album: None,
            cover: Some(metadata::Cover {
                mime: "image/png".into(),
                bytes: std::sync::Arc::new(vec![0x89, b'P', b'N', b'G']),
            }),
        });
        stream.set_progress(1_000, 5_000);
        assert!(fake.wait_for(|heard| heard.parameters.len() == 2));
        assert!(stream.is_alive(), "a refused cover ends nothing");
        let heard = fake.heard();
        assert_eq!(
            heard.metadata,
            [
                ("application/x-dmap-tagged".to_string(), true),
                ("image/png".to_string(), true)
            ]
        );
        let progress: Vec<u32> = heard.parameters[1]
            .trim_start_matches("progress: ")
            .split('/')
            .map(|number| number.parse().unwrap())
            .collect();
        drop(heard);
        assert_eq!(progress[1].wrapping_sub(progress[0]), 44_100);
        assert_eq!(progress[2].wrapping_sub(progress[0]), 220_500);
        stream.set_progress(-1_000, 5_000);
        stream.set_now_playing(NowPlaying {
            title: "Pneuma".into(),
            ..NowPlaying::default()
        });
        stream.play();
        std::thread::sleep(Duration::from_millis(1_000));
        assert_eq!(
            fake.heard().parameters.len(),
            2,
            "the next track is named only once it is heard"
        );
        assert!(fake.wait_for(|heard| heard.parameters.len() == 3));
        let next: Vec<u32> = fake.heard().parameters[2]
            .trim_start_matches("progress: ")
            .split('/')
            .map(|number| number.parse().unwrap())
            .collect();
        assert!(next[1].wrapping_sub(next[0]) < 4_410, "{next:?}");
        assert_eq!(next[2].wrapping_sub(next[0]), 220_500);
        assert_eq!(next[0].wrapping_sub(progress[1]), 44_100);
        assert_eq!(
            fake.heard().metadata[2..],
            [
                ("application/x-dmap-tagged".to_string(), true),
                ("image/none".to_string(), true)
            ],
            "the held title goes with it, and a track without a cover clears the old one"
        );
        assert!(fake.wait_for(|heard| heard.events_connected));
        assert_eq!(fake.press("nitm").as_deref(), Some("RTSP/1.0 200 OK"));
        assert_eq!(
            events.recv_timeout(Duration::from_secs(2)),
            Ok(StreamEvent::Remote(RemoteCommand::Next))
        );
        assert_eq!(fake.press("skpf").as_deref(), Some("RTSP/1.0 200 OK"));
        assert_eq!(fake.press("paus").as_deref(), Some("RTSP/1.0 200 OK"));
        assert_eq!(
            events.recv_timeout(Duration::from_secs(2)),
            Ok(StreamEvent::Remote(RemoteCommand::Pause))
        );
    }

    #[test]
    fn a_title_held_for_the_next_track_survives_a_pause_before_it_is_heard() {
        let fake = FakeReceiver::start("3939");
        let stream = Stream::start(&fake_device(fake.address), 0.5, Box::new(Ramp(0))).unwrap();
        stream.play();
        stream.set_now_playing(NowPlaying {
            title: "Tarantula".into(),
            ..NowPlaying::default()
        });
        stream.set_progress(1_000, 5_000);
        assert!(fake.wait_for(|heard| heard.metadata.len() == 2));
        stream.set_progress(-1_000, 5_000);
        stream.set_now_playing(NowPlaying {
            title: "Pneuma".into(),
            ..NowPlaying::default()
        });
        std::thread::sleep(Duration::from_millis(300));
        stream.pause(true);
        stream.set_progress(0, 5_000);
        stream.play();
        assert!(fake.wait_for(|heard| heard.metadata.len() == 4));
    }

    #[test]
    fn a_lost_packet_is_sent_again_on_the_audio_port_and_a_forgotten_one_is_named_futile() {
        let fake = FakeReceiver::start("3939");
        let stream = Stream::start(&fake_device(fake.address), 0.5, Box::new(Ramp(0))).unwrap();
        stream.play();
        assert!(fake.wait_for(|heard| heard.payloads.len() >= 20 && heard.syncs >= 1));
        let (seq, payload) = fake.heard().payloads[5].clone();
        assert!(fake.ask_resend(seq, 1));
        assert!(fake.wait_for(|heard| {
            heard
                .payloads
                .iter()
                .filter(|(again, data)| *again == seq && *data == payload)
                .count()
                == 2
        }));
        let gone = seq.wrapping_sub(5_000);
        assert!(fake.ask_resend(gone, 1));
        assert!(fake.wait_for(|heard| heard.futile == [gone]));
    }

    #[test]
    fn a_device_that_does_not_take_the_transient_code_is_refused_before_any_setup() {
        let fake = FakeReceiver::start("1234");
        let refused = Stream::start(&fake_device(fake.address), 0.5, Box::new(Ramp(0)));
        assert!(
            matches!(refused, Err(Error::Refused(_))),
            "{:?}",
            refused.err()
        );
        assert!(fake.wait_for(|heard| heard.requests.len() == 3));
        assert!(
            !fake
                .heard()
                .requests
                .iter()
                .any(|request| request.starts_with("SETUP"))
        );
    }

    fn timing(anchor_frame: i64, next_frame: i64, anchored: Duration) -> (Timing, Instant) {
        let now = Instant::now();
        (
            Timing {
                clock: Some(Clock {
                    anchor: now - anchored,
                    anchor_frame,
                }),
                next_frame,
            },
            now,
        )
    }

    #[test]
    fn right_after_a_start_only_what_was_sent_is_unheard() {
        let latency = i64::from(LATENCY_FRAMES);
        let (fresh, now) = timing(-latency, 4_410, Duration::from_millis(100));
        assert_eq!(fresh.unheard(now, 11_025), 4_410);
        let (steady, now) = timing(-latency, 441_000, Duration::from_secs(10));
        let unheard = steady.unheard(now, 11_025);
        assert!((88_200..=88_200 + 11_025).contains(&unheard), "{unheard}");
        let paused = Timing {
            clock: None,
            next_frame: 10,
        };
        assert_eq!(paused.unheard(now, 11_025), 0);
    }
}
