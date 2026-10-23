mod binilyrics;
mod elrc;
mod http;
mod lrclib;
mod lrcmux;
mod pick;
mod ttml;

use anyhow::Result;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LyricsQuery {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub duration_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebLyrics {
    pub text: String,
    pub provider: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
struct Candidate {
    provider: &'static str,
    title: String,
    artist: String,
    duration_secs: Option<f64>,
    text: String,
}

type Provider = fn(&http::Session, &LyricsQuery) -> Result<Vec<Candidate>>;

const BUDGET: Duration = Duration::from_secs(30);

const PROVIDERS: [(&str, Provider); 3] = [
    (binilyrics::NAME, binilyrics::candidates),
    (lrclib::NAME, lrclib::candidates),
    (lrcmux::NAME, lrcmux::candidates),
];

pub fn fetch(q: &LyricsQuery) -> Result<Option<WebLyrics>> {
    let session = http::Session::new(BUDGET);
    let mut found: Vec<pick::Scored> = Vec::new();
    let mut failure: Option<anyhow::Error> = None;
    for (order, (name, provider)) in PROVIDERS.iter().enumerate() {
        if session.expired() {
            log::warn!("lyrics: out of time before {name}");
            failure.get_or_insert_with(|| anyhow::anyhow!("lyrics lookup ran out of time"));
            break;
        }
        match provider(&session, q) {
            Ok(candidates) => found.extend(
                candidates
                    .into_iter()
                    .filter_map(|candidate| pick::Scored::new(q, candidate, order)),
            ),
            Err(e) => {
                log::warn!("lyrics: {name} did not answer: {e:#}");
                failure.get_or_insert(e);
            }
        }
        if pick::settled(&found) {
            break;
        }
    }
    match (pick::into_best(found), failure) {
        (Some(best), _) => Ok(Some(WebLyrics {
            text: best.text,
            provider: best.provider,
        })),
        (None, Some(e)) => Err(e),
        (None, None) => Ok(None),
    }
}
