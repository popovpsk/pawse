mod accumulator;
mod queue;
mod target;
mod targets;
mod worker;

pub use accumulator::{PlayAccumulator, should_scrobble};
pub use target::{ScrobbleTarget, SubmitError, TargetId};
pub use targets::audioscrobbler::{AudioscrobblerClient, LovedTrack, Profile, SessionError};
pub use targets::csv_log::CsvLog;
pub use targets::listenbrainz::{DEFAULT_ROOT as LISTENBRAINZ_ROOT, ListenBrainzClient};
pub use worker::{ScrobbleHandle, StatusEvent};

use serde::{Deserialize, Serialize};

pub fn creds() -> Option<(String, String)> {
    let key = std::env::var("LASTFM_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| option_env!("LASTFM_API_KEY").map(str::to_owned))?;
    let secret = std::env::var("LASTFM_API_SECRET")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| option_env!("LASTFM_API_SECRET").map(str::to_owned))?;
    Some((key, secret))
}

pub fn is_available() -> bool {
    creds().is_some()
}

pub fn lastfm_client() -> Option<AudioscrobblerClient> {
    let (key, secret) = creds()?;
    Some(AudioscrobblerClient::new(Profile::lastfm(key, secret)))
}

pub fn librefm_client() -> AudioscrobblerClient {
    AudioscrobblerClient::new(Profile::librefm())
}

pub fn primary_artist(artists: &[String], first_only: bool) -> Option<String> {
    let cleaned: Vec<&str> = artists
        .iter()
        .map(|a| a.trim())
        .filter(|a| !a.is_empty())
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    if first_only {
        Some(cleaned[0].to_string())
    } else {
        Some(cleaned.join(", "))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub key: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NowPlaying {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_number: Option<u32>,
    pub duration_secs: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scrobble {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    #[serde(default)]
    pub album_artist: Option<String>,
    #[serde(default)]
    pub track_number: Option<u32>,
    pub duration_secs: Option<u64>,
    pub timestamp: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn single_artist_is_returned_as_is() {
        assert_eq!(
            primary_artist(&names(&["Boards of Canada"]), true).as_deref(),
            Some("Boards of Canada")
        );
    }

    #[test]
    fn first_only_drops_the_rest() {
        assert_eq!(
            primary_artist(&names(&["A", "B", "C"]), true).as_deref(),
            Some("A")
        );
    }

    #[test]
    fn joined_mode_keeps_every_artist() {
        assert_eq!(
            primary_artist(&names(&["A", "B"]), false).as_deref(),
            Some("A, B")
        );
    }

    #[test]
    fn blank_entries_are_skipped() {
        assert_eq!(
            primary_artist(&names(&["  ", "B"]), true).as_deref(),
            Some("B")
        );
        assert_eq!(primary_artist(&names(&["", "  "]), true), None);
        assert_eq!(primary_artist(&[], true), None);
    }
}
