use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme};
use serde_json::{Value, json};

use crate::proto::{CastMessage, MAX_FRAME};
use crate::{Error, Event, NS_CONNECTION, NS_HEARTBEAT, RECEIVER, SENDER};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(20);
const PING_EVERY: Duration = Duration::from_secs(5);
const SILENCE_LIMIT: Duration = Duration::from_secs(20);

pub(crate) type Pending = Arc<Mutex<HashMap<u32, flume::Sender<Value>>>>;

pub(crate) struct Outgoing {
    pub destination: String,
    pub namespace: String,
    pub payload: String,
}

pub(crate) struct Connection {
    pub outgoing: flume::Sender<Outgoing>,
    pub pending: Pending,
    closed: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Debug)]
struct AnyCertificate(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

fn tls_config() -> Result<Arc<ClientConfig>, Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let algorithms = provider.signature_verification_algorithms;
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Io(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCertificate(algorithms)))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Connection {
    pub fn open(address: SocketAddr, events: flume::Sender<Event>) -> Result<Self, Error> {
        let socket = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
            .map_err(|e| Error::Io(format!("{address}: {e}")))?;
        socket.set_nodelay(true).ok();
        socket
            .set_read_timeout(Some(POLL))
            .map_err(|e| Error::Io(e.to_string()))?;
        let name = ServerName::IpAddress(address.ip().into());
        let tls =
            ClientConnection::new(tls_config()?, name).map_err(|e| Error::Io(e.to_string()))?;
        let (outgoing, queue) = flume::unbounded();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let mut worker = Worker {
            socket,
            tls,
            queue,
            pending: pending.clone(),
            events,
            closed: closed.clone(),
            inbox: Vec::new(),
            last_ping: Instant::now(),
            last_heard: Instant::now(),
        };
        let thread = std::thread::Builder::new()
            .name("chromecast".into())
            .spawn(move || {
                let reason = worker.run();
                worker.finish(reason);
            })
            .map_err(|e| Error::Io(e.to_string()))?;
        let connection = Self {
            outgoing,
            pending,
            closed,
            thread: Some(thread),
        };
        connection.send(RECEIVER, NS_CONNECTION, json!({"type": "CONNECT"}));
        Ok(connection)
    }

    pub fn send(&self, destination: &str, namespace: &str, payload: Value) {
        let _ = self.outgoing.send(Outgoing {
            destination: destination.to_string(),
            namespace: namespace.to_string(),
            payload: payload.to_string(),
        });
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub fn close(&mut self) {
        self.send(RECEIVER, NS_CONNECTION, json!({"type": "CLOSE"}));
        self.closed.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.close();
    }
}

struct Worker {
    socket: TcpStream,
    tls: ClientConnection,
    queue: flume::Receiver<Outgoing>,
    pending: Pending,
    events: flume::Sender<Event>,
    closed: Arc<AtomicBool>,
    inbox: Vec<u8>,
    last_ping: Instant,
    last_heard: Instant,
}

impl Worker {
    fn run(&mut self) -> String {
        let mut plain = vec![0u8; 16 * 1024];
        loop {
            let closing = self.closed.load(Ordering::Acquire);
            while let Ok(message) = self.queue.try_recv() {
                if let Err(e) = self.write(&message) {
                    return e.to_string();
                }
            }
            if self.last_ping.elapsed() >= PING_EVERY {
                self.last_ping = Instant::now();
                let ping = Outgoing {
                    destination: RECEIVER.into(),
                    namespace: NS_HEARTBEAT.into(),
                    payload: json!({"type": "PING"}).to_string(),
                };
                if let Err(e) = self.write(&ping) {
                    return e.to_string();
                }
            }
            if let Err(e) = self.flush() {
                return e.to_string();
            }
            if closing {
                return "closed".into();
            }
            if self.last_heard.elapsed() > SILENCE_LIMIT {
                return "the device stopped answering".into();
            }
            match self.tls.read_tls(&mut self.socket) {
                Ok(0) => return "the device closed the connection".into(),
                Ok(_) => {
                    if let Err(e) = self.tls.process_new_packets() {
                        return e.to_string();
                    }
                    loop {
                        match self.tls.reader().read(&mut plain) {
                            Ok(0) => return "the device closed the connection".into(),
                            Ok(n) => self.inbox.extend_from_slice(&plain[..n]),
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                            Err(e) => return e.to_string(),
                        }
                    }
                    if let Err(e) = self.drain_inbox() {
                        return e;
                    }
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return e.to_string(),
            }
        }
    }

    fn write(&mut self, message: &Outgoing) -> io::Result<()> {
        let frame = CastMessage {
            source: SENDER.into(),
            destination: message.destination.clone(),
            namespace: message.namespace.clone(),
            payload: message.payload.clone(),
        }
        .encode();
        self.tls.writer().write_all(&frame)
    }

    fn flush(&mut self) -> io::Result<()> {
        while self.tls.wants_write() {
            self.tls.write_tls(&mut self.socket)?;
        }
        Ok(())
    }

    fn drain_inbox(&mut self) -> Result<(), String> {
        loop {
            if self.inbox.len() < 4 {
                return Ok(());
            }
            let len =
                u32::from_be_bytes([self.inbox[0], self.inbox[1], self.inbox[2], self.inbox[3]])
                    as usize;
            if len > MAX_FRAME {
                return Err(format!("a {len}-byte message is too large"));
            }
            if self.inbox.len() < 4 + len {
                return Ok(());
            }
            let message = CastMessage::decode(&self.inbox[4..4 + len])?;
            self.inbox.drain(..4 + len);
            self.last_heard = Instant::now();
            self.dispatch(message);
        }
    }

    fn dispatch(&mut self, message: CastMessage) {
        let Ok(payload) = serde_json::from_str::<Value>(&message.payload) else {
            log::debug!("chromecast: non-JSON payload on {}", message.namespace);
            return;
        };
        let kind = payload["type"].as_str().unwrap_or_default();
        if message.namespace == NS_HEARTBEAT {
            if kind == "PING" {
                let pong = Outgoing {
                    destination: message.source,
                    namespace: NS_HEARTBEAT.into(),
                    payload: json!({"type": "PONG"}).to_string(),
                };
                let _ = self.write(&pong);
            }
            return;
        }
        if message.namespace == NS_CONNECTION {
            if kind == "CLOSE" {
                let _ = self.events.send(Event::TransportClosed(message.source));
            }
            return;
        }
        let request = payload["requestId"]
            .as_u64()
            .and_then(|id| u32::try_from(id).ok());
        if let Some(waiter) = request
            .filter(|id| *id != 0)
            .and_then(|id| lock(&self.pending).remove(&id))
        {
            let _ = waiter.send(payload.clone());
        }
        if let Some(event) = Event::from_payload(&payload) {
            let _ = self.events.send(event);
        }
    }

    fn finish(&mut self, reason: String) {
        self.closed.store(true, Ordering::Release);
        let _ = self.flush();
        lock(&self.pending).clear();
        if reason != "closed" {
            log::info!("chromecast: connection ended: {reason}");
            let _ = self.events.send(Event::Disconnected(reason));
        }
    }
}
