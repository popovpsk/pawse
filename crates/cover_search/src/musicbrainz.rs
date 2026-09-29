use serde::Deserialize;
use ureq::Agent;

use crate::candidate::Candidate;
use crate::http::{Error, get_text};

const SEARCH_URL: &str = "https://musicbrainz.org/ws/2/release-group/";

#[derive(Deserialize)]
struct Response {
    #[serde(default, rename = "release-groups")]
    release_groups: Vec<ReleaseGroup>,
}

#[derive(Deserialize)]
struct ReleaseGroup {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default, rename = "artist-credit")]
    artist_credit: Vec<Credit>,
}

#[derive(Deserialize)]
struct Credit {
    #[serde(default)]
    name: String,
    #[serde(default)]
    joinphrase: String,
}

pub fn search(
    agent: &Agent,
    artist: &str,
    album: &str,
    limit: usize,
) -> Result<Vec<Candidate>, Error> {
    let body = get_text(
        agent
            .get(SEARCH_URL)
            .query("query", query(artist, album))
            .query("fmt", "json")
            .query("limit", limit.to_string()),
    )?;
    parse(&body)
}

pub fn query(artist: &str, album: &str) -> String {
    format!(
        "releasegroup:{} AND artist:{}",
        phrase(album),
        phrase(artist)
    )
}

fn phrase(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

pub fn parse(body: &str) -> Result<Vec<Candidate>, Error> {
    let response: Response = serde_json::from_str(body).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(response
        .release_groups
        .into_iter()
        .filter(|group| !group.id.is_empty())
        .map(|group| {
            let artist = group
                .artist_credit
                .iter()
                .map(|credit| format!("{}{}", credit.name, credit.joinphrase))
                .collect::<String>();
            Candidate::release_group(artist, group.title, group.id)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Source;

    #[test]
    fn joins_the_artist_credit() {
        let body = r#"{"created":"x","count":1,"offset":0,"release-groups":[{
            "id":"rg-1","score":100,"title":"Watch the Throne",
            "artist-credit":[
                {"name":"JAY-Z","joinphrase":" & ","artist":{"id":"a1","name":"JAY-Z"}},
                {"name":"Kanye West","artist":{"id":"a2","name":"Kanye West"}}
            ]}]}"#;
        let found = parse(body).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].source, Source::MusicBrainz);
        assert_eq!(found[0].artist, "JAY-Z & Kanye West");
        assert_eq!(found[0].album, "Watch the Throne");
        assert_eq!(
            found[0].art_url(1200),
            "https://coverartarchive.org/release-group/rg-1/front-1200"
        );
    }

    #[test]
    fn empty_response_is_a_miss() {
        assert!(parse(r#"{"release-groups":[]}"#).unwrap().is_empty());
    }

    #[test]
    fn invalid_body_is_a_failure() {
        assert!(matches!(parse("Rate limit"), Err(Error::Parse(_))));
    }

    #[test]
    fn query_quotes_and_escapes_phrases() {
        assert_eq!(
            query(r#"AC/DC"#, r#"Say "Hi" \ Bye"#),
            r#"releasegroup:"Say \"Hi\" \\ Bye" AND artist:"AC/DC""#
        );
    }
}
