use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mood {
    Plays,
    StuckLoading,
    Stopped,
    FetchesThenStuck,
}

#[derive(Default)]
struct State {
    uri: Option<String>,
    playing_since: Option<Instant>,
    base: Duration,
    fetched: bool,
    actions: Vec<String>,
}

struct Shared {
    mood: Mood,
    state: Mutex<State>,
    off: AtomicBool,
}

pub(crate) struct FakeDlna {
    base: String,
    shared: Arc<Shared>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl FakeDlna {
    pub(crate) fn start(mood: Mood) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let shared = Arc::new(Shared {
            mood,
            state: Mutex::new(State::default()),
            off: AtomicBool::new(false),
        });
        let accepting = shared.clone();
        std::thread::spawn(move || {
            while !accepting.off.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let shared = accepting.clone();
                        std::thread::spawn(move || {
                            let _ = serve(stream, &shared);
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        });
        Self { base, shared }
    }

    pub(crate) fn location(&self) -> String {
        format!("{}/description.xml", self.base)
    }

    pub(crate) fn power_off(&self) {
        self.shared.off.store(true, Ordering::Release);
    }

    pub(crate) fn fetched(&self) -> bool {
        lock(&self.shared.state).fetched
    }

    pub(crate) fn actions(&self) -> Vec<String> {
        lock(&self.shared.state).actions.clone()
    }
}

impl Drop for FakeDlna {
    fn drop(&mut self) {
        self.power_off();
    }
}

struct Request {
    path: String,
    action: String,
    body: String,
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let path = line.split_whitespace().nth(1)?.to_string();
    let (mut length, mut action) = (0usize, String::new());
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 || header == "\r\n" {
            break;
        }
        let (name, value) = header.split_once(':')?;
        match name.to_ascii_lowercase().as_str() {
            "content-length" => length = value.trim().parse().unwrap_or(0),
            "soapaction" => {
                action = value
                    .trim()
                    .trim_matches('"')
                    .rsplit('#')
                    .next()
                    .unwrap_or_default()
                    .to_string();
            }
            _ => {}
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        path,
        action,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn serve(mut stream: TcpStream, shared: &Arc<Shared>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let Some(request) = read_request(&stream) else {
        return Ok(());
    };
    let (status, body) = if request.path == "/description.xml" {
        (200, description())
    } else {
        answer(shared, &request)
    };
    let head = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())
}

fn description() -> String {
    r#"<?xml version="1.0"?><root xmlns="urn:schemas-upnp-org:device-1-0"><device>
<deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
<friendlyName>Fake R1</friendlyName><UDN>uuid:fake-r1</UDN><serviceList>
<service><serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType><controlURL>/cm</controlURL></service>
<service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>/av</controlURL></service>
<service><serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType><controlURL>/rc</controlURL></service>
</serviceList></device></root>"#
        .into()
}

fn arg(body: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = body.find(&open)? + open.len();
    let end = start + body[start..].find(&format!("</{name}>"))?;
    Some(body[start..end].replace("&amp;", "&"))
}

fn reply(service: &str, action: &str, fields: &[(&str, String)]) -> String {
    let fields: String = fields
        .iter()
        .map(|(name, value)| format!("<{name}>{value}</{name}>"))
        .collect();
    format!(
        r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:{action}Response xmlns:u="urn:schemas-upnp-org:service:{service}:1">{fields}</u:{action}Response></s:Body></s:Envelope>"#
    )
}

fn clock(position: Duration) -> String {
    let total = position.as_secs();
    format!(
        "{}:{:02}:{:02}",
        total / 3600,
        (total / 60) % 60,
        total % 60
    )
}

fn fault(code: u32, text: &str) -> (u16, String) {
    (
        500,
        format!(
            r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail><UPnPError xmlns="urn:schemas-upnp-org:control-1-0"><errorCode>{code}</errorCode><errorDescription>{text}</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>"#
        ),
    )
}

fn parse_clock(text: &str) -> Duration {
    let mut seconds = 0;
    for part in text.trim().split(':') {
        seconds = seconds * 60 + part.parse::<u64>().unwrap_or(0);
    }
    Duration::from_secs(seconds)
}

fn answer(shared: &Arc<Shared>, request: &Request) -> (u16, String) {
    let mut state = lock(&shared.state);
    state.actions.push(request.action.clone());
    let body = match request.path.as_str() {
        "/cm" => reply(
            "ConnectionManager",
            &request.action,
            &[(
                "Sink",
                "http-get:*:audio/wav:*,http-get:*:audio/flac:*".into(),
            )],
        ),
        "/rc" => reply(
            "RenderingControl",
            &request.action,
            &[("CurrentVolume", "50".into()), ("CurrentMute", "0".into())],
        ),
        _ => {
            let loading = matches!(shared.mood, Mood::StuckLoading | Mood::FetchesThenStuck);
            if request.action == "Seek" && loading {
                return fault(701, "Transition not allowed");
            }
            transport(shared, &mut state, request)
        }
    };
    (200, body)
}

fn transport(shared: &Arc<Shared>, state: &mut State, request: &Request) -> String {
    match request.action.as_str() {
        "SetAVTransportURI" => {
            state.uri = arg(&request.body, "CurrentURI");
            state.playing_since = None;
            state.base = Duration::ZERO;
        }
        "Seek" => {
            state.base = parse_clock(&arg(&request.body, "Target").unwrap_or_default());
            if state.playing_since.is_some() {
                state.playing_since = Some(Instant::now());
            }
        }
        "Play" => {
            if matches!(shared.mood, Mood::Plays | Mood::FetchesThenStuck)
                && let Some(uri) = state.uri.clone()
            {
                let shared = shared.clone();
                std::thread::spawn(move || {
                    if fetch(&uri) {
                        lock(&shared.state).fetched = true;
                    }
                });
            }
            if shared.mood == Mood::Plays {
                state.playing_since = Some(Instant::now());
            }
        }
        "Stop" => state.playing_since = None,
        _ => {}
    }
    let transport_state = match (shared.mood, state.uri.is_some(), state.playing_since) {
        (_, false, _) => "NO_MEDIA_PRESENT",
        (Mood::Plays, true, Some(_)) => "PLAYING",
        (Mood::Plays, true, None) | (Mood::Stopped, ..) => "STOPPED",
        (Mood::StuckLoading | Mood::FetchesThenStuck, ..) => "TRANSITIONING",
    };
    let position = state.base
        + state
            .playing_since
            .map_or(Duration::ZERO, |since| since.elapsed());
    reply(
        "AVTransport",
        &request.action,
        &[
            ("CurrentTransportState", transport_state.into()),
            ("CurrentTransportStatus", "OK".into()),
            ("CurrentSpeed", "1".into()),
            ("RelTime", clock(position)),
            ("TrackDuration", "0:01:00".into()),
            ("TrackURI", state.uri.clone().unwrap_or_default()),
        ],
    )
}

fn fetch(uri: &str) -> bool {
    let Some(rest) = uri.strip_prefix("http://") else {
        return false;
    };
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let Ok(mut stream) = TcpStream::connect(authority) else {
        return false;
    };
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    if write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n"
    )
    .is_err()
    {
        return false;
    }
    let mut sink = Vec::new();
    stream.read_to_end(&mut sink).is_ok()
}
