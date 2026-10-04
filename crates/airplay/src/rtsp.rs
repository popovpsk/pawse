use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::Duration;

use crate::Error;
use crate::secure::{FrameCipher, MAX_FRAME, TAG_LEN};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024;

#[derive(Debug)]
pub(crate) struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

struct Channel {
    write: FrameCipher,
    read: FrameCipher,
}

pub(crate) struct Rtsp {
    stream: TcpStream,
    inbox: Vec<u8>,
    channel: Option<Channel>,
    sequence: u32,
    url: String,
    user_agent: &'static str,
    client_instance: String,
    active_remote: u32,
    session: Option<String>,
}

impl Rtsp {
    pub fn connect(
        address: SocketAddr,
        session_id: u32,
        client_instance: String,
        user_agent: &'static str,
    ) -> Result<Self, Error> {
        let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
            .map_err(|e| Error::Io(format!("{address}: {e}")))?;
        stream.set_nodelay(true).ok();
        stream.set_read_timeout(Some(IO_TIMEOUT)).ok();
        stream.set_write_timeout(Some(IO_TIMEOUT)).ok();
        let local = stream
            .local_addr()
            .map_err(|e| Error::Io(e.to_string()))?
            .ip();
        Ok(Self {
            stream,
            inbox: Vec::new(),
            channel: None,
            sequence: 0,
            url: format!("rtsp://{local}/{session_id}"),
            user_agent,
            client_instance,
            active_remote: session_id,
            session: None,
        })
    }

    pub fn local_ip(&self) -> Result<IpAddr, Error> {
        self.stream
            .local_addr()
            .map(|address| address.ip())
            .map_err(|e| Error::Io(e.to_string()))
    }

    pub fn peer_ip(&self) -> Result<IpAddr, Error> {
        self.stream
            .peer_addr()
            .map(|address| address.ip())
            .map_err(|e| Error::Io(e.to_string()))
    }

    pub fn set_timeout(&self, timeout: Duration) {
        self.stream.set_read_timeout(Some(timeout)).ok();
        self.stream.set_write_timeout(Some(timeout)).ok();
    }

    pub fn reset_timeout(&self) {
        self.set_timeout(IO_TIMEOUT);
    }

    pub fn set_session(&mut self, session: String) {
        self.session = Some(session);
    }

    pub fn encrypt(&mut self, write_key: &[u8; 32], read_key: &[u8; 32]) {
        self.inbox.clear();
        self.channel = Some(Channel {
            write: FrameCipher::new(write_key),
            read: FrameCipher::new(read_key),
        });
    }

    pub fn request(
        &mut self,
        method: &str,
        target: Option<&str>,
        headers: &[(&str, String)],
        body: Option<(&str, &[u8])>,
    ) -> Result<Response, Error> {
        self.sequence += 1;
        let mut request = format!(
            "{method} {} RTSP/1.0\r\nCSeq: {}\r\nUser-Agent: {}\r\nClient-Instance: {}\r\nDACP-ID: {}\r\nActive-Remote: {}\r\n",
            target.unwrap_or(&self.url),
            self.sequence,
            self.user_agent,
            self.client_instance,
            self.client_instance,
            self.active_remote,
        );
        if let Some(session) = &self.session {
            request.push_str(&format!("Session: {session}\r\n"));
        }
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        match body {
            Some((kind, bytes)) => request.push_str(&format!(
                "Content-Type: {kind}\r\nContent-Length: {}\r\n",
                bytes.len()
            )),
            None if self.channel.is_some() => request.push_str("Content-Length: 0\r\n"),
            None => {}
        }
        request.push_str("\r\n");
        let mut bytes = request.into_bytes();
        if let Some((_, body)) = body {
            bytes.extend_from_slice(body);
        }
        if let Some(channel) = &mut self.channel {
            bytes = channel.write.seal(&bytes);
        }
        self.stream
            .write_all(&bytes)
            .map_err(|e| Error::Io(format!("{method}: {e}")))?;
        let response = self.read_response().map_err(|e| match e {
            Error::Io(message) => Error::Io(format!("{method}: {message}")),
            other => other,
        })?;
        match response.status {
            200..=299 => Ok(response),
            401 => Err(Error::PasswordRequired),
            status => Err(Error::Refused(format!("{method} answered {status}"))),
        }
    }

    fn read_response(&mut self) -> Result<Response, Error> {
        loop {
            if let Some(response) = take_response(&mut self.inbox)? {
                return Ok(response);
            }
            self.fill()?;
        }
    }

    fn fill(&mut self) -> Result<(), Error> {
        let closed = || Error::Io("the device closed the connection".into());
        let io = |e: std::io::Error| match e.kind() {
            std::io::ErrorKind::UnexpectedEof => closed(),
            _ => Error::Io(e.to_string()),
        };
        match &mut self.channel {
            None => {
                let mut buffer = [0u8; 4096];
                let read = self.stream.read(&mut buffer).map_err(io)?;
                if read == 0 {
                    return Err(closed());
                }
                self.inbox.extend_from_slice(&buffer[..read]);
            }
            Some(channel) => {
                let mut length = [0u8; 2];
                self.stream.read_exact(&mut length).map_err(io)?;
                let size = usize::from(u16::from_le_bytes(length));
                if size > MAX_FRAME {
                    return Err(Error::Io(format!("a {size}-byte encrypted frame")));
                }
                let mut sealed = vec![0u8; size + TAG_LEN];
                self.stream.read_exact(&mut sealed).map_err(io)?;
                let plain = channel
                    .read
                    .open(length, &sealed)
                    .ok_or_else(|| Error::Io("an answer failed to decrypt".into()))?;
                self.inbox.extend_from_slice(&plain);
            }
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

pub(crate) struct Message {
    pub first_line: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

pub(crate) fn take_message(inbox: &mut Vec<u8>) -> Result<Option<Message>, Error> {
    let Some((end, separator)) = head_end(inbox) else {
        if inbox.len() > MAX_HEAD {
            return Err(Error::Io("a message head is too large".into()));
        }
        return Ok(None);
    };
    let head = String::from_utf8_lossy(&inbox[..end]).into_owned();
    let mut lines = head.split('\n').map(|line| line.trim_end_matches('\r'));
    let first_line = lines.next().unwrap_or_default().to_string();
    let headers: HashMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    let length = headers
        .get("content-length")
        .and_then(|length| length.parse::<usize>().ok())
        .unwrap_or(0);
    if length > MAX_BODY {
        return Err(Error::Io(format!("a {length}-byte message is too large")));
    }
    let start = end + separator;
    if inbox.len() < start + length {
        return Ok(None);
    }
    let body = inbox[start..start + length].to_vec();
    inbox.drain(..start + length);
    Ok(Some(Message {
        first_line,
        headers,
        body,
    }))
}

fn head_end(inbox: &[u8]) -> Option<(usize, usize)> {
    (0..inbox.len()).find_map(|at| {
        let rest = &inbox[at..];
        if rest.starts_with(b"\r\n\r\n") {
            Some((at, 4))
        } else if rest.starts_with(b"\n\n") {
            Some((at, 2))
        } else {
            None
        }
    })
}

fn take_response(inbox: &mut Vec<u8>) -> Result<Option<Response>, Error> {
    let Some(message) = take_message(inbox)? else {
        return Ok(None);
    };
    Ok(Some(Response {
        status: parse_status(&message.first_line)?,
        headers: message.headers,
        body: message.body,
    }))
}

fn parse_status(line: &str) -> Result<u16, Error> {
    let mut parts = line.split_whitespace();
    let protocol = parts.next().unwrap_or_default();
    if !protocol.starts_with("RTSP/") && !protocol.starts_with("HTTP/") {
        return Err(Error::Io(format!("not an RTSP answer: {}", line.trim())));
    }
    parts
        .next()
        .and_then(|status| status.parse().ok())
        .ok_or_else(|| Error::Io(format!("not an RTSP answer: {}", line.trim())))
}

pub(crate) fn transport_ports(transport: &str) -> HashMap<String, u16> {
    transport
        .split(';')
        .filter_map(|part| part.split_once('='))
        .filter_map(|(name, value)| {
            value
                .trim()
                .parse::<u16>()
                .ok()
                .map(|port| (name.trim().to_ascii_lowercase(), port))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_line_gives_the_code() {
        assert_eq!(parse_status("RTSP/1.0 200 OK").unwrap(), 200);
        assert_eq!(
            parse_status("RTSP/1.0 453 Not Enough Bandwidth").unwrap(),
            453
        );
        assert_eq!(parse_status("HTTP/1.1 200 OK").unwrap(), 200);
        assert!(parse_status("SSH-2.0 x").is_err());
        assert!(parse_status("").is_err());
    }

    #[test]
    fn an_answer_is_taken_only_once_its_body_is_complete() {
        let mut inbox = b"RTSP/1.0 200 OK\r\nCSeq: 3\r\nContent-Length: 4\r\n\r\nab".to_vec();
        assert!(take_response(&mut inbox).unwrap().is_none());
        inbox.extend_from_slice(b"cdRTSP/1.0 403 Forbidden\r\n\r\n");
        let first = take_response(&mut inbox).unwrap().unwrap();
        assert_eq!(first.status, 200);
        assert_eq!(first.header("cseq"), Some("3"));
        assert_eq!(first.body, b"abcd");
        let second = take_response(&mut inbox).unwrap().unwrap();
        assert_eq!(second.status, 403);
        assert!(second.body.is_empty());
        assert!(inbox.is_empty());
    }

    #[test]
    fn an_answer_with_bare_line_feeds_is_read_too() {
        let mut inbox = b"RTSP/1.0 200 OK\nCSeq: 1\nContent-Length: 2\n\nhi".to_vec();
        let answer = take_response(&mut inbox).unwrap().unwrap();
        assert_eq!(answer.status, 200);
        assert_eq!(answer.header("CSeq"), Some("1"));
        assert_eq!(answer.body, b"hi");
        assert!(inbox.is_empty());
    }

    #[test]
    fn the_setup_answer_lists_the_ports() {
        let ports = transport_ports(
            "RTP/AVP/UDP;unicast;mode=record;server_port=6010;control_port=6011;timing_port=6012",
        );
        assert_eq!(ports.get("server_port"), Some(&6010));
        assert_eq!(ports.get("control_port"), Some(&6011));
        assert_eq!(ports.get("timing_port"), Some(&6012));
        assert_eq!(ports.get("mode"), None);
    }
}
