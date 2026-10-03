use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::media::StreamOpener;
use crate::pcm::{PcmReader, PcmSpec};

const MAX_CONNECTIONS: usize = 32;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const CHUNK: usize = 64 * 1024;
const DLNA_FLAGS: &str = "DLNA.ORG_FLAGS=01700000000000000000000000000000";

#[derive(Clone)]
pub enum Body {
    File(PathBuf),
    Stream(StreamOpener),
    Pcm(PcmSpec),
    Bytes(Arc<Vec<u8>>),
}

#[derive(Clone)]
pub struct Entry {
    pub body: Body,
    pub mime: String,
}

impl Entry {
    fn converted(&self) -> bool {
        matches!(self.body, Body::Pcm(_))
    }

    fn len(&self) -> io::Result<Option<u64>> {
        match &self.body {
            Body::File(path) => Ok(Some(std::fs::metadata(path)?.len())),
            Body::Stream(open) => Ok(open()?.byte_len()),
            Body::Pcm(spec) => Ok(Some(spec.len())),
            Body::Bytes(bytes) => Ok(Some(bytes.len() as u64)),
        }
    }

    fn open(&self, offset: u64) -> io::Result<Box<dyn Read + Send>> {
        match &self.body {
            Body::File(path) => {
                let mut file = File::open(path)?;
                file.seek(SeekFrom::Start(offset))?;
                Ok(Box::new(file))
            }
            Body::Stream(open) => {
                let mut stream = open()?;
                stream.seek(SeekFrom::Start(offset))?;
                Ok(Box::new(stream))
            }
            Body::Pcm(spec) => Ok(Box::new(PcmReader::open(spec.clone(), offset)?)),
            Body::Bytes(bytes) => {
                let start = (offset as usize).min(bytes.len());
                Ok(Box::new(io::Cursor::new(bytes[start..].to_vec())))
            }
        }
    }

    pub fn content_features(&self) -> String {
        let converted = if self.converted() { 1 } else { 0 };
        format!("DLNA.ORG_OP=01;DLNA.ORG_CI={converted};{DLNA_FLAGS}")
    }
}

struct Shared {
    token: String,
    entries: Mutex<HashMap<String, Entry>>,
    next: AtomicUsize,
    stopped: AtomicBool,
    connections: AtomicUsize,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct MediaServer {
    port: u16,
    shared: Arc<Shared>,
}

fn token() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default();
        bytes = nanos.to_le_bytes();
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl MediaServer {
    pub fn start() -> io::Result<Arc<Self>> {
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))?;
        let port = listener.local_addr()?.port();
        let shared = Arc::new(Shared {
            token: token(),
            entries: Mutex::new(HashMap::new()),
            next: AtomicUsize::new(1),
            stopped: AtomicBool::new(false),
            connections: AtomicUsize::new(0),
        });
        let accepting = shared.clone();
        std::thread::Builder::new()
            .name("cast-http".into())
            .spawn(move || accept(listener, accepting))?;
        log::info!("cast: serving media on port {port}");
        Ok(Arc::new(Self { port, shared }))
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn publish(&self, entry: Entry, extension: &str) -> String {
        let id = self.shared.next.fetch_add(1, Ordering::Relaxed);
        let name = format!("{id}.{extension}");
        lock(&self.shared.entries).insert(name.clone(), entry);
        format!("/{}/{name}", self.shared.token)
    }

    pub fn remove(&self, paths: &[String]) {
        let mut entries = lock(&self.shared.entries);
        for path in paths {
            if let Some(name) = path.rsplit('/').next() {
                entries.remove(name);
            }
        }
    }

    pub fn url(&self, peer: IpAddr, path: &str) -> io::Result<String> {
        match crate::net::local_ip_for(peer)? {
            IpAddr::V4(v4) => Ok(format!("http://{v4}:{}{path}", self.port)),
            IpAddr::V6(_) => Err(io::Error::other(
                "the device is only reachable over IPv6, which the media server does not serve",
            )),
        }
    }

    #[cfg(test)]
    fn entry(&self, path: &str) -> Option<Entry> {
        lookup(&self.shared, path)
    }
}

impl Drop for MediaServer {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Release);
        let _ = TcpStream::connect_timeout(
            &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.port),
            Duration::from_millis(200),
        );
    }
}

fn lookup(shared: &Shared, path: &str) -> Option<Entry> {
    let path = path.split('?').next().unwrap_or(path);
    let rest = path.strip_prefix('/')?;
    let (token, name) = rest.split_once('/')?;
    if token != shared.token {
        return None;
    }
    lock(&shared.entries).get(name).cloned()
}

fn accept(listener: TcpListener, shared: Arc<Shared>) {
    for stream in listener.incoming() {
        if shared.stopped.load(Ordering::Acquire) {
            return;
        }
        let Ok(stream) = stream else {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        if shared.connections.load(Ordering::Acquire) >= MAX_CONNECTIONS {
            log::warn!("cast: too many media connections, refusing one");
            continue;
        }
        shared.connections.fetch_add(1, Ordering::AcqRel);
        let serving = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("cast-http-conn".into())
            .spawn(move || {
                if let Err(e) = serve(stream, &serving) {
                    log::debug!("cast: media connection ended: {e}");
                }
                serving.connections.fetch_sub(1, Ordering::AcqRel);
            });
        if spawned.is_err() {
            shared.connections.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    close: bool,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }
}

fn read_limited_line(
    reader: &mut BufReader<TcpStream>,
    line: &mut String,
    left: usize,
) -> io::Result<usize> {
    (&mut *reader).take(left as u64 + 1).read_line(line)
}

fn read_request(reader: &mut BufReader<TcpStream>) -> io::Result<Option<Request>> {
    let mut line = String::new();
    if read_limited_line(reader, &mut line, MAX_HEADER_BYTES)? == 0 {
        return Ok(None);
    }
    if line.len() > MAX_HEADER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "request line too long",
        ));
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_ascii_uppercase();
    let path = parts.next().unwrap_or_default().to_string();
    let version = parts.next().unwrap_or("HTTP/1.0").to_ascii_uppercase();
    let mut headers = HashMap::new();
    let mut total = line.len();
    loop {
        line.clear();
        let read = read_limited_line(reader, &mut line, MAX_HEADER_BYTES.saturating_sub(total))?;
        total += read;
        if read == 0 || total > MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bad request head",
            ));
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let connection = headers
        .get("connection")
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    let close = connection.contains("close")
        || (version == "HTTP/1.0" && !connection.contains("keep-alive"));
    Ok(Some(Request {
        method,
        path,
        headers,
        close,
    }))
}

pub(crate) fn parse_range(header: &str, len: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = header.trim().strip_prefix("bytes=") else {
        return Ok(None);
    };
    let first = spec.split(',').next().unwrap_or_default().trim();
    let Some((start, end)) = first.split_once('-') else {
        return Ok(None);
    };
    let (start, end) = (start.trim(), end.trim());
    if len == 0 {
        return Err(());
    }
    if start.is_empty() {
        let suffix: u64 = end.parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        return Ok(Some((len.saturating_sub(suffix), len - 1)));
    }
    let start: u64 = start.parse().map_err(|_| ())?;
    if start >= len {
        return Err(());
    }
    let end = if end.is_empty() {
        len - 1
    } else {
        end.parse::<u64>().map_err(|_| ())?.min(len - 1)
    };
    if end < start {
        return Err(());
    }
    Ok(Some((start, end)))
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        _ => "Internal Server Error",
    }
}

fn write_head(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(&str, String)],
    close: bool,
) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {status} {}\r\n", status_text(status));
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("Server: Pawse UPnP/1.0 DLNADOC/1.50\r\n");
    head.push_str("Access-Control-Allow-Origin: *\r\n");
    head.push_str(if close {
        "Connection: close\r\n\r\n"
    } else {
        "Connection: keep-alive\r\n\r\n"
    });
    stream.write_all(head.as_bytes())
}

fn serve(stream: TcpStream, shared: &Shared) -> io::Result<()> {
    stream.set_read_timeout(Some(IDLE_TIMEOUT))?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    stream.set_nodelay(true).ok();
    let peer = stream.peer_addr().ok();
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    while let Some(request) = read_request(&mut reader)? {
        log::debug!(
            "cast: {:?} {} {} range {:?}",
            peer,
            request.method,
            request.path,
            request.header("range")
        );
        let keep = respond(&mut writer, &request, shared)?;
        if !keep || request.close {
            return Ok(());
        }
    }
    Ok(())
}

fn respond(stream: &mut TcpStream, request: &Request, shared: &Shared) -> io::Result<bool> {
    let head_only = match request.method.as_str() {
        "GET" => false,
        "HEAD" => true,
        _ => {
            write_head(stream, 405, &[("Content-Length", "0".into())], true)?;
            return Ok(false);
        }
    };
    let Some(entry) = lookup(shared, &request.path) else {
        write_head(stream, 404, &[("Content-Length", "0".into())], false)?;
        return Ok(true);
    };
    let len = match entry.len() {
        Ok(len) => len,
        Err(e) => {
            log::warn!("cast: {} cannot be opened: {e}", request.path);
            write_head(stream, 500, &[("Content-Length", "0".into())], true)?;
            return Ok(false);
        }
    };
    let mut headers: Vec<(&str, String)> = vec![("Content-Type", entry.mime.clone())];
    headers.push(("transferMode.dlna.org", "Streaming".into()));
    headers.push(("contentFeatures.dlna.org", entry.content_features()));
    headers.push(("Cache-Control", "no-cache".into()));
    let Some(len) = len else {
        if head_only {
            write_head(stream, 200, &headers, true)?;
            return Ok(false);
        }
        write_head(stream, 200, &headers, true)?;
        let mut body = entry.open(0)?;
        copy(&mut body, stream, None)?;
        return Ok(false);
    };
    headers.push(("Accept-Ranges", "bytes".into()));
    let range = match request.header("range").map(|range| parse_range(range, len)) {
        Some(Err(())) => {
            headers.push(("Content-Range", format!("bytes */{len}")));
            headers.push(("Content-Length", "0".into()));
            write_head(stream, 416, &headers, false)?;
            return Ok(true);
        }
        Some(Ok(range)) => range,
        None => None,
    };
    let (status, start, count) = match range {
        Some((start, end)) => {
            headers.push(("Content-Range", format!("bytes {start}-{end}/{len}")));
            (206, start, end - start + 1)
        }
        None => (200, 0, len),
    };
    headers.push(("Content-Length", count.to_string()));
    if head_only {
        write_head(stream, status, &headers, false)?;
        return Ok(true);
    }
    let mut body = match entry.open(start) {
        Ok(body) => body,
        Err(e) => {
            log::warn!("cast: {} cannot be read: {e}", request.path);
            write_head(stream, 500, &[("Content-Length", "0".into())], true)?;
            return Ok(false);
        }
    };
    write_head(stream, status, &headers, false)?;
    let sent = copy(&mut body, stream, Some(count))?;
    if sent < count {
        log::warn!("cast: {} ended after {sent} of {count} bytes", request.path);
        return Ok(false);
    }
    Ok(true)
}

fn copy(body: &mut dyn Read, stream: &mut TcpStream, limit: Option<u64>) -> io::Result<u64> {
    let mut buffer = vec![0u8; CHUNK];
    let mut sent = 0u64;
    loop {
        let want = match limit {
            Some(limit) if sent >= limit => return Ok(sent),
            Some(limit) => ((limit - sent) as usize).min(CHUNK),
            None => CHUNK,
        };
        let read = match body.read(&mut buffer[..want]) {
            Ok(0) => return Ok(sent),
            Ok(read) => read,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                log::warn!("cast: reading media failed: {e}");
                return Ok(sent);
            }
        };
        stream.write_all(&buffer[..read])?;
        sent += read as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_follow_rfc_7233() {
        assert_eq!(parse_range("bytes=0-", 100), Ok(Some((0, 99))));
        assert_eq!(parse_range("bytes=10-19", 100), Ok(Some((10, 19))));
        assert_eq!(parse_range("bytes=90-200", 100), Ok(Some((90, 99))));
        assert_eq!(parse_range("bytes=-10", 100), Ok(Some((90, 99))));
        assert_eq!(parse_range("bytes=-500", 100), Ok(Some((0, 99))));
        assert_eq!(parse_range("bytes=5-7,20-30", 100), Ok(Some((5, 7))));
        assert_eq!(parse_range("bytes=100-", 100), Err(()));
        assert_eq!(parse_range("bytes=20-10", 100), Err(()));
        assert_eq!(parse_range("items=0-1", 100), Ok(None));
    }

    #[test]
    fn entries_are_only_found_under_the_token() {
        let server = MediaServer::start().unwrap();
        let path = server.publish(
            Entry {
                body: Body::Bytes(Arc::new(vec![1, 2, 3])),
                mime: "audio/flac".into(),
            },
            "flac",
        );
        assert!(server.entry(&path).is_some());
        assert!(server.entry(&format!("{path}?x=1")).is_some());
        assert!(server.entry("/wrong/1.flac").is_none());
        server.remove(std::slice::from_ref(&path));
        assert!(server.entry(&path).is_none());
    }
}
