use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use audio_decoder::Codec;
use chromecast::testing::FakeChromecast;
use rstest::rstest;

use crate::media::{Accepts, Delivery, Probe, plan, probe};
use crate::pcm::{Container, PcmReader, PcmSpec};
use crate::server::{Body, Entry};
use crate::*;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../fixtures")
        .join(name)
}

fn request(server: &MediaServer, raw: &str) -> (String, Vec<u8>) {
    let mut stream = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    stream.write_all(raw.as_bytes()).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    (
        String::from_utf8_lossy(&response[..split]).into_owned(),
        response[split + 4..].to_vec(),
    )
}

fn get(server: &MediaServer, path: &str, extra: &str) -> (String, Vec<u8>) {
    request(
        server,
        &format!("GET {path} HTTP/1.1\r\nHost: test\r\n{extra}Connection: close\r\n\r\n"),
    )
}

#[test]
fn files_are_served_whole_and_by_range() {
    let path = fixture("tagged_basic.flac");
    let bytes = std::fs::read(&path).unwrap();
    let server = MediaServer::start().unwrap();
    let published = server.publish(
        Entry {
            body: Body::File(path),
            mime: "audio/flac".into(),
        },
        "flac",
    );

    let (head, body) = get(&server, &published, "getcontentFeatures.dlna.org: 1\r\n");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(head.contains("Content-Type: audio/flac"));
    assert!(head.contains("Accept-Ranges: bytes"));
    assert!(head.contains("contentFeatures.dlna.org: DLNA.ORG_OP=01;DLNA.ORG_CI=0"));
    assert_eq!(body, bytes);

    let (head, body) = get(&server, &published, "Range: bytes=10-19\r\n");
    assert!(head.starts_with("HTTP/1.1 206"), "{head}");
    assert!(head.contains(&format!("Content-Range: bytes 10-19/{}", bytes.len())));
    assert_eq!(body, bytes[10..20]);

    let (head, body) = request(
        &server,
        &format!("HEAD {published} HTTP/1.1\r\nConnection: close\r\n\r\n"),
    );
    assert!(head.contains(&format!("Content-Length: {}", bytes.len())));
    assert!(body.is_empty());

    let (head, _) = get(
        &server,
        &published,
        &format!("Range: bytes={}-\r\n", bytes.len()),
    );
    assert!(head.starts_with("HTTP/1.1 416"), "{head}");

    let (head, _) = get(&server, "/0123/1.flac", "");
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");
}

#[test]
fn a_connection_is_kept_alive_between_requests() {
    let bytes = Arc::new((0u8..=255).collect::<Vec<u8>>());
    let server = MediaServer::start().unwrap();
    let published = server.publish(
        Entry {
            body: Body::Bytes(bytes.clone()),
            mime: "audio/wav".into(),
        },
        "wav",
    );
    let (head, body) = request(
        &server,
        &format!(
            "GET {published} HTTP/1.1\r\nRange: bytes=0-1\r\n\r\nGET {published} HTTP/1.1\r\nRange: bytes=254-\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(head.starts_with("HTTP/1.1 206"));
    let second = body
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    assert_eq!(&body[..2], &[0, 1]);
    assert_eq!(&body[second + 4..], &[254, 255]);
}

struct PcmOnly;

impl Accepts for PcmOnly {
    fn original(&self, _: Codec, _: &str, _: &Probe) -> Option<String> {
        None
    }

    fn pcm(&self) -> Container {
        Container::Wav
    }

    fn pcm_limits(&self) -> (u32, u16) {
        (u32::MAX, 8)
    }
}

struct SmallDevice;

impl Accepts for SmallDevice {
    fn original(&self, _: Codec, _: &str, _: &Probe) -> Option<String> {
        None
    }

    fn pcm(&self) -> Container {
        Container::Wav
    }

    fn pcm_limits(&self) -> (u32, u16) {
        (24_000, 1)
    }
}

fn pcm_spec(name: &str, start: Duration, length: Option<Duration>) -> PcmSpec {
    pcm_spec_for(name, start, length, &PcmOnly)
}

fn pcm_spec_for(
    name: &str,
    start: Duration,
    length: Option<Duration>,
    accepts: &dyn Accepts,
) -> PcmSpec {
    let path = fixture(name);
    let extension = name.rsplit('.').next().unwrap().to_string();
    let source = Source::File(path);
    let probed = probe(&source, &extension).unwrap();
    let media = Media {
        source,
        extension,
        start,
        length,
        info: TrackInfo::default(),
        cover: None,
    };
    match plan(&media, &probed, accepts).unwrap() {
        Delivery::Pcm(spec) => spec,
        Delivery::Original { .. } => panic!("expected PCM"),
    }
}

fn read_from(spec: &PcmSpec, offset: u64) -> Vec<u8> {
    let mut out = Vec::new();
    PcmReader::open(spec.clone(), offset)
        .unwrap()
        .read_to_end(&mut out)
        .unwrap();
    out
}

#[rstest]
#[case::wav("sine_440_16_44_stereo.wav")]
#[case::flac("tagged_basic.flac")]
#[case::flac_24("sine_440_24_44_mono.wav")]
fn pcm_ranges_match_a_straight_decode(#[case] name: &str) {
    let spec = pcm_spec(name, Duration::ZERO, None);
    let full = read_from(&spec, 0);
    assert_eq!(full.len() as u64, spec.len());
    assert_eq!(&full[..4], b"RIFF");
    let header = spec.header_len();
    let align = spec.block_align();
    for offset in [
        header,
        header + align * 1000,
        header + align * 1000 + 1,
        header + align * 7000 + align - 1,
        spec.len() - 100,
        10,
    ] {
        let tail = read_from(&spec, offset);
        assert_eq!(
            tail,
            full[offset as usize..],
            "{name}: reading from {offset} differs"
        );
    }
}

#[test]
fn a_segment_is_cut_from_the_whole_decode() {
    let whole = pcm_spec("sine_440_16_44_stereo.wav", Duration::ZERO, None);
    let full = read_from(&whole, 0);
    let segment = pcm_spec(
        "sine_440_16_44_stereo.wav",
        Duration::from_millis(100),
        Some(Duration::from_millis(200)),
    );
    assert_eq!(segment.frames, 8_820);
    let cut = read_from(&segment, 0);
    let align = whole.block_align() as usize;
    let from = whole.header_len() as usize + 4_410 * align;
    assert_eq!(
        &cut[segment.header_len() as usize..],
        &full[from..from + 8_820 * align]
    );
}

#[test]
fn a_reduced_stream_has_the_promised_length_and_the_sine() {
    let spec = pcm_spec_for(
        "sine_440_16_48_mono.wav",
        Duration::ZERO,
        None,
        &SmallDevice,
    );
    assert_eq!((spec.source_rate, spec.sample_rate), (48_000, 24_000));
    let full = read_from(&spec, 0);
    assert_eq!(full.len() as u64, spec.len());
    let samples: Vec<i16> = full[spec.header_len() as usize..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair))
        .collect();
    let body = &samples[2_400..samples.len() - 2_400];
    let crossings = body
        .windows(2)
        .filter(|pair| (pair[0] < 0) != (pair[1] < 0))
        .count();
    let seconds = body.len() as f64 / 24_000.0;
    let frequency = crossings as f64 / 2.0 / seconds;
    assert!((frequency - 440.0).abs() < 10.0, "{frequency} Hz");
    let tail = read_from(&spec, spec.header_len() + 2 * 6_000);
    assert_eq!(tail.len() as u64, spec.len() - spec.header_len() - 12_000);
}

fn expect_event(
    events: &flume::Receiver<SessionEvent>,
    seen: &mut Vec<SessionEvent>,
    wanted: impl Fn(&SessionEvent) -> bool,
) {
    if seen.iter().any(&wanted) {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        let Ok(event) = events.recv_timeout(left) else {
            break;
        };
        let hit = wanted(&event);
        seen.push(event);
        if hit {
            return;
        }
    }
    panic!("the expected event never came; got {seen:?}");
}

fn chromecast_session() -> (FakeChromecast, Session, flume::Receiver<SessionEvent>) {
    let fake = FakeChromecast::start();
    let server = MediaServer::start().unwrap();
    let receiver = Receiver::from_chromecast(&fake.device());
    let session = connect(&receiver, server).unwrap();
    let events = session.events();
    (fake, session, events)
}

#[test]
fn a_chromecast_plays_a_file_through_the_media_server() {
    let (fake, session, events) = chromecast_session();
    let mut seen = Vec::new();
    expect_event(&events, &mut seen, |e| matches!(e, SessionEvent::Volume(_)));
    let path = fixture("tagged_basic.flac");
    session.load(Load {
        media: Media {
            source: Source::File(path.clone()),
            extension: "flac".into(),
            start: Duration::ZERO,
            length: None,
            info: TrackInfo {
                title: "Basic".into(),
                artist: Some("Tester".into()),
                album: None,
            },
            cover: Some(Cover {
                bytes: Arc::new(vec![0xff, 0xd8, 0xff, 0xe0]),
                mime: "image/jpeg".into(),
            }),
        },
        start: Duration::ZERO,
        autoplay: true,
    });
    expect_event(&events, &mut seen, |e| {
        matches!(e, SessionEvent::Loaded { .. })
    });
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Playing);
    let state = fake.state();
    let loaded = state.loaded.unwrap();
    assert_eq!(loaded.content_type, "audio/flac");
    assert_eq!(loaded.title.as_deref(), Some("Basic"));
    assert!(loaded.image.unwrap().ends_with(".jpg"));
    assert_eq!(state.fetched.unwrap(), std::fs::read(&path).unwrap());

    session.pause();
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Paused);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(fake.state().player, "PAUSED");
    session.play();
    session.seek(Duration::from_secs(10));
    std::thread::sleep(Duration::from_millis(300));
    assert!(fake.state().position >= 10.0);

    std::thread::sleep(Duration::from_millis(1600));
    fake.finish_track();
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Ended);

    fake.disconnect_everyone();
    expect_event(&events, &mut seen, |e| matches!(e, SessionEvent::Lost(_)));
}

#[test]
fn a_cue_segment_reaches_a_chromecast_as_wav() {
    let (fake, session, events) = chromecast_session();
    let mut seen = Vec::new();
    session.load(Load {
        media: Media {
            source: Source::File(fixture("sine_440_16_44_stereo.wav")),
            extension: "wav".into(),
            start: Duration::from_millis(100),
            length: Some(Duration::from_millis(200)),
            info: TrackInfo::default(),
            cover: None,
        },
        start: Duration::ZERO,
        autoplay: false,
    });
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Paused);
    let state = fake.state();
    assert_eq!(state.loaded.unwrap().content_type, "audio/wav");
    let fetched = state.fetched.unwrap();
    assert_eq!(&fetched[..4], b"RIFF");
    assert_eq!(fetched.len(), 44 + 8_820 * 4);
    session.close(Duration::from_secs(2));
    assert!(fake.state().commands.iter().any(|c| c == "STOP"));
}

struct Device {
    calls: Vec<&'static str>,
    state: crate::session::State,
    base: Duration,
    since: Option<Instant>,
    queued: std::collections::VecDeque<(crate::session::State, Duration)>,
    fail_seek: bool,
}

impl Device {
    fn position(&self) -> Duration {
        let exact = self.base + self.since.map_or(Duration::ZERO, |since| since.elapsed());
        Duration::from_secs(exact.as_secs())
    }

    fn run_from(&mut self, position: Duration) {
        self.base = position;
        self.since = Some(Instant::now());
    }

    fn freeze(&mut self) {
        self.base = self.position();
        self.since = None;
    }
}

#[derive(Clone)]
struct FakeRenderer(Arc<std::sync::Mutex<Device>>);

impl FakeRenderer {
    fn new() -> Self {
        Self(Arc::new(std::sync::Mutex::new(Device {
            calls: Vec::new(),
            state: crate::session::State::Idle,
            base: Duration::ZERO,
            since: None,
            queued: Default::default(),
            fail_seek: false,
        })))
    }

    fn device(&self) -> std::sync::MutexGuard<'_, Device> {
        self.0.lock().unwrap()
    }
}

impl Accepts for FakeRenderer {
    fn original(&self, _: Codec, _: &str, _: &Probe) -> Option<String> {
        Some("audio/wav".into())
    }

    fn pcm(&self) -> Container {
        Container::Wav
    }

    fn pcm_limits(&self) -> (u32, u16) {
        (192_000, 8)
    }
}

impl crate::session::Driver for FakeRenderer {
    fn peer(&self) -> std::net::IpAddr {
        std::net::Ipv4Addr::LOCALHOST.into()
    }

    fn load(&mut self, loading: &crate::session::Loading) -> Result<(), String> {
        let mut device = self.device();
        device.calls.push(if loading.autoplay {
            "load and play"
        } else {
            "load"
        });
        device.base = Duration::ZERO;
        device.since = loading.autoplay.then(Instant::now);
        device.state = if loading.autoplay {
            crate::session::State::Playing
        } else {
            crate::session::State::Idle
        };
        Ok(())
    }

    fn play(&mut self) -> Result<(), String> {
        let mut device = self.device();
        device.calls.push("play");
        device.state = crate::session::State::Playing;
        let position = device.position();
        device.run_from(position);
        Ok(())
    }

    fn pause(&mut self) -> Result<(), String> {
        let mut device = self.device();
        device.calls.push("pause");
        device.freeze();
        device.state = crate::session::State::Paused;
        Ok(())
    }

    fn seek(&mut self, position: Duration) -> Result<(), String> {
        let mut device = self.device();
        device.calls.push("seek");
        if device.fail_seek {
            return Err("seek refused".into());
        }
        device.base = position;
        if device.since.is_some() {
            device.since = Some(Instant::now());
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<(), String> {
        let mut device = self.device();
        device.calls.push("stop");
        device.state = crate::session::State::Idle;
        device.since = None;
        Ok(())
    }

    fn set_volume(&mut self, _: f32) -> Result<(), String> {
        Ok(())
    }

    fn status(&mut self) -> Result<crate::session::Status, String> {
        let mut device = self.device();
        let (state, position) = match device.queued.pop_front() {
            Some(scripted) => scripted,
            None => (device.state, device.position()),
        };
        Ok(crate::session::Status {
            state: Some(state),
            position: Some(position),
            ..Default::default()
        })
    }

    fn lost(&mut self) -> Option<String> {
        None
    }

    fn volume(&mut self) -> Option<f32> {
        None
    }

    fn seeks_on_load(&self) -> bool {
        false
    }

    fn silence_start(&mut self) {
        self.device().calls.push("silence");
    }

    fn restore_sound(&mut self) {
        self.device().calls.push("restore");
    }

    fn close(&mut self) {}
}

fn renderer_session() -> (FakeRenderer, Session, flume::Receiver<SessionEvent>) {
    let fake = FakeRenderer::new();
    let session = Session::start(Box::new(fake.clone()), MediaServer::start().unwrap());
    let events = session.events();
    (fake, session, events)
}

fn minute_long(start: Duration, autoplay: bool) -> Load {
    Load {
        media: Media {
            source: Source::File(fixture("sine_440_16_44_stereo.wav")),
            extension: "wav".into(),
            start: Duration::ZERO,
            length: Some(Duration::from_secs(60)),
            info: TrackInfo::default(),
            cover: None,
        },
        start,
        autoplay,
    }
}

#[test]
fn a_renderer_started_mid_track_is_silent_from_before_it_gets_the_track() {
    let (fake, session, events) = renderer_session();
    let mut seen = Vec::new();
    session.load(minute_long(Duration::from_secs(30), true));
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Playing);
    assert_eq!(
        fake.device().calls,
        ["silence", "load", "play", "seek", "restore"]
    );
}

#[test]
fn a_renderer_loaded_paused_gets_nothing_until_play() {
    let (fake, session, events) = renderer_session();
    fake.device().state = crate::session::State::Playing;
    fake.device().run_from(Duration::from_secs(100));
    let mut seen = Vec::new();
    session.load(minute_long(Duration::from_secs(30), false));
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Paused);
    std::thread::sleep(Duration::from_millis(2500));
    while let Ok(event) = events.try_recv() {
        seen.push(event);
    }
    assert!(fake.device().calls.is_empty());
    assert!(!seen.contains(&SessionEvent::Playing), "{seen:?}");
    assert_eq!(
        seen.iter().rev().find_map(|e| match e {
            SessionEvent::Position(position) => Some(*position),
            _ => None,
        }),
        Some(Duration::from_secs(30))
    );

    session.play();
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Playing);
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(
        fake.device().calls,
        ["silence", "load", "play", "seek", "restore"]
    );
    assert!(fake.device().position() >= Duration::from_secs(30));
    assert!(fake.device().position() < Duration::from_secs(40));
}

#[test]
fn a_renderer_paused_on_its_own_buttons_shows_as_paused_while_it_still_says_playing() {
    let (fake, session, events) = renderer_session();
    let mut seen = Vec::new();
    session.load(minute_long(Duration::ZERO, true));
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Playing);
    std::thread::sleep(Duration::from_millis(1500));
    fake.device().freeze();
    let frozen = fake.device().position();
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Paused);
    assert_eq!(
        seen.iter().rev().find_map(|e| match e {
            SessionEvent::Position(position) => Some(*position),
            _ => None,
        }),
        Some(frozen)
    );
    assert_eq!(fake.device().state, crate::session::State::Playing);

    seen.clear();
    fake.device().run_from(frozen);
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Playing);

    fake.device().freeze();
    seen.clear();
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Paused);
    fake.device().calls.clear();
    session.play();
    seen.clear();
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Playing);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(fake.device().calls, ["pause", "play"]);
    drop(session);
}

#[test]
fn a_renderer_loaded_paused_after_playing_is_stopped_first() {
    let (fake, session, events) = renderer_session();
    let mut seen = Vec::new();
    session.load(minute_long(Duration::ZERO, true));
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Playing);
    seen.clear();
    session.load(minute_long(Duration::from_secs(10), false));
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Paused);
    assert_eq!(fake.device().calls, ["load and play", "stop"]);
}

#[test]
fn a_renderer_that_fails_the_start_is_stopped_before_the_sound_returns() {
    let (fake, session, events) = renderer_session();
    fake.device().fail_seek = true;
    let mut seen = Vec::new();
    session.load(minute_long(Duration::from_secs(30), true));
    expect_event(&events, &mut seen, |e| matches!(e, SessionEvent::Failed(_)));
    assert_eq!(
        fake.device().calls,
        ["silence", "load", "play", "seek", "stop", "restore"]
    );
}

#[test]
fn a_renderer_that_reports_zero_just_before_it_stops_still_ends_the_track() {
    let (fake, session, events) = renderer_session();
    let mut seen = Vec::new();
    session.load(minute_long(Duration::ZERO, true));
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Playing);
    fake.device().run_from(Duration::from_secs(57));
    std::thread::sleep(Duration::from_millis(2500));
    {
        let mut device = fake.device();
        device
            .queued
            .push_back((crate::session::State::Playing, Duration::ZERO));
        device.state = crate::session::State::Idle;
        device.base = Duration::ZERO;
        device.since = None;
    }
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Ended);
    let after_start = seen
        .iter()
        .skip_while(|e| **e != SessionEvent::Playing)
        .collect::<Vec<_>>();
    assert!(!after_start.contains(&&SessionEvent::Paused), "{seen:?}");
    assert!(
        !after_start.contains(&&SessionEvent::Position(Duration::ZERO)),
        "{seen:?}"
    );
    drop(session);
}

#[test]
fn a_renderer_that_buffers_long_at_the_start_is_not_taken_for_paused() {
    let (fake, session, events) = renderer_session();
    let mut seen = Vec::new();
    {
        let mut device = fake.device();
        for _ in 0..6 {
            device
                .queued
                .push_back((crate::session::State::Buffering, Duration::ZERO));
        }
    }
    session.load(minute_long(Duration::ZERO, true));
    expect_event(&events, &mut seen, |e| *e == SessionEvent::Playing);
    fake.device().since = None;
    let deadline = Instant::now() + Duration::from_secs(8);
    while !fake.device().queued.is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    fake.device().run_from(Duration::ZERO);
    std::thread::sleep(Duration::from_millis(3000));
    while let Ok(event) = events.try_recv() {
        seen.push(event);
    }
    assert!(!seen.contains(&SessionEvent::Paused), "{seen:?}");
    drop(session);
}
