use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::store::Love;
use crate::{NowPlaying, Scrobble};

const SERVER_PREFIX: &str = "server:";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetId {
    Lastfm,
    Librefm,
    ListenBrainz,
    CsvLog,
    Server(i64),
}

impl TargetId {
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "lastfm" => Some(TargetId::Lastfm),
            "librefm" => Some(TargetId::Librefm),
            "listen_brainz" => Some(TargetId::ListenBrainz),
            "csv_log" => Some(TargetId::CsvLog),
            _ => key
                .strip_prefix(SERVER_PREFIX)
                .and_then(|id| id.parse().ok())
                .map(TargetId::Server),
        }
    }

    pub fn key(self) -> Cow<'static, str> {
        match self {
            TargetId::Lastfm => Cow::Borrowed("lastfm"),
            TargetId::Librefm => Cow::Borrowed("librefm"),
            TargetId::ListenBrainz => Cow::Borrowed("listen_brainz"),
            TargetId::CsvLog => Cow::Borrowed("csv_log"),
            TargetId::Server(source_id) => Cow::Owned(format!("{SERVER_PREFIX}{source_id}")),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TargetId::Lastfm => "last.fm",
            TargetId::Librefm => "libre.fm",
            TargetId::ListenBrainz => "listenbrainz",
            TargetId::CsvLog => "csv",
            TargetId::Server(_) => "server",
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

    fn accepts_scrobbles(&self) -> bool {
        true
    }

    fn rewrites(&self) -> bool {
        true
    }

    fn now_playing_track(
        &self,
        now_playing: &NowPlaying,
        _track_id: Option<i64>,
    ) -> Result<(), SubmitError> {
        self.now_playing(now_playing)
    }

    fn submit_tracks(
        &self,
        items: &[Scrobble],
        _track_ids: &[Option<i64>],
    ) -> Result<(), SubmitError> {
        self.submit(items)
    }

    fn love_track(&self, love: &Love) -> Result<(), SubmitError> {
        self.love(&love.artist, &love.title, love.loved, love.at)
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

    #[test]
    fn a_server_target_round_trips_through_its_key() {
        let target = TargetId::Server(12);
        assert_eq!(target.key(), "server:12");
        assert_eq!(TargetId::from_key(&target.key()), Some(target));
        assert_eq!(TargetId::from_key("server:"), None);
        assert_eq!(TargetId::from_key("server:x"), None);
    }
}
