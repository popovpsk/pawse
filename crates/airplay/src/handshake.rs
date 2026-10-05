use std::io::Cursor;
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use plist::{Dictionary, Value};

use crate::events::EventChannel;
use crate::rtsp::{Rtsp, transport_ports};
use crate::secure::{AudioCipher, FrameCipher};
use crate::{
    CHANNELS, Device, Error, FRAMES_PER_PACKET, LATENCY_FRAMES, SAMPLE_RATE, pairing, random, rtp,
    volume_db,
};

const RAOP_USER_AGENT: &str = "iTunes/7.6.2 (Windows; N;)";
const APPLE_RAOP_USER_AGENT: &str = "AirPlay/999.0.0";
const AIRPLAY2_USER_AGENT: &str = "AirPlay/670.6.2";
const EVENTS_TIMEOUT: Duration = Duration::from_secs(3);
const SETUP_TIMEOUT: Duration = Duration::from_secs(10);
const TIMING_POLL: Duration = Duration::from_millis(100);
const PLIST: &str = "application/x-apple-binary-plist";
const ALAC_44100_16_2: u64 = 0x40000;
const STREAM_REALTIME: u64 = 96;
const COMPRESSION_ALAC: u64 = 2;
const LATENCY_MIN: u64 = 11_025;

pub(crate) struct Link {
    pub active_remote: u32,
    pub rtsp: Rtsp,
    pub audio: UdpSocket,
    pub control: UdpSocket,
    pub timing: TimingServer,
    pub control_target: SocketAddr,
    pub ssrc: u32,
    pub base_seq: u16,
    pub base_ts: u32,
    pub device_latency: u32,
    pub cipher: Option<AudioCipher>,
    pub events: Option<EventChannel>,
}

pub(crate) struct TimingServer {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TimingServer {
    fn start(local: IpAddr) -> Result<Self, Error> {
        let socket = bind_near(local)?;
        socket.set_read_timeout(Some(TIMING_POLL)).ok();
        let port = port(&socket)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::Builder::new()
            .name("airplay-timing".into())
            .spawn(move || {
                let mut buffer = [0u8; 128];
                while !stopped.load(Ordering::Acquire) {
                    if let Ok((len, from)) = socket.recv_from(&mut buffer) {
                        let received = rtp::ntp_now();
                        if let Some(reply) = rtp::timing_reply(&buffer[..len], received) {
                            let _ = socket.send_to(&reply, from);
                        }
                    }
                }
            })
            .map_err(|e| Error::Io(e.to_string()))?;
        Ok(Self {
            port,
            stop,
            thread: Some(thread),
        })
    }
}

impl TimingServer {
    pub fn stopper(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }
}

impl Drop for TimingServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn instance() -> [u8; 8] {
    static INSTANCE: OnceLock<[u8; 8]> = OnceLock::new();
    *INSTANCE.get_or_init(|| {
        let mut id = random::<8>();
        id[0] |= 0x10;
        id
    })
}

pub(crate) fn dacp_id() -> String {
    hex(&instance())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

fn colon_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn uuid() -> String {
    let bytes = hex(&random::<16>());
    format!(
        "{}-{}-{}-{}-{}",
        &bytes[..8],
        &bytes[8..12],
        &bytes[12..16],
        &bytes[16..20],
        &bytes[20..]
    )
}

fn session_id() -> u32 {
    u32::from_be_bytes(random::<4>()) & 0x7fff_ffff
}

fn bind_near(local: IpAddr) -> Result<UdpSocket, Error> {
    UdpSocket::bind(SocketAddr::new(local, 0)).map_err(|e| Error::Io(e.to_string()))
}

fn port(socket: &UdpSocket) -> Result<u16, Error> {
    socket
        .local_addr()
        .map(|address| address.port())
        .map_err(|e| Error::Io(e.to_string()))
}

fn audio_latency(answer: &crate::rtsp::Response) -> u32 {
    answer
        .header("Audio-Latency")
        .and_then(|latency| latency.trim().parse::<u32>().ok())
        .unwrap_or(0)
        .min(SAMPLE_RATE * 2)
}

fn send_volume(rtsp: &mut Rtsp, volume: f32) -> Result<(), Error> {
    let body = format!("volume: {:.6}\r\n", volume_db(volume));
    rtsp.request(
        "SET_PARAMETER",
        None,
        &[],
        Some(("text/parameters", body.as_bytes())),
    )
    .map(|_| ())
}

fn raop_user_agent(model: Option<&str>) -> &'static str {
    let apple = model.is_some_and(|model| {
        let model = model.to_ascii_lowercase();
        model.contains("audioaccessory") || model.contains("appletv")
    });
    if apple {
        APPLE_RAOP_USER_AGENT
    } else {
        RAOP_USER_AGENT
    }
}

pub(crate) fn raop(device: &Device, volume: f32) -> Result<Link, Error> {
    let session_id = session_id();
    let mut rtsp = Rtsp::connect(
        device.address,
        session_id,
        hex(&instance()),
        raop_user_agent(device.model.as_deref()),
    )?;
    let local = rtsp.local_ip()?;
    let peer = rtsp.peer_ip()?;
    let control = bind_near(local)?;
    let timing = TimingServer::start(local)?;
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
        timing.port
    );
    let setup = rtsp.request("SETUP", None, &[("Transport", transport)], None)?;
    let ports = transport_ports(setup.header("Transport").unwrap_or_default());
    let server_port = *ports
        .get("server_port")
        .ok_or_else(|| Error::Refused("SETUP did not name an audio port".into()))?;
    let remote_control = match ports.get("control_port") {
        Some(port) => *port,
        None => server_port
            .checked_add(1)
            .ok_or_else(|| Error::Refused("SETUP named no control port".into()))?,
    };
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
    let device_latency = audio_latency(&record);
    send_volume(&mut rtsp, volume)?;
    audio
        .connect(SocketAddr::new(peer, server_port))
        .map_err(|e| Error::Io(e.to_string()))?;
    Ok(Link {
        active_remote: session_id,
        rtsp,
        audio,
        control,
        timing,
        control_target: SocketAddr::new(peer, remote_control),
        ssrc: u32::from_be_bytes(random::<4>()),
        base_seq,
        base_ts,
        device_latency,
        cipher: None,
        events: None,
    })
}

fn integer(value: u64) -> Value {
    Value::Integer(value.into())
}

fn dictionary(entries: Vec<(&str, Value)>) -> Value {
    Value::Dictionary(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn setup(rtsp: &mut Rtsp, body: &Value) -> Result<Dictionary, Error> {
    let mut bytes = Vec::new();
    body.to_writer_binary(&mut bytes)
        .map_err(|e| Error::Io(format!("SETUP: {e}")))?;
    let answer = rtsp.request("SETUP", None, &[], Some((PLIST, &bytes)))?;
    if answer.body.is_empty() {
        return Ok(Dictionary::new());
    }
    Value::from_reader(Cursor::new(answer.body))
        .ok()
        .and_then(Value::into_dictionary)
        .ok_or_else(|| Error::Refused("SETUP answered something that is not a plist".into()))
}

fn port_in(answer: &Dictionary, key: &str) -> Option<u16> {
    answer
        .get(key)
        .and_then(Value::as_unsigned_integer)
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port != 0)
}

pub(crate) fn session_setup(instance: &[u8], timing_port: u16) -> Value {
    dictionary(vec![
        ("deviceID", Value::String(colon_hex(instance))),
        ("sessionUUID", Value::String(uuid())),
        ("timingPort", integer(timing_port.into())),
        ("timingProtocol", Value::String("NTP".into())),
    ])
}

pub(crate) fn stream_setup(
    session_id: u32,
    key: &[u8; 32],
    data_port: u16,
    control_port: u16,
) -> Value {
    let stream = dictionary(vec![
        ("audioFormat", integer(ALAC_44100_16_2)),
        ("audioMode", Value::String("default".into())),
        ("controlPort", integer(control_port.into())),
        ("ct", integer(COMPRESSION_ALAC)),
        ("dataPort", integer(data_port.into())),
        ("isMedia", Value::Boolean(true)),
        ("latencyMax", integer(LATENCY_FRAMES.into())),
        ("latencyMin", integer(LATENCY_MIN)),
        ("shk", Value::Data(key.to_vec())),
        ("spf", integer(FRAMES_PER_PACKET as u64)),
        ("sr", integer(SAMPLE_RATE.into())),
        ("streamConnectionID", integer(session_id.into())),
        ("supportsDynamicStreamID", Value::Boolean(false)),
        ("type", integer(STREAM_REALTIME)),
    ]);
    dictionary(vec![("streams", Value::Array(vec![stream]))])
}

pub(crate) fn airplay2(device: &Device, volume: f32) -> Result<Link, Error> {
    let session_id = session_id();
    let instance = instance();
    let mut rtsp = Rtsp::connect(
        device.address,
        session_id,
        hex(&instance),
        AIRPLAY2_USER_AGENT,
    )?;
    let local = rtsp.local_ip()?;
    let peer = rtsp.peer_ip()?;
    rtsp.request("GET", Some("/info"), &[], None)?;
    let keys = pairing::transient(&mut rtsp)?;
    rtsp.encrypt(&keys.write, &keys.read);
    let control = bind_near(local)?;
    let timing = TimingServer::start(local)?;
    let audio = bind_near(local)?;

    rtsp.set_timeout(SETUP_TIMEOUT);
    let session = setup(&mut rtsp, &session_setup(&instance, timing.port))?;
    let events = port_in(&session, "eventPort")
        .and_then(|events_port| {
            TcpStream::connect_timeout(&SocketAddr::new(peer, events_port), EVENTS_TIMEOUT)
                .inspect_err(|e| log::info!("AirPlay: {} has no event channel: {e}", device.name))
                .ok()
        })
        .map(|stream| EventChannel {
            stream,
            read: FrameCipher::new(&keys.events_read),
            write: FrameCipher::new(&keys.events_write),
        });
    let record = rtsp.request("RECORD", None, &[], None)?;
    let stream = setup(
        &mut rtsp,
        &stream_setup(session_id, &keys.audio, port(&audio)?, port(&control)?),
    )?;
    let described = stream
        .get("streams")
        .and_then(Value::as_array)
        .and_then(|streams| streams.first())
        .and_then(Value::as_dictionary)
        .ok_or_else(|| Error::Refused("SETUP did not describe the stream".into()))?;
    let data_port = port_in(described, "dataPort")
        .ok_or_else(|| Error::Refused("SETUP did not name an audio port".into()))?;
    let remote_control = port_in(described, "controlPort")
        .ok_or_else(|| Error::Refused("SETUP did not name a control port".into()))?;
    send_volume(&mut rtsp, volume)?;
    rtsp.reset_timeout();
    audio
        .connect(SocketAddr::new(peer, data_port))
        .map_err(|e| Error::Io(e.to_string()))?;
    Ok(Link {
        active_remote: session_id,
        rtsp,
        audio,
        control,
        timing,
        control_target: SocketAddr::new(peer, remote_control),
        ssrc: session_id,
        base_seq: u16::from_be_bytes(random::<2>()),
        base_ts: u32::from_be_bytes(random::<4>()),
        device_latency: LATENCY_MIN as u32 + audio_latency(&record),
        cipher: Some(AudioCipher::new(&keys.audio)),
        events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(value: &Value) -> Dictionary {
        let mut bytes = Vec::new();
        value.to_writer_binary(&mut bytes).unwrap();
        assert!(bytes.starts_with(b"bplist00"));
        Value::from_reader(Cursor::new(bytes))
            .unwrap()
            .into_dictionary()
            .unwrap()
    }

    #[test]
    fn the_sender_id_never_starts_with_a_zero() {
        let id = dacp_id();
        assert_eq!(id.len(), 16);
        assert!(!id.starts_with('0'), "{id}");
    }

    #[test]
    fn apple_speakers_get_the_user_agent_they_check_and_others_get_itunes() {
        assert_eq!(
            raop_user_agent(Some("AudioAccessory5,1")),
            "AirPlay/999.0.0"
        );
        assert_eq!(raop_user_agent(Some("AppleTV11,1")), "AirPlay/999.0.0");
        assert_eq!(
            raop_user_agent(Some("ShairportSync")),
            "iTunes/7.6.2 (Windows; N;)"
        );
        assert_eq!(raop_user_agent(None), "iTunes/7.6.2 (Windows; N;)");
    }

    #[test]
    fn the_session_setup_asks_for_ntp_timing_on_our_port() {
        let session = decode(&session_setup(&[0x11, 0x22, 0, 0, 0, 0, 0, 0xab], 6001));
        assert_eq!(
            session.get("deviceID").and_then(Value::as_string),
            Some("11:22:00:00:00:00:00:AB")
        );
        assert_eq!(
            session.get("timingProtocol").and_then(Value::as_string),
            Some("NTP")
        );
        assert_eq!(
            session
                .get("timingPort")
                .and_then(Value::as_unsigned_integer),
            Some(6001)
        );
        let uuid = session
            .get("sessionUUID")
            .and_then(Value::as_string)
            .unwrap();
        assert_eq!(uuid.len(), 36);
        assert_eq!(uuid.matches('-').count(), 4);
    }

    #[test]
    fn the_stream_setup_describes_a_realtime_alac_stream_with_its_key() {
        let setup = decode(&stream_setup(77, &[5; 32], 6002, 6003));
        let stream = setup
            .get("streams")
            .and_then(Value::as_array)
            .and_then(|streams| streams.first())
            .and_then(Value::as_dictionary)
            .unwrap();
        let number = |key: &str| stream.get(key).and_then(Value::as_unsigned_integer);
        assert_eq!(number("type"), Some(96));
        assert_eq!(number("ct"), Some(2));
        assert_eq!(number("audioFormat"), Some(0x40000));
        assert_eq!(number("spf"), Some(352));
        assert_eq!(number("sr"), Some(44_100));
        assert_eq!(number("dataPort"), Some(6002));
        assert_eq!(number("controlPort"), Some(6003));
        assert_eq!(number("streamConnectionID"), Some(77));
        assert_eq!(number("latencyMax"), Some(88_200));
        assert_eq!(
            stream.get("shk").and_then(Value::as_data),
            Some(&[5u8; 32][..])
        );
    }

    #[test]
    fn ports_are_read_by_name_and_zero_is_no_port() {
        let answer: Dictionary = [
            ("controlPort".to_string(), integer(7001)),
            ("dataPort".to_string(), integer(7002)),
            ("eventPort".to_string(), integer(0)),
        ]
        .into_iter()
        .collect();
        assert_eq!(port_in(&answer, "dataPort"), Some(7002));
        assert_eq!(port_in(&answer, "controlPort"), Some(7001));
        assert_eq!(port_in(&answer, "eventPort"), None);
        assert_eq!(port_in(&answer, "timingPort"), None);
    }
}
