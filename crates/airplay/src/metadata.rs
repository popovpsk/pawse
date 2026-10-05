use std::io::Cursor;
use std::sync::Arc;

use plist::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cover {
    pub mime: String,
    pub bytes: Arc<Vec<u8>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub cover: Option<Cover>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteCommand {
    Play,
    Pause,
    PlayPause,
    Next,
    Previous,
}

pub fn dacp_command(path: &str) -> Option<RemoteCommand> {
    let command = path.strip_prefix("/ctrl-int/1/")?;
    let command = command.split(['?', '&']).next().unwrap_or(command);
    match command {
        "play" => Some(RemoteCommand::Play),
        "pause" | "discrete-pause" | "stop" => Some(RemoteCommand::Pause),
        "playpause" => Some(RemoteCommand::PlayPause),
        "nextitem" => Some(RemoteCommand::Next),
        "previtem" => Some(RemoteCommand::Previous),
        _ => None,
    }
}

fn item(out: &mut Vec<u8>, tag: &[u8; 4], value: &[u8]) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&(value.len() as u32).to_be_bytes());
    out.extend_from_slice(value);
}

pub(crate) fn dmap(now: &NowPlaying) -> Vec<u8> {
    let mut items = Vec::new();
    item(&mut items, b"mikd", &[2]);
    item(&mut items, b"minm", now.title.as_bytes());
    item(
        &mut items,
        b"asar",
        now.artist.as_deref().unwrap_or_default().as_bytes(),
    );
    item(
        &mut items,
        b"asal",
        now.album.as_deref().unwrap_or_default().as_bytes(),
    );
    let mut out = Vec::with_capacity(items.len() + 8);
    item(&mut out, b"mlit", &items);
    out
}

pub(crate) fn progress(start: u32, current: u32, end: u32) -> String {
    format!("progress: {start}/{current}/{end}\r\n")
}

pub(crate) fn remote_command(body: &[u8]) -> Result<RemoteCommand, String> {
    let event = Value::from_reader(Cursor::new(body))
        .ok()
        .and_then(Value::into_dictionary)
        .ok_or_else(|| "not a plist".to_string())?;
    let text = |key: &str| {
        event
            .get(key)
            .and_then(Value::as_string)
            .unwrap_or_default()
    };
    if text("type") != "sendMediaRemoteCommand" {
        return Err(format!("event type {:?}", text("type")));
    }
    match text("value") {
        "play" => Ok(RemoteCommand::Play),
        "paus" | "stop" => Ok(RemoteCommand::Pause),
        "plps" => Ok(RemoteCommand::PlayPause),
        "nitm" => Ok(RemoteCommand::Next),
        "pitm" => Ok(RemoteCommand::Previous),
        other => Err(format!("command {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dmap_item_lists_title_artist_and_album() {
        let now = NowPlaying {
            title: "Tarantula".into(),
            artist: Some("Gorillaz".into()),
            album: None,
            cover: None,
        };
        let bytes = dmap(&now);
        assert_eq!(&bytes[..4], b"mlit");
        assert_eq!(
            u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize,
            bytes.len() - 8
        );
        assert_eq!(&bytes[8..17], b"mikd\0\0\0\x01\x02");
        assert_eq!(&bytes[17..21], b"minm");
        assert_eq!(u32::from_be_bytes(bytes[21..25].try_into().unwrap()), 9);
        assert_eq!(&bytes[25..34], b"Tarantula");
        assert_eq!(&bytes[34..42], b"asar\0\0\0\x08");
        assert_eq!(&bytes[42..50], b"Gorillaz");
        assert_eq!(&bytes[50..], b"asal\0\0\0\0");
    }

    fn event(kind: &str, value: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        Value::Dictionary(
            [
                ("type".to_string(), Value::String(kind.into())),
                ("value".to_string(), Value::String(value.into())),
            ]
            .into_iter()
            .collect(),
        )
        .to_writer_binary(&mut bytes)
        .unwrap();
        bytes
    }

    #[test]
    fn dacp_paths_name_remote_commands() {
        assert_eq!(
            dacp_command("/ctrl-int/1/playpause"),
            Some(RemoteCommand::PlayPause)
        );
        assert_eq!(
            dacp_command("/ctrl-int/1/nextitem"),
            Some(RemoteCommand::Next)
        );
        assert_eq!(
            dacp_command("/ctrl-int/1/previtem"),
            Some(RemoteCommand::Previous)
        );
        assert_eq!(dacp_command("/ctrl-int/1/play"), Some(RemoteCommand::Play));
        assert_eq!(
            dacp_command("/ctrl-int/1/pause"),
            Some(RemoteCommand::Pause)
        );
        assert_eq!(
            dacp_command("/ctrl-int/1/discrete-pause"),
            Some(RemoteCommand::Pause)
        );
        assert_eq!(
            dacp_command("/ctrl-int/1/setproperty?dmcp.device-volume=-15.000000"),
            None
        );
        assert_eq!(dacp_command("/server-info"), None);
    }

    #[test]
    fn remote_commands_are_read_from_media_remote_events() {
        let command = |value| remote_command(&event("sendMediaRemoteCommand", value));
        assert_eq!(command("play"), Ok(RemoteCommand::Play));
        assert_eq!(command("paus"), Ok(RemoteCommand::Pause));
        assert_eq!(command("nitm"), Ok(RemoteCommand::Next));
        assert_eq!(command("pitm"), Ok(RemoteCommand::Previous));
        assert_eq!(command("plps"), Ok(RemoteCommand::PlayPause));
        assert!(command("skpf").is_err());
        assert!(remote_command(&event("updateInfo", "play")).is_err());
        assert!(remote_command(b"garbage").is_err());
    }
}
