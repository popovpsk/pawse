use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::rtsp::{Rtsp, transport_ports};
use crate::{
    CHANNELS, Device, Error, FRAMES_PER_PACKET, LATENCY_FRAMES, SAMPLE_RATE, alac, rtp, volume_db,
};

const HISTORY: usize = 1024;
const SYNC_EVERY: Duration = Duration::from_secs(1);
const KEEPALIVE_EVERY: Duration = Duration::from_secs(15);
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
}

enum Control {
    Play,
    Pause { flush: bool },
    Flush,
    Volume,
    Close,
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
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    if getrandom::fill(&mut bytes).is_err() {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default();
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = (seed >> ((i % 16) * 8)) as u8;
        }
    }
    bytes
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

fn bind_near(local: std::net::IpAddr) -> Result<UdpSocket, Error> {
    UdpSocket::bind(SocketAddr::new(local, 0)).map_err(|e| Error::Io(e.to_string()))
}

fn port(socket: &UdpSocket) -> Result<u16, Error> {
    socket
        .local_addr()
        .map(|address| address.port())
        .map_err(|e| Error::Io(e.to_string()))
}

impl Stream {
    pub fn start(device: &Device, volume: f32, render: Box<dyn Render>) -> Result<Self, Error> {
        let session_id = u32::from_be_bytes(random::<4>()) & 0x7fff_ffff;
        let mut rtsp = Rtsp::connect(device.address, session_id, hex(&random::<8>()))?;
        let local = rtsp.local_ip()?;
        let peer = rtsp.peer_ip()?;
        let control = bind_near(local)?;
        let timing = bind_near(local)?;
        let audio = bind_near(local)?;

        rtsp.request("OPTIONS", Some("*"), &[], None)?;
        let sdp = format!(
            "v=0\r\no=iTunes {session_id} 0 IN IP4 {local}\r\ns=iTunes\r\nc=IN IP4 {peer}\r\nt=0 0\r\n\
m=audio 0 RTP/AVP 96\r\na=rtpmap:96 AppleLossless\r\n\
a=fmtp:96 {FRAMES_PER_PACKET} 0 16 40 10 14 {CHANNELS} 255 0 0 {SAMPLE_RATE}\r\n"
        );
        rtsp.request(
            "ANNOUNCE",
            None,
            &[],
            Some(("application/sdp", sdp.as_bytes())),
        )?;
        let transport = format!(
            "RTP/AVP/UDP;unicast;interleaved=0-1;mode=record;control_port={};timing_port={}",
            port(&control)?,
            port(&timing)?
        );
        let setup = rtsp.request("SETUP", None, &[("Transport", transport)], None)?;
        let ports = transport_ports(setup.header("Transport").unwrap_or_default());
        let server_port = *ports
            .get("server_port")
            .ok_or_else(|| Error::Refused("SETUP did not name an audio port".into()))?;
        let remote_control = ports
            .get("control_port")
            .copied()
            .unwrap_or(server_port + 1);
        let session = setup
            .header("Session")
            .map(|session| {
                session
                    .split(';')
                    .next()
                    .unwrap_or(session)
                    .trim()
                    .to_string()
            })
            .unwrap_or_else(|| "1".to_string());
        rtsp.set_session(session);

        let base_seq = u16::from_be_bytes(random::<2>());
        let base_ts = u32::from_be_bytes(random::<4>());
        let record = rtsp.request(
            "RECORD",
            None,
            &[
                ("Range", "npt=0-".to_string()),
                ("RTP-Info", format!("seq={base_seq};rtptime={base_ts}")),
            ],
            None,
        )?;
        let device_latency = record
            .header("Audio-Latency")
            .and_then(|latency| latency.trim().parse::<u32>().ok())
            .unwrap_or(0)
            .min(SAMPLE_RATE * 2);
        let volume_body = format!("volume: {:.6}\r\n", volume_db(volume));
        rtsp.request(
            "SET_PARAMETER",
            None,
            &[],
            Some(("text/parameters", volume_body.as_bytes())),
        )?;

        audio
            .connect(SocketAddr::new(peer, server_port))
            .map_err(|e| Error::Io(e.to_string()))?;
        control.set_read_timeout(Some(UDP_POLL)).ok();
        timing.set_read_timeout(Some(UDP_POLL)).ok();

        let shared = Arc::new(Shared {
            timing: Mutex::new(Timing {
                clock: None,
                next_frame: 0,
            }),
            alive: AtomicBool::new(true),
            closed: AtomicBool::new(false),
            device_latency,
            volume: AtomicU32::new(volume.to_bits()),
            volume_pending: AtomicBool::new(false),
            history: Mutex::new(VecDeque::with_capacity(HISTORY)),
        });
        let (events_tx, events) = flume::unbounded();
        let (control_tx, control_rx) = flume::unbounded();
        let control_target = SocketAddr::new(peer, remote_control);
        let resends = control.try_clone().map_err(|e| Error::Io(e.to_string()))?;
        let sender = Sender {
            rtsp,
            audio,
            control,
            control_target,
            ssrc: u32::from_be_bytes(random::<4>()),
            base_seq,
            base_ts,
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
        let timing_shared = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("airplay-timing".into())
                .spawn(move || serve_timing(timing, timing_shared))
                .map_err(|e| Error::Io(e.to_string()))?,
        );
        let resend_shared = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("airplay-control".into())
                .spawn(move || serve_resends(resends, resend_shared))
                .map_err(|e| Error::Io(e.to_string()))?,
        );
        threads.push(
            std::thread::Builder::new()
                .name("airplay-sender".into())
                .spawn(move || sender.run())
                .map_err(|e| Error::Io(e.to_string()))?,
        );
        log::info!(
            "AirPlay: streaming to {} at {}",
            device.name,
            device.address
        );
        Ok(Self {
            control: control_tx,
            shared,
            events,
            threads,
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
            } else if self.last_keepalive.elapsed() >= KEEPALIVE_EVERY {
                self.last_keepalive = Instant::now();
                if let Err(e) = self.rtsp.request("OPTIONS", Some("*"), &[], None) {
                    break e.to_string();
                }
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
                Control::Close => {
                    let _ = self.rtsp.request("TEARDOWN", None, &[], None);
                    self.rtsp.shutdown();
                    self.shared.alive.store(false, Ordering::Release);
                    return;
                }
            };
            if let Err(e) = outcome {
                break e.to_string();
            }
        };
        self.rtsp.shutdown();
        self.shared.alive.store(false, Ordering::Release);
        if playing {
            log::warn!("AirPlay: stream lost: {reason}");
            let _ = self.events.send(StreamEvent::Lost(reason));
        } else {
            log::info!("AirPlay: the idle session ended ({reason}); it reconnects on play");
        }
    }

    fn send_flush(&mut self) -> Result<(), Error> {
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
        let packet = rtp::audio_packet(
            self.first,
            seq,
            self.timestamp(self.next_frame()),
            self.ssrc,
            &frame,
        );
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

fn serve_timing(socket: UdpSocket, shared: Arc<Shared>) {
    let mut buffer = [0u8; 128];
    while !shared.closed.load(Ordering::Acquire) && shared.alive.load(Ordering::Acquire) {
        if let Ok((len, from)) = socket.recv_from(&mut buffer) {
            let received = rtp::ntp_now();
            if let Some(reply) = rtp::timing_reply(&buffer[..len], received) {
                let _ = socket.send_to(&reply, from);
            }
        }
    }
}

fn serve_resends(socket: UdpSocket, shared: Arc<Shared>) {
    let mut buffer = [0u8; 128];
    while !shared.closed.load(Ordering::Acquire) && shared.alive.load(Ordering::Acquire) {
        let Ok((len, from)) = socket.recv_from(&mut buffer) else {
            continue;
        };
        let Some((first, count)) = rtp::resend_request(&buffer[..len]) else {
            continue;
        };
        let history = lock(&shared.history);
        for offset in 0..count {
            let seq = first.wrapping_add(offset);
            if let Some((_, packet)) = history.iter().find(|(stored, _)| *stored == seq) {
                let _ = socket.send_to(&rtp::resend_packet(packet), from);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
