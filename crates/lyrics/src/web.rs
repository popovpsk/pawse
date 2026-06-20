use anyhow::{Context as _, Result};
use serde::Deserialize;
use std::time::Duration;

const USER_AGENT: &str = "pawse music player (https://github.com/popovpsk/pawse)";
const GET_URL: &str = "https://lrclib.net/api/get";
const SEARCH_URL: &str = "https://lrclib.net/api/search";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LyricsQuery {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub duration_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteLyrics {
    pub synced: Option<String>,
    pub plain: Option<String>,
}

#[derive(Deserialize)]
struct ApiLyrics {
    #[serde(default)]
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(default)]
    #[serde(rename = "plainLyrics")]
    plain_lyrics: Option<String>,
}

pub fn fetch(q: &LyricsQuery) -> Result<Option<RemoteLyrics>> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(10))
        .build();

    if let Some(found) = get_exact(&agent, q, true)? {
        return Ok(Some(found));
    }
    if q.album.is_some()
        && let Some(found) = get_exact(&agent, q, false)?
    {
        return Ok(Some(found));
    }
    search(&agent, q)
}

fn get_exact(
    agent: &ureq::Agent,
    q: &LyricsQuery,
    with_album: bool,
) -> Result<Option<RemoteLyrics>> {
    let mut req = agent
        .get(GET_URL)
        .set("User-Agent", USER_AGENT)
        .query("artist_name", &q.artist)
        .query("track_name", &q.title);
    if with_album && let Some(album) = &q.album {
        req = req.query("album_name", album);
    }
    if let Some(duration) = q.duration_secs {
        req = req.query("duration", &duration.to_string());
    }

    match req.call() {
        Ok(response) => {
            let body = response
                .into_string()
                .context("reading LRCLIB get response")?;
            Ok(parse_response(&body))
        }
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(e) => Err(anyhow::Error::new(e).context("LRCLIB get request failed")),
    }
}

fn search(agent: &ureq::Agent, q: &LyricsQuery) -> Result<Option<RemoteLyrics>> {
    let query = format!("{} {}", q.artist, q.title);
    let req = agent
        .get(SEARCH_URL)
        .set("User-Agent", USER_AGENT)
        .query("q", query.trim())
        .query("track_name", &q.title)
        .query("artist_name", &q.artist);

    match req.call() {
        Ok(response) => {
            let body = response
                .into_string()
                .context("reading LRCLIB search response")?;
            Ok(parse_search_response(&body))
        }
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(e) => Err(anyhow::Error::new(e).context("LRCLIB search request failed")),
    }
}

fn parse_response(body: &str) -> Option<RemoteLyrics> {
    let api: ApiLyrics = serde_json::from_str(body).ok()?;
    into_remote(api)
}

fn parse_search_response(body: &str) -> Option<RemoteLyrics> {
    let results: Vec<ApiLyrics> = serde_json::from_str(body).ok()?;
    results.into_iter().find_map(into_remote)
}

fn into_remote(api: ApiLyrics) -> Option<RemoteLyrics> {
    let synced = non_empty(api.synced_lyrics);
    let plain = non_empty(api.plain_lyrics);
    if synced.is_none() && plain.is_none() {
        None
    } else {
        Some(RemoteLyrics { synced, plain })
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_response_with_both() {
        let body = r#"{"syncedLyrics":"[00:01.00]hi","plainLyrics":"hi"}"#;
        let parsed = parse_response(body).unwrap();
        assert_eq!(parsed.synced.as_deref(), Some("[00:01.00]hi"));
        assert_eq!(parsed.plain.as_deref(), Some("hi"));
    }

    #[test]
    fn parse_response_synced_only() {
        let body = r#"{"syncedLyrics":"[00:01.00]hi","plainLyrics":null}"#;
        let parsed = parse_response(body).unwrap();
        assert_eq!(parsed.synced.as_deref(), Some("[00:01.00]hi"));
        assert_eq!(parsed.plain, None);
    }

    #[test]
    fn parse_response_plain_only() {
        let body = r#"{"syncedLyrics":null,"plainLyrics":"hello world"}"#;
        let parsed = parse_response(body).unwrap();
        assert_eq!(parsed.synced, None);
        assert_eq!(parsed.plain.as_deref(), Some("hello world"));
    }

    #[test]
    fn parse_response_both_null_is_none() {
        let body = r#"{"syncedLyrics":null,"plainLyrics":null}"#;
        assert_eq!(parse_response(body), None);
    }

    #[test]
    fn parse_response_empty_strings_are_none() {
        let body = r#"{"syncedLyrics":"","plainLyrics":"   "}"#;
        assert_eq!(parse_response(body), None);
    }

    #[test]
    fn parse_response_missing_fields() {
        let body = r#"{"id":123,"name":"Song"}"#;
        assert_eq!(parse_response(body), None);
    }

    #[test]
    fn parse_search_response_takes_first_with_lyrics() {
        let body = r#"[
            {"syncedLyrics":null,"plainLyrics":null},
            {"syncedLyrics":"[00:02.00]second","plainLyrics":"second"},
            {"syncedLyrics":"[00:03.00]third","plainLyrics":"third"}
        ]"#;
        let parsed = parse_search_response(body).unwrap();
        assert_eq!(parsed.synced.as_deref(), Some("[00:02.00]second"));
        assert_eq!(parsed.plain.as_deref(), Some("second"));
    }

    #[test]
    fn parse_search_response_empty_list_is_none() {
        assert_eq!(parse_search_response("[]"), None);
    }
}
