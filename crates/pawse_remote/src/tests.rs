use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use super::*;

struct EmptyLibrary;

impl LibraryReader for EmptyLibrary {
    fn cover(&self, _: i64, _: CoverSize) -> Option<Vec<u8>> {
        None
    }
    fn cover_original(&self, _: i64) -> Option<(Vec<u8>, String)> {
        None
    }
    fn artists(&self, _: ArtistSort, _: bool) -> Vec<ArtistEntry> {
        Vec::new()
    }
    fn artist_detail(&self, _: i64, _: bool) -> Option<ArtistDetail> {
        None
    }
    fn albums(&self, _: AlbumSort, _: bool) -> Vec<AlbumEntry> {
        Vec::new()
    }
    fn album_detail(&self, _: i64) -> Option<AlbumDetail> {
        None
    }
    fn playlists(&self) -> Vec<PlaylistEntry> {
        Vec::new()
    }
    fn playlist_detail(&self, _: i64) -> Option<PlaylistDetail> {
        None
    }
    fn liked(&self) -> Vec<PlaylistTrack> {
        Vec::new()
    }
    fn lyrics(&self, _: i64) -> TrackLyrics {
        TrackLyrics::default()
    }
}

fn free_addr() -> SocketAddr {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

fn start(addr: SocketAddr, rx: StateRx) -> RemoteServer {
    let (commands, _) = commands();
    let (server, ready) = spawn(addr, rx, commands, Arc::new(EmptyLibrary));
    assert_eq!(ready.blocking_recv().unwrap(), Ok(()));
    server
}

fn connect(addr: SocketAddr) -> TcpStream {
    let stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
}

fn read_head(reader: &mut BufReader<TcpStream>) -> String {
    let mut head = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line.is_empty() || line == "\r\n" {
            return head;
        }
        head.push_str(&line);
    }
}

#[test]
fn a_library_read_is_answered() {
    let addr = free_addr();
    let (_state, rx) = channel();
    let _server = start(addr, rx);
    let mut stream = connect(addr);
    write!(
        stream,
        "GET /api/playlists HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(reply.ends_with("[]"), "{reply}");
}

#[test]
fn a_dropped_server_closes_kept_alive_connections_and_frees_its_port() {
    let addr = free_addr();
    let (_state, rx) = channel();
    let server = start(addr, rx.clone());
    let mut stream = connect(addr);
    write!(stream, "GET /api/liked HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    let mut reader = BufReader::new(stream);
    let head = read_head(&mut reader);
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    let length: usize = head
        .lines()
        .find_map(|line| {
            let line = line.to_ascii_lowercase();
            line.strip_prefix("content-length:")
                .map(|value| value.trim().parse().unwrap())
        })
        .expect("no Content-Length");
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    drop(server);
    let mut rest = Vec::new();
    reader
        .read_to_end(&mut rest)
        .expect("a kept-alive connection stayed open after the server was dropped");
    assert!(TcpStream::connect(addr).is_err());
    drop(start(addr, rx));
}

#[test]
fn a_dropped_server_ends_a_live_websocket() {
    let addr = free_addr();
    let (_state, rx) = channel();
    let server = start(addr, rx);
    let mut stream = connect(addr);
    write!(
        stream,
        "GET /ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
    .unwrap();
    let mut reader = BufReader::new(stream);
    let head = read_head(&mut reader);
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    let mut first = [0u8; 2];
    reader.read_exact(&mut first).unwrap();
    assert_eq!(first[0] & 0x0f, 0x1, "the first frame is the state as text");
    drop(server);
    let mut rest = Vec::new();
    reader
        .read_to_end(&mut rest)
        .expect("the socket stayed open after the server was dropped");
}
