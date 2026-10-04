use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce, Tag};
use plist::{Dictionary, Value};

use crate::secure::{FrameCipher, TAG_LEN, derive};
use crate::{srp, tlv};

const POLL: Duration = Duration::from_millis(50);

#[derive(Default)]
pub(crate) struct Heard {
    pub requests: Vec<String>,
    pub parameters: Vec<String>,
    pub metadata: Vec<(String, bool)>,
    pub payloads: Vec<(u16, Vec<u8>)>,
    pub syncs: usize,
    pub events_connected: bool,
}

struct EventSide {
    stream: TcpStream,
    write: FrameCipher,
    read: FrameCipher,
}

pub(crate) struct FakeReceiver {
    pub address: SocketAddr,
    heard: Arc<Mutex<Heard>>,
    events: Arc<Mutex<Option<EventSide>>>,
    stop: Arc<AtomicBool>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl FakeReceiver {
    pub fn start(password: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let heard = Arc::new(Mutex::new(Heard::default()));
        let events = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let fake = Self {
            address,
            heard: heard.clone(),
            events: events.clone(),
            stop: stop.clone(),
        };
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                serve(stream, password, heard, events, stop);
            }
        });
        fake
    }

    pub fn press(&self, value: &str) -> Option<String> {
        let mut events = lock(&self.events);
        let side = events.as_mut()?;
        let body = plist_bytes(Value::Dictionary(
            [("type", "sendMediaRemoteCommand"), ("value", value)]
                .into_iter()
                .map(|(key, text)| (key.to_string(), Value::String(text.into())))
                .collect(),
        ));
        let mut request = format!(
            "POST /command RTSP/1.0\r\nCSeq: 7\r\nContent-Type: application/x-apple-binary-plist\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        request.extend_from_slice(&body);
        let sealed = side.write.seal(&request);
        side.stream.write_all(&sealed).ok()?;
        side.stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .ok()?;
        let mut length = [0u8; 2];
        side.stream.read_exact(&mut length).ok()?;
        let mut answer = vec![0u8; usize::from(u16::from_le_bytes(length)) + TAG_LEN];
        side.stream.read_exact(&mut answer).ok()?;
        let plain = side.read.open(length, &answer)?;
        let text = String::from_utf8_lossy(&plain).into_owned();
        text.lines().next().map(str::to_string)
    }

    pub fn heard(&self) -> MutexGuard<'_, Heard> {
        lock(&self.heard)
    }

    pub fn wait_for(&self, what: impl Fn(&Heard) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if what(&self.heard()) {
                return true;
            }
            std::thread::sleep(POLL);
        }
        false
    }
}

impl Drop for FakeReceiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

struct Request {
    method: String,
    target: String,
    cseq: String,
    kind: String,
    rtp_info: bool,
    body: Vec<u8>,
}

struct Connection {
    stream: TcpStream,
    inbox: Vec<u8>,
    read: Option<FrameCipher>,
    write: Option<FrameCipher>,
}

impl Connection {
    fn next_request(&mut self) -> Option<Request> {
        loop {
            if let Some(end) = self.inbox.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&self.inbox[..end]).into_owned();
                let mut lines = head.split("\r\n");
                let mut first = lines.next()?.split_whitespace();
                let method = first.next()?.to_string();
                let target = first.next()?.to_string();
                let headers: HashMap<String, String> = lines
                    .filter_map(|line| line.split_once(':'))
                    .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                    .collect();
                let length: usize = headers
                    .get("content-length")
                    .and_then(|l| l.parse().ok())
                    .unwrap_or(0);
                if self.inbox.len() >= end + 4 + length {
                    let body = self.inbox[end + 4..end + 4 + length].to_vec();
                    self.inbox.drain(..end + 4 + length);
                    return Some(Request {
                        method,
                        target,
                        cseq: headers.get("cseq").cloned().unwrap_or_default(),
                        kind: headers.get("content-type").cloned().unwrap_or_default(),
                        rtp_info: headers.contains_key("rtp-info"),
                        body,
                    });
                }
            }
            match &mut self.read {
                None => {
                    let mut buffer = [0u8; 4096];
                    let read = self.stream.read(&mut buffer).ok()?;
                    if read == 0 {
                        return None;
                    }
                    self.inbox.extend_from_slice(&buffer[..read]);
                }
                Some(cipher) => {
                    let mut length = [0u8; 2];
                    self.stream.read_exact(&mut length).ok()?;
                    let mut sealed = vec![0u8; usize::from(u16::from_le_bytes(length)) + TAG_LEN];
                    self.stream.read_exact(&mut sealed).ok()?;
                    let plain = cipher.open(length, &sealed)?;
                    self.inbox.extend_from_slice(&plain);
                }
            }
        }
    }

    fn reply(&mut self, request: &Request, status: u16, body: Option<(&str, Vec<u8>)>) {
        let mut head = format!("RTSP/1.0 {status} OK\r\nCSeq: {}\r\n", request.cseq);
        let body = match body {
            Some((kind, bytes)) => {
                head.push_str(&format!(
                    "Content-Type: {kind}\r\nContent-Length: {}\r\n",
                    bytes.len()
                ));
                bytes
            }
            None => Vec::new(),
        };
        head.push_str("\r\n");
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(&body);
        if let Some(cipher) = &mut self.write {
            bytes = cipher.seal(&bytes);
        }
        let _ = self.stream.write_all(&bytes);
    }
}

fn plist_bytes(value: Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    value.to_writer_binary(&mut bytes).unwrap();
    bytes
}

fn port(port: u16) -> Value {
    Value::Integer(u64::from(port).into())
}

fn serve(
    stream: TcpStream,
    password: &str,
    heard: Arc<Mutex<Heard>>,
    event_side: Arc<Mutex<Option<EventSide>>>,
    stop: Arc<AtomicBool>,
) {
    let mut connection = Connection {
        stream,
        inbox: Vec::new(),
        read: None,
        write: None,
    };
    let srp = srp::Server::new("Pair-Setup", password, &[7; 16], &[9; 32]);
    let mut timing_port = None;
    let mut session_key = None;
    while let Some(request) = connection.next_request() {
        lock(&heard)
            .requests
            .push(format!("{} {}", request.method, request.target));
        match request.method.as_str() {
            "GET" => {
                let info = Value::Dictionary(
                    [("name".to_string(), Value::String("Fake".into()))]
                        .into_iter()
                        .collect(),
                );
                connection.reply(
                    &request,
                    200,
                    Some(("application/x-apple-binary-plist", plist_bytes(info))),
                );
            }
            "POST" if request.target == "/pair-setup" => {
                let items = tlv::decode(&request.body).unwrap();
                if items.get(tlv::STATE) == Some(&[1]) {
                    let answer = tlv::encode(&[
                        (tlv::STATE, &[2]),
                        (tlv::SALT, &[7; 16]),
                        (tlv::PUBLIC_KEY, &srp.public()),
                    ]);
                    connection.reply(&request, 200, Some(("application/octet-stream", answer)));
                    continue;
                }
                let verified = srp.verify(
                    "Pair-Setup",
                    items.get(tlv::PUBLIC_KEY).unwrap_or_default(),
                    items.get(tlv::PROOF).unwrap_or_default(),
                );
                let Some((key, proof)) = verified else {
                    let answer = tlv::encode(&[(tlv::STATE, &[4]), (tlv::ERROR, &[2])]);
                    connection.reply(&request, 200, Some(("application/octet-stream", answer)));
                    continue;
                };
                session_key = Some(key);
                let answer = tlv::encode(&[(tlv::STATE, &[4]), (tlv::PROOF, &proof)]);
                connection.reply(&request, 200, Some(("application/octet-stream", answer)));
                connection.read = Some(FrameCipher::new(&derive(
                    &key,
                    "Control-Salt",
                    "Control-Write-Encryption-Key",
                )));
                connection.write = Some(FrameCipher::new(&derive(
                    &key,
                    "Control-Salt",
                    "Control-Read-Encryption-Key",
                )));
            }
            "SETUP" => {
                let setup = Value::from_reader(Cursor::new(&request.body))
                    .unwrap()
                    .into_dictionary()
                    .unwrap();
                let answer = match setup.get("streams") {
                    Some(streams) => {
                        if !clock_answers(timing_port) {
                            return;
                        }
                        let stream = streams.as_array().unwrap()[0].as_dictionary().unwrap();
                        let key: [u8; 32] = stream
                            .get("shk")
                            .and_then(Value::as_data)
                            .unwrap()
                            .try_into()
                            .unwrap();
                        let data = UdpSocket::bind("127.0.0.1:0").unwrap();
                        let control = UdpSocket::bind("127.0.0.1:0").unwrap();
                        let answer: Dictionary = [
                            (
                                "controlPort".to_string(),
                                port(control.local_addr().unwrap().port()),
                            ),
                            (
                                "dataPort".to_string(),
                                port(data.local_addr().unwrap().port()),
                            ),
                            ("type".to_string(), port(96)),
                        ]
                        .into_iter()
                        .collect();
                        let (receiving, stopping) = (heard.clone(), stop.clone());
                        std::thread::spawn(move || receive_audio(data, key, receiving, stopping));
                        let (syncing, stopping) = (heard.clone(), stop.clone());
                        std::thread::spawn(move || count_syncs(control, syncing, stopping));
                        Value::Dictionary(
                            [(
                                "streams".to_string(),
                                Value::Array(vec![Value::Dictionary(answer)]),
                            )]
                            .into_iter()
                            .collect(),
                        )
                    }
                    None => {
                        timing_port = setup
                            .get("timingPort")
                            .and_then(Value::as_unsigned_integer)
                            .and_then(|port| u16::try_from(port).ok());
                        let events = TcpListener::bind("127.0.0.1:0").unwrap();
                        let events_port = events.local_addr().unwrap().port();
                        let connected = heard.clone();
                        let side = event_side.clone();
                        let key = session_key.unwrap();
                        std::thread::spawn(move || {
                            if let Ok((stream, _)) = events.accept() {
                                *lock(&side) = Some(EventSide {
                                    stream,
                                    write: FrameCipher::new(&derive(
                                        &key,
                                        "Events-Salt",
                                        "Events-Write-Encryption-Key",
                                    )),
                                    read: FrameCipher::new(&derive(
                                        &key,
                                        "Events-Salt",
                                        "Events-Read-Encryption-Key",
                                    )),
                                });
                                lock(&connected).events_connected = true;
                            }
                        });
                        Value::Dictionary(
                            [("eventPort".to_string(), port(events_port))]
                                .into_iter()
                                .collect(),
                        )
                    }
                };
                connection.reply(
                    &request,
                    200,
                    Some(("application/x-apple-binary-plist", plist_bytes(answer))),
                );
            }
            "SET_PARAMETER" if request.kind == "text/parameters" => {
                lock(&heard)
                    .parameters
                    .push(String::from_utf8_lossy(&request.body).trim().to_string());
                connection.reply(&request, 200, None);
            }
            "SET_PARAMETER" => {
                lock(&heard)
                    .metadata
                    .push((request.kind.clone(), request.rtp_info));
                let status = if request.kind == "image/png" {
                    415
                } else {
                    200
                };
                connection.reply(&request, status, None);
            }
            "TEARDOWN" => {
                connection.reply(&request, 200, None);
                return;
            }
            _ => connection.reply(&request, 200, None),
        }
    }
}

fn clock_answers(timing_port: Option<u16>) -> bool {
    let Some(timing_port) = timing_port else {
        return false;
    };
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut request = [0u8; 32];
    request[..4].copy_from_slice(&[0x80, 0xd2, 0, 7]);
    socket
        .send_to(&request, ("127.0.0.1", timing_port))
        .unwrap();
    let mut reply = [0u8; 64];
    matches!(socket.recv(&mut reply), Ok(32) if reply[1] == 0xd3)
}

fn receive_audio(
    socket: UdpSocket,
    key: [u8; 32],
    heard: Arc<Mutex<Heard>>,
    stop: Arc<AtomicBool>,
) {
    socket.set_read_timeout(Some(POLL)).unwrap();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let mut buffer = [0u8; 4096];
    while !stop.load(Ordering::Acquire) {
        let Ok(len) = socket.recv(&mut buffer) else {
            continue;
        };
        let packet = &buffer[..len];
        let mut nonce = [0u8; 12];
        nonce[4..].copy_from_slice(&packet[len - 8..]);
        let tag_at = len - 8 - TAG_LEN;
        let mut payload = packet[12..tag_at].to_vec();
        let opened = cipher.decrypt_in_place_detached(
            Nonce::from_slice(&nonce),
            &packet[4..12],
            &mut payload,
            Tag::from_slice(&packet[tag_at..len - 8]),
        );
        if opened.is_ok() {
            let seq = u16::from_be_bytes([packet[2], packet[3]]);
            lock(&heard).payloads.push((seq, payload));
        }
    }
}

fn count_syncs(socket: UdpSocket, heard: Arc<Mutex<Heard>>, stop: Arc<AtomicBool>) {
    socket.set_read_timeout(Some(POLL)).unwrap();
    let mut buffer = [0u8; 128];
    while !stop.load(Ordering::Acquire) {
        if let Ok(len) = socket.recv(&mut buffer)
            && len == 20
            && buffer[1] == 0xd4
        {
            lock(&heard).syncs += 1;
        }
    }
}
