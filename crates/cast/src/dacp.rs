use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use airplay::RemoteCommand;
use mdns_sd::{ServiceDaemon, ServiceInfo};

const SERVICE_TYPE: &str = "_dacp._tcp.local.";
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_REQUEST: usize = 8 * 1024;
const ACCEPT_RETRY: Duration = Duration::from_millis(100);

type Routes = Arc<Mutex<HashMap<String, flume::Sender<RemoteCommand>>>>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Server {
    routes: Routes,
    _daemon: Option<ServiceDaemon>,
}

fn server() -> Option<&'static Server> {
    static SERVER: OnceLock<Option<Server>> = OnceLock::new();
    SERVER.get_or_init(start).as_ref()
}

pub(crate) fn warm_up() {
    server();
}

fn start() -> Option<Server> {
    let listener = crate::net::listen("the AirPlay remote (DACP) server")
        .inspect_err(|e| log::warn!("AirPlay remote: no DACP server: {e}"))
        .ok()?;
    let port = listener.local_addr().ok()?.port();
    let routes = Routes::default();
    let serving = routes.clone();
    std::thread::Builder::new()
        .name("cast-dacp".into())
        .spawn(move || {
            let mut logged = HashSet::new();
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => handle(stream, &serving, &mut logged),
                    Err(e) => {
                        log::debug!("AirPlay remote: accepting failed: {e}");
                        std::thread::sleep(ACCEPT_RETRY);
                    }
                }
            }
        })
        .inspect_err(|e| log::warn!("AirPlay remote: the DACP server did not start: {e}"))
        .ok()?;
    let host = format!("pawse-{}.local.", airplay::dacp_id().to_ascii_lowercase());
    let daemon = announce(&host, port);
    log::info!(
        "AirPlay remote: DACP iTunes_Ctrl_{} on {}:{port}",
        airplay::dacp_id(),
        if daemon.is_some() {
            host.as_str()
        } else {
            "nothing"
        }
    );
    Some(Server {
        routes,
        _daemon: daemon,
    })
}

fn announce(host: &str, port: u16) -> Option<ServiceDaemon> {
    let daemon = ServiceDaemon::new()
        .inspect_err(|e| log::warn!("AirPlay remote: DACP is not announced: {e}"))
        .ok()?;
    let properties = [
        ("txtvers", "1"),
        ("Ver", "131077"),
        ("DbId", "1"),
        ("OSsi", "0x2012E"),
    ];
    let info = ServiceInfo::new(
        SERVICE_TYPE,
        &format!("iTunes_Ctrl_{}", airplay::dacp_id()),
        host,
        (),
        port,
        &properties[..],
    )
    .inspect_err(|e| log::warn!("AirPlay remote: DACP is not announced: {e}"))
    .ok()?
    .enable_addr_auto();
    daemon
        .register(info)
        .inspect_err(|e| log::warn!("AirPlay remote: DACP is not announced: {e}"))
        .ok()?;
    Some(daemon)
}

pub(crate) struct Route {
    routes: Routes,
    key: String,
}

impl Drop for Route {
    fn drop(&mut self) {
        lock(&self.routes).remove(&self.key);
    }
}

pub(crate) fn route(active_remote: u32, commands: flume::Sender<RemoteCommand>) -> Option<Route> {
    let routes = server()?.routes.clone();
    let key = active_remote.to_string();
    lock(&routes).insert(key.clone(), commands);
    Some(Route { routes, key })
}

fn read_head(stream: &mut TcpStream) -> Option<String> {
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut head = Vec::new();
    let mut buffer = [0u8; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let left = deadline.checked_duration_since(Instant::now())?;
        stream
            .set_read_timeout(Some(left.max(Duration::from_millis(1))))
            .ok();
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 || head.len() + read > MAX_REQUEST {
            break;
        }
        head.extend_from_slice(&buffer[..read]);
    }
    Some(String::from_utf8_lossy(&head).into_owned())
}

fn handle(mut stream: TcpStream, routes: &Routes, logged: &mut HashSet<String>) {
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok();
    let Some(head) = read_head(&mut stream) else {
        return;
    };
    let mut lines = head.lines();
    let path = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_string();
    let active_remote = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("Active-Remote"))
        .map(|(_, value)| value.trim().to_string())
        .unwrap_or_default();
    let status = if path.starts_with("/ctrl-int/1/getproperty") {
        "400 Bad Request"
    } else {
        "204 No Content"
    };
    let _ = stream.write_all(
        format!("HTTP/1.0 {status}\r\nDAAP-Server: Pawse\r\nContent-Type: application/x-dmap-tagged\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes(),
    );
    let sender = lock(routes).get(&active_remote).cloned();
    match (airplay::dacp_command(&path), sender) {
        (Some(command), Some(sender)) => {
            log::info!("AirPlay remote: {path} asks for {command:?}");
            let _ = sender.send(command);
        }
        (command, sender) => {
            let kind = path.split('=').next().unwrap_or_default().to_string();
            let level = if logged.insert(kind) {
                log::Level::Info
            } else {
                log::Level::Debug
            };
            log::log!(
                level,
                "AirPlay remote: ignored {path} (known: {}, active remote {active_remote:?} is ours: {})",
                command.is_some(),
                sender.is_some()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn ask(routes: &Routes, request: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client.write_all(request.as_bytes()).unwrap();
        let (served, _) = listener.accept().unwrap();
        handle(served, routes, &mut HashSet::new());
        let mut answer = String::new();
        client.read_to_string(&mut answer).unwrap();
        answer
    }

    #[test]
    fn a_dacp_request_reaches_the_stream_it_names() {
        let routes = Routes::default();
        let (ours, commands) = flume::unbounded();
        lock(&routes).insert("1234".into(), ours);
        let answer = ask(
            &routes,
            "GET /ctrl-int/1/playpause HTTP/1.1\r\nHost: x\r\nActive-Remote: 1234\r\n\r\n",
        );
        assert!(answer.starts_with("HTTP/1.0 204"), "{answer}");
        assert_eq!(commands.try_recv(), Ok(RemoteCommand::PlayPause));
        ask(
            &routes,
            "GET /ctrl-int/1/nextitem HTTP/1.1\r\nActive-Remote: 999\r\n\r\n",
        );
        ask(
            &routes,
            "GET /ctrl-int/1/setproperty?dmcp.device-volume=-20.0 HTTP/1.1\r\nActive-Remote: 1234\r\n\r\n",
        );
        let polled = ask(
            &routes,
            "GET /ctrl-int/1/getproperty?properties=dmcp.volume HTTP/1.1\r\nActive-Remote: 1234\r\n\r\n",
        );
        assert!(polled.starts_with("HTTP/1.0 400"), "{polled}");
        assert!(commands.try_recv().is_err());
    }
}
