use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use crate::Error;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BODY: usize = 64 * 1024;

#[derive(Debug)]
pub(crate) struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

pub(crate) struct Rtsp {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    sequence: u32,
    url: String,
    client_instance: String,
    active_remote: u32,
    session: Option<String>,
}

impl Rtsp {
    pub fn connect(
        address: SocketAddr,
        session_id: u32,
        client_instance: String,
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
        let writer = stream.try_clone().map_err(|e| Error::Io(e.to_string()))?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
            sequence: 0,
            url: format!("rtsp://{local}/{session_id}"),
            client_instance,
            active_remote: session_id,
            session: None,
        })
    }

    pub fn local_ip(&self) -> Result<std::net::IpAddr, Error> {
        self.writer
            .local_addr()
            .map(|address| address.ip())
            .map_err(|e| Error::Io(e.to_string()))
    }

    pub fn peer_ip(&self) -> Result<std::net::IpAddr, Error> {
        self.writer
            .peer_addr()
            .map(|address| address.ip())
            .map_err(|e| Error::Io(e.to_string()))
    }

    pub fn set_session(&mut self, session: String) {
        self.session = Some(session);
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
            "{method} {} RTSP/1.0\r\nCSeq: {}\r\nUser-Agent: Pawse/1.0\r\nClient-Instance: {}\r\nDACP-ID: {}\r\nActive-Remote: {}\r\n",
            target.unwrap_or(&self.url),
            self.sequence,
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
        if let Some((kind, bytes)) = body {
            request.push_str(&format!(
                "Content-Type: {kind}\r\nContent-Length: {}\r\n",
                bytes.len()
            ));
        }
        request.push_str("\r\n");
        let mut bytes = request.into_bytes();
        if let Some((_, body)) = body {
            bytes.extend_from_slice(body);
        }
        self.writer
            .write_all(&bytes)
            .map_err(|e| Error::Io(format!("{method}: {e}")))?;
        let response = self.read_response().map_err(|e| match e {
            Error::Io(message) => Error::Io(format!("{method}: {message}")),
            other => other,
        })?;
        match response.status {
            200..=299 => Ok(response),
            401 => Err(Error::PasswordRequired),
            403 | 453 | 470 => Err(Error::Refused(format!(
                "{method} answered {}",
                response.status
            ))),
            status => Err(Error::Refused(format!("{method} answered {status}"))),
        }
    }

    fn read_response(&mut self) -> Result<Response, Error> {
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .map_err(|e| Error::Io(e.to_string()))?;
        if line.is_empty() {
            return Err(Error::Io("the device closed the connection".into()));
        }
        let status = parse_status(&line)?;
        let mut headers = HashMap::new();
        loop {
            line.clear();
            self.reader
                .read_line(&mut line)
                .map_err(|e| Error::Io(e.to_string()))?;
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                break;
            }
            if let Some((name, value)) = trimmed.split_once(':') {
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }
        let length = headers
            .get("content-length")
            .and_then(|length| length.parse::<usize>().ok())
            .unwrap_or(0);
        if length > MAX_BODY {
            return Err(Error::Io(format!("a {length}-byte RTSP body is too large")));
        }
        let mut body = vec![0; length];
        self.reader
            .read_exact(&mut body)
            .map_err(|e| Error::Io(e.to_string()))?;
        Ok(Response { status, headers })
    }

    pub fn shutdown(&self) {
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
    }
}

fn parse_status(line: &str) -> Result<u16, Error> {
    let mut parts = line.split_whitespace();
    let protocol = parts.next().unwrap_or_default();
    if !protocol.starts_with("RTSP/") {
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
        assert_eq!(parse_status("RTSP/1.0 200 OK\r\n").unwrap(), 200);
        assert_eq!(
            parse_status("RTSP/1.0 453 Not Enough Bandwidth").unwrap(),
            453
        );
        assert!(parse_status("HTTP/1.1 200 OK").is_err());
        assert!(parse_status("").is_err());
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
