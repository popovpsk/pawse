use serde::{Deserialize, Serialize};

use crate::{NowPlaying, Scrobble};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetId {
    Lastfm,
    Librefm,
    ListenBrainz,
    CsvLog,
}

impl TargetId {
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "lastfm" => Some(TargetId::Lastfm),
            "librefm" => Some(TargetId::Librefm),
            "listen_brainz" => Some(TargetId::ListenBrainz),
            "csv_log" => Some(TargetId::CsvLog),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            TargetId::Lastfm => "lastfm",
            TargetId::Librefm => "librefm",
            TargetId::ListenBrainz => "listen_brainz",
            TargetId::CsvLog => "csv_log",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TargetId::Lastfm => "last.fm",
            TargetId::Librefm => "libre.fm",
            TargetId::ListenBrainz => "listenbrainz",
            TargetId::CsvLog => "csv",
        }
    }
}

#[derive(Debug)]
pub enum SubmitError {
    Transient(String),
    Permanent(String),
    Auth(String),
    Unsupported,
}

impl std::fmt::Display for SubmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubmitError::Transient(m) => write!(f, "transient: {m}"),
            SubmitError::Permanent(m) => write!(f, "permanent: {m}"),
            SubmitError::Auth(m) => write!(f, "auth: {m}"),
            SubmitError::Unsupported => write!(f, "unsupported"),
        }
    }
}

pub trait ScrobbleTarget: Send {
    fn id(&self) -> TargetId;

    fn max_batch(&self) -> usize;

    fn now_playing(&self, now_playing: &NowPlaying) -> Result<(), SubmitError>;

    fn submit(&self, items: &[Scrobble]) -> Result<(), SubmitError>;

    fn love(&self, artist: &str, title: &str, love: bool, at: u64) -> Result<(), SubmitError>;

    fn accepts_loves(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_key_round_trips_every_serialized_name() {
        for target in [
            TargetId::Lastfm,
            TargetId::Librefm,
            TargetId::ListenBrainz,
            TargetId::CsvLog,
        ] {
            let json = serde_json::to_string(&target).unwrap();
            let key = json.trim_matches('"');
            assert_eq!(TargetId::from_key(key), Some(target), "key {key}");
        }
        assert_eq!(TargetId::from_key("maloja"), None);
    }
}
