use serde::Deserialize;
use ureq::Agent;

use crate::candidate::Candidate;
use crate::http::{Error, get_text};

const SEARCH_URL: &str = "https://itunes.apple.com/search";

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    results: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    #[serde(default, rename = "artistName")]
    artist_name: Option<String>,
    #[serde(default, rename = "collectionName")]
    collection_name: Option<String>,
    #[serde(default, rename = "artworkUrl100")]
    artwork_url_100: Option<String>,
}

pub fn search(
    agent: &Agent,
    artist: &str,
    album: &str,
    limit: usize,
) -> Result<Vec<Candidate>, Error> {
    let term = format!("{artist} {album}");
    let body = get_text(
        agent
            .get(SEARCH_URL)
            .query("term", &term)
            .query("media", "music")
            .query("entity", "album")
            .query("limit", limit.to_string()),
    )?;
    parse(&body)
}

pub fn parse(body: &str) -> Result<Vec<Candidate>, Error> {
    let response: Response = serde_json::from_str(body).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(response
        .results
        .into_iter()
        .filter_map(|item| {
            let art = item.artwork_url_100.filter(|url| !url.is_empty())?;
            Some(Candidate::itunes(
                item.artist_name.unwrap_or_default(),
                item.collection_name.unwrap_or_default(),
                art,
            ))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Source;

    #[test]
    fn reads_artist_album_and_art() {
        let body = r#"{"resultCount":2,"results":[
            {"artistName":"Portishead","collectionName":"Dummy","artworkUrl100":"https://is1.mzstatic.com/a/100x100bb.jpg"},
            {"artistName":"Nobody","collectionName":"No Art"}
        ]}"#;
        let found = parse(body).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].source, Source::Itunes);
        assert_eq!(found[0].artist, "Portishead");
        assert_eq!(found[0].album, "Dummy");
        assert_eq!(
            found[0].art_url(512),
            "https://is1.mzstatic.com/a/512x512bb.jpg"
        );
    }

    #[test]
    fn empty_or_missing_results_are_a_definitive_miss() {
        assert!(parse(r#"{"results":[]}"#).unwrap().is_empty());
        assert!(parse("{}").unwrap().is_empty());
    }

    #[test]
    fn invalid_body_is_a_failure_not_a_miss() {
        assert!(matches!(
            parse("<html>throttled</html>"),
            Err(Error::Parse(_))
        ));
    }
}
