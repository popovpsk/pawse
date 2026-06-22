use anyhow::{Context as _, Result};
use serde::Deserialize;
use std::time::Duration;

const USER_AGENT: &str = "pawse music player (https://github.com/popovpsk/pawse)";
const GET_URL: &str = "https://lrclib.net/api/get";
const SEARCH_URL: &str = "https://lrclib.net/api/search";
const DURATION_TOLERANCE_SECS: f64 = 5.0;
const MAX_ATTEMPTS: u32 = 5;
const RETRY_BACKOFF_MS: u64 = 250;
const RETRY_AFTER_CAP_SECS: u64 = 10;

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
    #[serde(default)]
    instrumental: bool,
}

#[derive(Deserialize)]
struct ApiSearchHit {
    #[serde(default)]
    #[serde(rename = "trackName")]
    track_name: String,
    #[serde(default)]
    #[serde(rename = "artistName")]
    artist_name: String,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(default)]
    #[serde(rename = "plainLyrics")]
    plain_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

enum GetResult {
    Lyrics(RemoteLyrics),
    NoLyrics,
    NotFound,
}

pub fn fetch(q: &LyricsQuery) -> Result<Option<RemoteLyrics>> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(10))
        .build();

    match get_exact(&agent, q, true)? {
        GetResult::Lyrics(found) => return Ok(Some(found)),
        GetResult::NoLyrics => return Ok(None),
        GetResult::NotFound => {}
    }
    if q.album.is_some() {
        match get_exact(&agent, q, false)? {
            GetResult::Lyrics(found) => return Ok(Some(found)),
            GetResult::NoLyrics => return Ok(None),
            GetResult::NotFound => {}
        }
    }
    search(&agent, q)
}

fn get_exact(agent: &ureq::Agent, q: &LyricsQuery, with_album: bool) -> Result<GetResult> {
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

    match call_with_retry(req) {
        Ok(response) => {
            let body = response
                .into_string()
                .context("reading LRCLIB get response")?;
            Ok(parse_get(&body))
        }
        Err(e) => match *e {
            ureq::Error::Status(404, _) => Ok(GetResult::NotFound),
            e => Err(anyhow::Error::new(e).context("LRCLIB get request failed")),
        },
    }
}

fn search(agent: &ureq::Agent, q: &LyricsQuery) -> Result<Option<RemoteLyrics>> {
    let mut req = agent
        .get(SEARCH_URL)
        .set("User-Agent", USER_AGENT)
        .query("track_name", &q.title)
        .query("artist_name", &q.artist);
    if let Some(album) = &q.album {
        req = req.query("album_name", album);
    }

    match call_with_retry(req) {
        Ok(response) => {
            let body = response
                .into_string()
                .context("reading LRCLIB search response")?;
            Ok(select_search_match(&body, q))
        }
        Err(e) => match *e {
            ureq::Error::Status(404, _) => Ok(None),
            e => Err(anyhow::Error::new(e).context("LRCLIB search request failed")),
        },
    }
}

fn call_with_retry(req: ureq::Request) -> Result<ureq::Response, Box<ureq::Error>> {
    let mut attempt = 0u32;
    loop {
        match req.clone().call() {
            Ok(response) => return Ok(response),
            Err(e) => {
                attempt += 1;
                if attempt >= MAX_ATTEMPTS || !is_retryable(&e) {
                    return Err(Box::new(e));
                }
                std::thread::sleep(retry_delay(&e));
            }
        }
    }
}

fn is_retryable(e: &ureq::Error) -> bool {
    match e {
        ureq::Error::Status(code, _) => retryable_status(*code),
        ureq::Error::Transport(_) => true,
    }
}

fn retryable_status(code: u16) -> bool {
    code == 429 || code >= 500
}

fn retry_delay(e: &ureq::Error) -> Duration {
    if let ureq::Error::Status(429, resp) = e
        && let Some(delay) = retry_after_delay(resp.header("retry-after"))
    {
        return delay;
    }
    Duration::from_millis(RETRY_BACKOFF_MS)
}

fn retry_after_delay(header: Option<&str>) -> Option<Duration> {
    header
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|secs| Duration::from_secs(secs.min(RETRY_AFTER_CAP_SECS)))
}

fn parse_get(body: &str) -> GetResult {
    let Ok(api) = serde_json::from_str::<ApiLyrics>(body) else {
        return GetResult::NoLyrics;
    };
    if api.instrumental {
        return GetResult::NoLyrics;
    }
    match into_remote(api.synced_lyrics, api.plain_lyrics) {
        Some(found) => GetResult::Lyrics(found),
        None => GetResult::NoLyrics,
    }
}

fn select_search_match(body: &str, q: &LyricsQuery) -> Option<RemoteLyrics> {
    let hits: Vec<ApiSearchHit> = serde_json::from_str(body).ok()?;
    let mut best: Option<(f64, RemoteLyrics)> = None;
    for hit in hits {
        if hit.instrumental
            || !text_matches(&hit.track_name, &q.title)
            || !text_matches(&hit.artist_name, &q.artist)
        {
            continue;
        }
        let delta = match (hit.duration, q.duration_secs) {
            (Some(d), Some(w)) => (d - w as f64).abs(),
            _ => f64::INFINITY,
        };
        if delta.is_finite() && delta > DURATION_TOLERANCE_SECS {
            continue;
        }
        let Some(found) = into_remote(hit.synced_lyrics, hit.plain_lyrics) else {
            continue;
        };
        if best.as_ref().is_none_or(|(bd, _)| delta < *bd) {
            best = Some((delta, found));
        }
    }
    best.map(|(_, found)| found)
}

fn text_matches(candidate: &str, wanted: &str) -> bool {
    let candidate = candidate.trim().to_lowercase();
    let wanted = wanted.trim().to_lowercase();
    if candidate.is_empty() || wanted.is_empty() {
        return false;
    }
    candidate == wanted || candidate.contains(&wanted) || wanted.contains(&candidate)
}

fn into_remote(synced: Option<String>, plain: Option<String>) -> Option<RemoteLyrics> {
    let synced = non_empty(synced);
    let plain = non_empty(plain);
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

    fn query(artist: &str, title: &str, duration_secs: Option<u64>) -> LyricsQuery {
        LyricsQuery {
            artist: artist.to_string(),
            title: title.to_string(),
            album: None,
            duration_secs,
        }
    }

    fn lyrics(result: GetResult) -> Option<RemoteLyrics> {
        match result {
            GetResult::Lyrics(found) => Some(found),
            _ => None,
        }
    }

    #[test]
    fn parse_get_with_both() {
        let body = r#"{"syncedLyrics":"[00:01.00]hi","plainLyrics":"hi"}"#;
        let parsed = lyrics(parse_get(body)).unwrap();
        assert_eq!(parsed.synced.as_deref(), Some("[00:01.00]hi"));
        assert_eq!(parsed.plain.as_deref(), Some("hi"));
    }

    #[test]
    fn parse_get_synced_only() {
        let body = r#"{"syncedLyrics":"[00:01.00]hi","plainLyrics":null}"#;
        let parsed = lyrics(parse_get(body)).unwrap();
        assert_eq!(parsed.synced.as_deref(), Some("[00:01.00]hi"));
        assert_eq!(parsed.plain, None);
    }

    #[test]
    fn parse_get_plain_only() {
        let body = r#"{"syncedLyrics":null,"plainLyrics":"hello world"}"#;
        let parsed = lyrics(parse_get(body)).unwrap();
        assert_eq!(parsed.synced, None);
        assert_eq!(parsed.plain.as_deref(), Some("hello world"));
    }

    #[test]
    fn parse_get_both_null_is_no_lyrics() {
        let body = r#"{"syncedLyrics":null,"plainLyrics":null}"#;
        assert!(matches!(parse_get(body), GetResult::NoLyrics));
    }

    #[test]
    fn parse_get_empty_strings_are_no_lyrics() {
        let body = r#"{"syncedLyrics":"","plainLyrics":"   "}"#;
        assert!(matches!(parse_get(body), GetResult::NoLyrics));
    }

    #[test]
    fn parse_get_instrumental_is_no_lyrics_even_with_text() {
        let body = r#"{"instrumental":true,"syncedLyrics":"[00:01.00]x","plainLyrics":"x"}"#;
        assert!(matches!(parse_get(body), GetResult::NoLyrics));
    }

    #[test]
    fn search_skips_wrong_artist_for_same_title() {
        let body = r#"[
            {"trackName":"Vaeloth","artistName":"Quill Vesper","duration":554.0,
             "syncedLyrics":"[00:01.00]wrong","plainLyrics":"wrong"}
        ]"#;
        assert_eq!(
            select_search_match(body, &query("Zephyrnauts", "Vaeloth", Some(215))),
            None
        );
    }

    #[test]
    fn search_skips_instrumental_match() {
        let body = r#"[
            {"trackName":"Vaeloth","artistName":"Zephyrnauts","duration":215.0,
             "instrumental":true,"syncedLyrics":null,"plainLyrics":null}
        ]"#;
        assert_eq!(
            select_search_match(body, &query("Zephyrnauts", "Vaeloth", Some(215))),
            None
        );
    }

    #[test]
    fn search_accepts_matching_artist_and_title() {
        let body = r#"[
            {"trackName":"Vaeloth","artistName":"Quill Vesper","duration":554.0,
             "syncedLyrics":"[00:01.00]wrong","plainLyrics":"wrong"},
            {"trackName":"Hollow Tide","artistName":"Zephyrnauts","duration":300.0,
             "syncedLyrics":"[00:02.00]right","plainLyrics":"right"}
        ]"#;
        let parsed =
            select_search_match(body, &query("Zephyrnauts", "Hollow Tide", Some(301))).unwrap();
        assert_eq!(parsed.plain.as_deref(), Some("right"));
    }

    #[test]
    fn search_prefers_closest_duration() {
        let body = r#"[
            {"trackName":"Song","artistName":"Band","duration":305.0,
             "syncedLyrics":null,"plainLyrics":"far"},
            {"trackName":"Song","artistName":"Band","duration":302.0,
             "syncedLyrics":null,"plainLyrics":"near"}
        ]"#;
        let parsed = select_search_match(body, &query("Band", "Song", Some(300))).unwrap();
        assert_eq!(parsed.plain.as_deref(), Some("near"));
    }

    #[test]
    fn search_rejects_same_artist_wrong_duration_overlapping_title() {
        let body = r#"[
            {"trackName":"Bright Vaeloth","artistName":"Zephyrnauts","duration":480.0,
             "syncedLyrics":null,"plainLyrics":"different song"}
        ]"#;
        assert_eq!(
            select_search_match(body, &query("Zephyrnauts", "Vaeloth", Some(215))),
            None
        );
    }

    #[test]
    fn search_allows_text_match_when_duration_unknown() {
        let body = r#"[
            {"trackName":"Song","artistName":"Band","duration":420.0,
             "syncedLyrics":null,"plainLyrics":"ok"}
        ]"#;
        let parsed = select_search_match(body, &query("Band", "Song", None)).unwrap();
        assert_eq!(parsed.plain.as_deref(), Some("ok"));
    }

    #[test]
    fn search_empty_list_is_none() {
        assert_eq!(select_search_match("[]", &query("a", "b", None)), None);
    }

    #[test]
    fn retryable_status_classification() {
        assert!(!retryable_status(404));
        assert!(!retryable_status(400));
        assert!(!retryable_status(200));
        assert!(retryable_status(429));
        assert!(retryable_status(500));
        assert!(retryable_status(503));
    }

    #[test]
    fn retry_after_delay_parses_clamps_and_rejects() {
        assert_eq!(retry_after_delay(None), None);
        assert_eq!(retry_after_delay(Some("nope")), None);
        assert_eq!(retry_after_delay(Some(" 3 ")), Some(Duration::from_secs(3)));
        assert_eq!(
            retry_after_delay(Some("999")),
            Some(Duration::from_secs(10))
        );
    }

    #[test]
    fn text_matches_handles_substrings_and_case() {
        assert!(text_matches("Zephyrnauts", "zephyrnauts"));
        assert!(text_matches("Zephyrnauts feat. Marn", "Zephyrnauts"));
        assert!(text_matches("Vaeloth", "vaeloth (remastered)"));
        assert!(!text_matches("Quill Vesper", "Zephyrnauts"));
        assert!(!text_matches("", "x"));
    }
}
