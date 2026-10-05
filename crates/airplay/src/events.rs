use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::metadata::{RemoteCommand, remote_command};
use crate::rtsp::{Message, take_message};
use crate::secure::{FrameCipher, MAX_FRAME, TAG_LEN};

const POLL: Duration = Duration::from_millis(200);
const WRITE_TIMEOUT: Duration = Duration::from_secs(1);

pub(crate) struct EventChannel {
    pub stream: TcpStream,
    pub read: FrameCipher,
    pub write: FrameCipher,
}

impl EventChannel {
    pub fn serve(mut self, stop: impl Fn() -> bool, mut command: impl FnMut(RemoteCommand)) {
        self.stream.set_read_timeout(Some(POLL)).ok();
        self.stream.set_write_timeout(Some(WRITE_TIMEOUT)).ok();
        let mut raw = Vec::new();
        let mut inbox = Vec::new();
        let mut buffer = [0u8; 4096];
        while !stop() {
            match self.stream.read(&mut buffer) {
                Ok(0) => {
                    log::info!("AirPlay: the device closed its event channel");
                    return;
                }
                Ok(read) => raw.extend_from_slice(&buffer[..read]),
                Err(e)
                    if matches!(
                        e.kind(),
                        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                    ) =>
                {
                    continue;
                }
                Err(e) => {
                    log::info!("AirPlay: the event channel failed: {e}");
                    return;
                }
            }
            if self.open_frames(&mut raw, &mut inbox).is_none() {
                log::warn!("AirPlay: an event from the device failed to decrypt");
                return;
            }
            loop {
                match take_message(&mut inbox) {
                    Ok(Some(message)) => {
                        if !self.answer(&message) {
                            return;
                        }
                        match remote_command(&message.body) {
                            Ok(received) => {
                                log::info!("AirPlay: the device asks for {received:?}");
                                command(received);
                            }
                            Err(what) => log::info!(
                                "AirPlay: ignored an event ({}: {what})",
                                message.first_line
                            ),
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        log::debug!("AirPlay: an unreadable event: {e}");
                        return;
                    }
                }
            }
        }
    }

    fn open_frames(&mut self, raw: &mut Vec<u8>, inbox: &mut Vec<u8>) -> Option<()> {
        loop {
            let [low, high, ..] = raw[..] else {
                return Some(());
            };
            let size = usize::from(u16::from_le_bytes([low, high]));
            if size > MAX_FRAME {
                return None;
            }
            let end = 2 + size + TAG_LEN;
            if raw.len() < end {
                return Some(());
            }
            inbox.extend(self.read.open([low, high], &raw[2..end])?);
            raw.drain(..end);
        }
    }

    fn answer(&mut self, message: &Message) -> bool {
        let header = |name: &str| {
            message
                .headers
                .get(&name.to_ascii_lowercase())
                .map(|value| format!("{name}: {value}\r\n"))
                .unwrap_or_default()
        };
        let version = message
            .first_line
            .split_whitespace()
            .nth(2)
            .unwrap_or("RTSP/1.0");
        let reply = format!(
            "{version} 200 OK\r\nContent-Length: 0\r\nAudio-Latency: 0\r\n{}{}\r\n",
            header("Server"),
            header("CSeq")
        );
        let sealed = self.write.seal(reply.as_bytes());
        self.stream.write_all(&sealed).is_ok()
    }
}
