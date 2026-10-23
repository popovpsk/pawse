use anyhow::{Context as _, Result};
use serde::Deserialize;

use super::{Candidate, LyricsQuery, http};

pub(super) const NAME: &str = "lrclib";
const GET_URL: &str = "https://lrclib.net/api/get";
const SEARCH_URL: &str = "https://lrclib.net/api/search";
const ATTEMPTS: u32 = 5;

#[derive(Deserialize)]
struct ApiLyrics {
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default, rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(default, rename = "plainLyrics")]
    plain_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

#[derive(Deserialize)]
struct ApiSearchHit {
    #[serde(default, rename = "trackName")]
    track_name: String,
    #[serde(default, rename = "artistName")]
    artist_name: String,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default, rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(default, rename = "plainLyrics")]
    plain_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

enum GetResult {
    Lyrics(Vec<Candidate>),
    NoLyrics,
    NotFound,
}

pub(super) fn candidates(session: &http::Session, q: &LyricsQuery) -> Result<Vec<Candidate>> {
    match get_exact(session, q, true)? {
        GetResult::Lyrics(found) => return Ok(found),
        GetResult::NoLyrics => return Ok(Vec::new()),
        GetResult::NotFound => {}
    }
    if album(q).is_some() {
        match get_exact(session, q, false)? {
            GetResult::Lyrics(found) => return Ok(found),
            GetResult::NoLyrics => return Ok(Vec::new()),
            GetResult::NotFound => {}
        }
    }
    search(session, q)
}

fn get_exact(session: &http::Session, q: &LyricsQuery, with_album: bool) -> Result<GetResult> {
    let mut response = session
        .call(ATTEMPTS, || {
            let mut req = session
                .get(GET_URL)
                .query("artist_name", &q.artist)
                .query("track_name", &q.title);
            if with_album && let Some(album) = album(q) {
                req = req.query("album_name", album);
            }
            if let Some(duration) = q.duration_secs {
                req = req.query("duration", duration.to_string());
            }
            req.call()
        })
        .context("LRCLIB get request failed")?;

    match response.status().as_u16() {
        404 => Ok(GetResult::NotFound),
        code if !(200..300).contains(&code) => {
            Err(anyhow::anyhow!("LRCLIB get request failed: HTTP {code}"))
        }
        _ => parse_get(&http::body(&mut response, "LRCLIB get")?, q),
    }
}

fn search(session: &http::Session, q: &LyricsQuery) -> Result<Vec<Candidate>> {
    let mut response = session
        .call(ATTEMPTS, || {
            let mut req = session
                .get(SEARCH_URL)
                .query("track_name", &q.title)
                .query("artist_name", &q.artist);
            if let Some(album) = album(q) {
                req = req.query("album_name", album);
            }
            req.call()
        })
        .context("LRCLIB search request failed")?;

    match response.status().as_u16() {
        404 => Ok(Vec::new()),
        code if !(200..300).contains(&code) => {
            Err(anyhow::anyhow!("LRCLIB search request failed: HTTP {code}"))
        }
        _ => parse_search(&http::body(&mut response, "LRCLIB search")?),
    }
}

fn album(q: &LyricsQuery) -> Option<&str> {
    q.album.as_deref().filter(|album| !album.is_empty())
}

fn parse_get(body: &str, q: &LyricsQuery) -> Result<GetResult> {
    let api: ApiLyrics = serde_json::from_str(body).context("LRCLIB get answer is not JSON")?;
    if api.instrumental {
        return Ok(GetResult::NoLyrics);
    }
    let found: Vec<Candidate> = texts(api.synced_lyrics, api.plain_lyrics)
        .map(|text| Candidate {
            provider: NAME,
            title: q.title.clone(),
            artist: q.artist.clone(),
            duration_secs: api.duration,
            text,
        })
        .collect();
    Ok(if found.is_empty() {
        GetResult::NoLyrics
    } else {
        GetResult::Lyrics(found)
    })
}

fn parse_search(body: &str) -> Result<Vec<Candidate>> {
    let hits: Vec<ApiSearchHit> =
        serde_json::from_str(body).context("LRCLIB search answer is not JSON")?;
    Ok(hits
        .into_iter()
        .filter(|hit| !hit.instrumental)
        .flat_map(|hit| {
            let ApiSearchHit {
                track_name,
                artist_name,
                duration,
                synced_lyrics,
                plain_lyrics,
                ..
            } = hit;
            texts(synced_lyrics, plain_lyrics).map(move |text| Candidate {
                provider: NAME,
                title: track_name.clone(),
                artist: artist_name.clone(),
                duration_secs: duration,
                text,
            })
        })
        .collect())
}

fn texts(synced: Option<String>, plain: Option<String>) -> impl Iterator<Item = String> {
    [synced, plain]
        .into_iter()
        .flatten()
        .filter(|text| !text.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query() -> LyricsQuery {
        LyricsQuery {
            artist: "Band".to_string(),
            title: "Song".to_string(),
            album: None,
            duration_secs: Some(215),
        }
    }

    fn texts_of(found: &[Candidate]) -> Vec<&str> {
        found.iter().map(|c| c.text.as_str()).collect()
    }

    fn lyrics(result: GetResult) -> Vec<Candidate> {
        match result {
            GetResult::Lyrics(found) => found,
            _ => Vec::new(),
        }
    }

    #[test]
    fn get_offers_both_texts_for_the_asked_track() {
        let body = r#"{"duration":214.0,"syncedLyrics":"[00:01.00]hi","plainLyrics":"hi"}"#;
        let found = lyrics(parse_get(body, &query()).unwrap());
        assert_eq!(texts_of(&found), vec!["[00:01.00]hi", "hi"]);
        assert_eq!(
            (found[0].title.as_str(), found[0].artist.as_str()),
            ("Song", "Band")
        );
        assert_eq!(found[0].duration_secs, Some(214.0));

        let body = r#"{"syncedLyrics":null,"plainLyrics":"hello world"}"#;
        assert_eq!(
            texts_of(&lyrics(parse_get(body, &query()).unwrap())),
            vec!["hello world"]
        );
    }

    #[test]
    fn get_without_usable_lyrics_is_no_lyrics() {
        for body in [
            r#"{"syncedLyrics":null,"plainLyrics":null}"#,
            r#"{"syncedLyrics":"","plainLyrics":"   "}"#,
            r#"{"instrumental":true,"syncedLyrics":"[00:01.00]x","plainLyrics":"x"}"#,
        ] {
            assert!(
                matches!(parse_get(body, &query()).unwrap(), GetResult::NoLyrics),
                "{body}"
            );
        }
        assert!(parse_get("not json", &query()).is_err());
    }

    #[test]
    fn search_keeps_every_text_of_every_hit() {
        let body = r#"[
            {"trackName":"Vaeloth","artistName":"Quill Vesper","duration":554.0,
             "syncedLyrics":"[00:01.00]other","plainLyrics":"other"},
            {"trackName":"Hollow Tide","artistName":"Zephyrnauts","duration":300.0,
             "syncedLyrics":null,"plainLyrics":"right"},
            {"trackName":"Hollow Tide","artistName":"Zephyrnauts","duration":300.0,
             "instrumental":true,"syncedLyrics":null,"plainLyrics":null},
            {"trackName":"Hollow Tide","artistName":"Zephyrnauts","duration":300.0,
             "syncedLyrics":" ","plainLyrics":""}
        ]"#;
        let found = parse_search(body).unwrap();
        assert_eq!(texts_of(&found), vec!["[00:01.00]other", "other", "right"]);
        assert_eq!(found[2].duration_secs, Some(300.0));
        assert!(parse_search("[]").unwrap().is_empty());
        assert!(parse_search("oops").is_err());
    }
}
