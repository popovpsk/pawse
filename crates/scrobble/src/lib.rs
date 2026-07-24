mod accumulator;
mod client;
mod queue;
mod worker;

pub use accumulator::{PlayAccumulator, should_scrobble};
pub use client::LastfmClient;
pub use worker::ScrobbleHandle;

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

pub fn client_from_creds() -> Option<LastfmClient> {
    let (key, secret) = creds()?;
    Some(LastfmClient::new(key, secret))
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
    pub duration_secs: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scrobble {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub duration_secs: Option<u64>,
    pub timestamp: u64,
}
