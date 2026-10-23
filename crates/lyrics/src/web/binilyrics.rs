use anyhow::{Context as _, Result};
use serde::Deserialize;

use super::{Candidate, LyricsQuery, http, pick, ttml};

pub(super) const NAME: &str = "binilyrics";
const SEARCH_URL: &str = "https://lyrics-api.binimum.org/";
const ATTEMPTS: u32 = 2;

#[derive(Deserialize)]
struct Found {
    #[serde(default)]
    results: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
    #[serde(default)]
    track_name: String,
    #[serde(default)]
    artist_name: String,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    timing_type: String,
    #[serde(default, rename = "lyricsUrl")]
    lyrics_url: Option<String>,
}

pub(super) fn candidates(session: &http::Session, q: &LyricsQuery) -> Result<Vec<Candidate>> {
    let album = q.album.as_deref().filter(|album| !album.is_empty());
    let mut hit = search(session, q, album)?;
    if hit.is_none() && album.is_some() {
        hit = search(session, q, None)?;
    }
    let Some(hit) = hit else {
        return Ok(Vec::new());
    };
    let Some(url) = hit.lyrics_url.as_deref() else {
        return Ok(Vec::new());
    };
    let Some(sheet) = sheet(session, url)? else {
        return Ok(Vec::new());
    };
    let text = ttml::to_lrc(&sheet).context("BiniLyrics sent a sheet that is not TTML")?;
    Ok(vec![Candidate {
        provider: NAME,
        title: hit.track_name,
        artist: hit.artist_name,
        duration_secs: hit.duration,
        text,
    }])
}

fn search(session: &http::Session, q: &LyricsQuery, album: Option<&str>) -> Result<Option<Hit>> {
    let mut response = session
        .call(ATTEMPTS, || {
            let mut req = session
                .get(SEARCH_URL)
                .query("track", &q.title)
                .query("artist", &q.artist);
            if let Some(album) = album {
                req = req.query("album", album);
            }
            if let Some(duration) = q.duration_secs {
                req = req.query("duration", duration.to_string());
            }
            req.call()
        })
        .context("BiniLyrics search request failed")?;
    match response.status().as_u16() {
        404 => Ok(None),
        code if !(200..300).contains(&code) => {
            anyhow::bail!("BiniLyrics search request failed: HTTP {code}")
        }
        _ => best_hit(&http::body(&mut response, "BiniLyrics search")?, q),
    }
}

fn sheet(session: &http::Session, url: &str) -> Result<Option<String>> {
    let mut response = session
        .call(ATTEMPTS, || session.get(url).call())
        .context("BiniLyrics sheet request failed")?;
    match response.status().as_u16() {
        404 => Ok(None),
        code if !(200..300).contains(&code) => {
            anyhow::bail!("BiniLyrics sheet request failed: HTTP {code}")
        }
        _ => http::body(&mut response, "BiniLyrics sheet").map(Some),
    }
}

fn best_hit(body: &str, q: &LyricsQuery) -> Result<Option<Hit>> {
    let found: Found =
        serde_json::from_str(body).context("BiniLyrics search answer is not JSON")?;
    Ok(found
        .results
        .into_iter()
        .filter(|hit| {
            hit.lyrics_url
                .as_deref()
                .is_some_and(|url| url.starts_with("https://"))
                && pick::fits(q, &hit.track_name, &hit.artist_name, hit.duration)
        })
        .min_by_key(|hit| (timing_rank(&hit.timing_type), pick::drift(q, hit.duration))))
}

fn timing_rank(timing: &str) -> u8 {
    match timing {
        "word" | "syllable" => 0,
        "line" => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query() -> LyricsQuery {
        LyricsQuery {
            artist: "Band".to_string(),
            title: "Song".to_string(),
            album: None,
            duration_secs: Some(200),
        }
    }

    fn picked(body: &str) -> Option<String> {
        best_hit(body, &query())
            .unwrap()
            .and_then(|hit| hit.lyrics_url)
    }

    #[test]
    fn word_timing_wins_then_the_closest_duration() {
        let body = r#"{"results":[
            {"track_name":"Song","artist_name":"BAND","duration":200,"timing_type":"line","lyricsUrl":"https://x/line.ttml"},
            {"track_name":"Song","artist_name":"Band","duration":208,"timing_type":"word","lyricsUrl":"https://x/far.ttml"},
            {"track_name":"Song","artist_name":"Band","duration":201,"timing_type":"syllable","lyricsUrl":"https://x/near.ttml"}
        ]}"#;
        assert_eq!(picked(body).as_deref(), Some("https://x/near.ttml"));
    }

    #[test]
    fn other_tracks_and_insecure_sheets_are_skipped() {
        let body = r#"{"results":[
            {"track_name":"Song (Live)","artist_name":"Band","duration":260,"timing_type":"word","lyricsUrl":"https://x/live.ttml"},
            {"track_name":"Song","artist_name":"Other","duration":200,"timing_type":"word","lyricsUrl":"https://x/other.ttml"},
            {"track_name":"Song","artist_name":"Band","duration":200,"timing_type":"word","lyricsUrl":"http://x/plain.ttml"},
            {"track_name":"Song","artist_name":"Band","duration":200,"timing_type":"none"}
        ]}"#;
        assert_eq!(picked(body), None);
        assert_eq!(picked(r#"{"results":[]}"#), None);
        assert!(best_hit("<html>challenge</html>", &query()).is_err());
    }
}
