use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

#[test]
fn serves_state_snapshot() {
    let addr: SocketAddr = ([127, 0, 0, 1], 18770).into();
    let (handle, rx) = pawse_remote::channel();
    let (commands, _command_rx) = pawse_remote::commands();
    let (_server, _ready) = pawse_remote::spawn(addr, rx, commands);
    handle.publish(pawse_remote::PlayerState {
        has_track: true,
        title: Some("Smoke Track".into()),
        playing: true,
        ..Default::default()
    });

    let body = wait_for_state(addr);
    assert!(body.contains("\"v\":1"), "body: {body}");
    assert!(body.contains("Smoke Track"), "body: {body}");
    assert!(body.contains("\"playing\":true"), "body: {body}");
}

fn wait_for_state(addr: SocketAddr) -> String {
    for _ in 0..50 {
        if let Some(body) = try_get(addr, "/api/state") {
            return body;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("server did not respond on {addr}");
}

fn try_get(addr: SocketAddr, path: &str) -> Option<String> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(200)).ok()?;
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .ok()?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf).ok()?;
    let body = buf.split("\r\n\r\n").nth(1)?.to_string();
    if body.is_empty() { None } else { Some(body) }
}
