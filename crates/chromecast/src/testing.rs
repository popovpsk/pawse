use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde_json::{Value, json};

use crate::proto::CastMessage;
use crate::{DEFAULT_MEDIA_RECEIVER, Device, NS_CONNECTION, NS_HEARTBEAT};

const CERT: &[u8] = include_bytes!("../testdata/fake.crt");
const KEY: &[u8] = include_bytes!("../testdata/fake.key");
const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";

#[derive(Debug, Clone, Default)]
pub struct Loaded {
    pub url: String,
    pub content_type: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub image: Option<String>,
    pub autoplay: bool,
    pub start: f64,
}

#[derive(Debug, Clone)]
pub struct State {
    pub launched: bool,
    pub volume: f32,
    pub loaded: Option<Loaded>,
    pub fetched: Option<Vec<u8>>,
    pub player: &'static str,
    pub idle_reason: Option<&'static str>,
    pub position: f64,
    pub commands: Vec<String>,
    playing_since: Option<Instant>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            launched: false,
            volume: 0.4,
            loaded: None,
            fetched: None,
            player: "IDLE",
            idle_reason: None,
            position: 0.0,
            commands: Vec::new(),
            playing_since: None,
        }
    }
}

impl State {
    fn current_time(&self) -> f64 {
        self.position
            + self
                .playing_since
                .map_or(0.0, |since| since.elapsed().as_secs_f64())
    }

    fn settle(&mut self) {
        self.position = self.current_time();
        self.playing_since = None;
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct FakeChromecast {
    pub address: SocketAddr,
    state: Arc<Mutex<State>>,
    stopped: Arc<AtomicBool>,
    drop_connections: Arc<AtomicBool>,
}

impl FakeChromecast {
    pub fn start() -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind fake chromecast");
        let address = listener.local_addr().expect("fake chromecast address");
        let state = Arc::new(Mutex::new(State::default()));
        let stopped = Arc::new(AtomicBool::new(false));
        let drop_connections = Arc::new(AtomicBool::new(false));
        let config = Arc::new(
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("protocol versions")
                .with_no_client_auth()
                .with_single_cert(
                    vec![CertificateDer::from_pem_slice(CERT).expect("fake cert")],
                    PrivateKeyDer::from_pem_slice(KEY).expect("fake key"),
                )
                .expect("fake tls config"),
        );
        let accepting = (state.clone(), stopped.clone(), drop_connections.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if accepting.1.load(Ordering::Acquire) {
                    return;
                }
                let Ok(stream) = stream else { continue };
                let state = accepting.0.clone();
                let drop_now = accepting.2.clone();
                let config = config.clone();
                std::thread::spawn(move || {
                    let _ = serve(stream, config, state, drop_now);
                });
            }
        });
        Self {
            address,
            state,
            stopped,
            drop_connections,
        }
    }

    pub fn device(&self) -> Device {
        Device {
            id: "fake".into(),
            name: "Fake Chromecast".into(),
            model: Some("Chromecast".into()),
            address: self.address,
        }
    }

    pub fn state(&self) -> State {
        lock(&self.state).clone()
    }

    pub fn finish_track(&self) {
        let mut state = lock(&self.state);
        state.settle();
        state.player = "IDLE";
        state.idle_reason = Some("FINISHED");
    }

    pub fn disconnect_everyone(&self) {
        self.drop_connections.store(true, Ordering::Release);
    }
}

impl Drop for FakeChromecast {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        self.drop_connections.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.address);
    }
}

fn read_frame(stream: &mut impl Read) -> io::Result<CastMessage> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len)?;
    let mut body = vec![0u8; u32::from_be_bytes(len) as usize];
    stream.read_exact(&mut body)?;
    CastMessage::decode(&body).map_err(io::Error::other)
}

fn serve(
    socket: TcpStream,
    config: Arc<ServerConfig>,
    state: Arc<Mutex<State>>,
    drop_now: Arc<AtomicBool>,
) -> io::Result<()> {
    socket.set_read_timeout(Some(Duration::from_millis(50)))?;
    let connection = ServerConnection::new(config).map_err(io::Error::other)?;
    let mut tls = StreamOwned::new(connection, socket);
    loop {
        if drop_now.load(Ordering::Acquire) {
            return Ok(());
        }
        let message = match read_frame(&mut tls) {
            Ok(message) => message,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(e) => return Err(e),
        };
        let payload: Value = serde_json::from_str(&message.payload).unwrap_or(Value::Null);
        let replies = answer(&message, &payload, &state);
        for (namespace, reply) in replies {
            let frame = CastMessage {
                source: message.destination.clone(),
                destination: message.source.clone(),
                namespace,
                payload: reply.to_string(),
            }
            .encode();
            tls.write_all(&frame)?;
        }
        tls.flush()?;
    }
}

fn receiver_status(state: &State) -> Value {
    let applications = if state.launched {
        json!([{
            "appId": DEFAULT_MEDIA_RECEIVER,
            "displayName": "Default Media Receiver",
            "sessionId": "session-1",
            "transportId": "transport-1",
        }])
    } else {
        json!([])
    };
    json!({
        "type": "RECEIVER_STATUS",
        "status": {
            "applications": applications,
            "volume": {"level": state.volume, "muted": false},
        },
    })
}

fn media_status(state: &State) -> Value {
    if state.loaded.is_none() {
        return json!({"type": "MEDIA_STATUS", "status": []});
    }
    let mut status = json!({
        "mediaSessionId": 1,
        "playerState": state.player,
        "currentTime": state.current_time(),
        "media": {
            "contentId": state.loaded.as_ref().map(|l| l.url.clone()),
            "duration": 30.0,
        },
    });
    if let Some(reason) = state.idle_reason {
        status["idleReason"] = json!(reason);
    }
    json!({"type": "MEDIA_STATUS", "status": [status]})
}

fn with_request(mut reply: Value, payload: &Value) -> Value {
    reply["requestId"] = payload["requestId"].clone();
    reply
}

fn answer(message: &CastMessage, payload: &Value, state: &Mutex<State>) -> Vec<(String, Value)> {
    let kind = payload["type"].as_str().unwrap_or_default();
    let mut state = lock(state);
    match message.namespace.as_str() {
        NS_HEARTBEAT if kind == "PING" => vec![(NS_HEARTBEAT.into(), json!({"type": "PONG"}))],
        NS_CONNECTION => Vec::new(),
        NS_RECEIVER => {
            match kind {
                "LAUNCH" => state.launched = true,
                "STOP" => {
                    state.launched = false;
                    state.loaded = None;
                }
                "SET_VOLUME" => {
                    if let Some(level) = payload["volume"]["level"].as_f64() {
                        state.volume = level as f32;
                    }
                }
                _ => {}
            }
            state.commands.push(kind.to_string());
            vec![(
                NS_RECEIVER.into(),
                with_request(receiver_status(&state), payload),
            )]
        }
        NS_MEDIA => {
            state.commands.push(kind.to_string());
            match kind {
                "LOAD" => {
                    let media = &payload["media"];
                    let loaded = Loaded {
                        url: media["contentId"].as_str().unwrap_or_default().to_string(),
                        content_type: media["contentType"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        title: media["metadata"]["title"].as_str().map(str::to_string),
                        artist: media["metadata"]["artist"].as_str().map(str::to_string),
                        image: media["metadata"]["images"][0]["url"]
                            .as_str()
                            .map(str::to_string),
                        autoplay: payload["autoplay"].as_bool().unwrap_or(true),
                        start: payload["currentTime"].as_f64().unwrap_or(0.0),
                    };
                    state.fetched = fetch(&loaded.url).ok();
                    state.position = loaded.start;
                    state.idle_reason = None;
                    state.player = if loaded.autoplay { "PLAYING" } else { "PAUSED" };
                    state.playing_since = loaded.autoplay.then(Instant::now);
                    state.loaded = Some(loaded);
                }
                "PLAY" => {
                    if state.player != "PLAYING" {
                        state.player = "PLAYING";
                        state.playing_since = Some(Instant::now());
                    }
                }
                "PAUSE" => {
                    state.settle();
                    state.player = "PAUSED";
                }
                "SEEK" => {
                    let playing = state.playing_since.is_some();
                    state.position = payload["currentTime"].as_f64().unwrap_or(0.0);
                    state.playing_since = playing.then(Instant::now);
                }
                "STOP" => {
                    state.settle();
                    state.player = "IDLE";
                    state.idle_reason = Some("CANCELLED");
                }
                _ => {}
            }
            vec![(NS_MEDIA.into(), with_request(media_status(&state), payload))]
        }
        _ => Vec::new(),
    }
}

fn fetch(url: &str) -> io::Result<Vec<u8>> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::other("not http"))?;
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let mut stream = TcpStream::connect(authority)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n"
    )?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| io::Error::other("no header end"))?;
    let head = String::from_utf8_lossy(&response[..split]);
    if !head.starts_with("HTTP/1.1 200") {
        return Err(io::Error::other(
            head.lines().next().unwrap_or_default().to_string(),
        ));
    }
    Ok(response[split + 4..].to_vec())
}
