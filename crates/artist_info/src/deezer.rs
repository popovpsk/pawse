use cover_search::{Error, get_text};
use serde::Deserialize;
use ureq::Agent;

const PLACEHOLDER_MARK: &str = "/images/artist//";

#[derive(Deserialize)]
struct ArtistResponse {
    #[serde(default)]
    picture_big: Option<String>,
}

pub fn picture_url(agent: &Agent, artist_id: &str) -> Result<Option<String>, Error> {
    let body = get_text(agent.get(format!("https://api.deezer.com/artist/{artist_id}")))?;
    parse_picture(&body)
}

pub fn parse_picture(body: &str) -> Result<Option<String>, Error> {
    let response: ArtistResponse =
        serde_json::from_str(body).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(response
        .picture_big
        .filter(|url| !url.is_empty() && !url.contains(PLACEHOLDER_MARK)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_big_picture() {
        let body = r#"{"id":464,"name":"Rammstein",
            "picture_big":"https://cdn-images.dzcdn.net/images/artist/f22cac6a41e838f54d2c7b4ea47b5f94/500x500-000000-80-0-0.jpg"}"#;
        assert_eq!(
            parse_picture(body).unwrap().as_deref(),
            Some(
                "https://cdn-images.dzcdn.net/images/artist/f22cac6a41e838f54d2c7b4ea47b5f94/500x500-000000-80-0-0.jpg"
            )
        );
    }

    #[test]
    fn placeholder_picture_is_no_picture() {
        let body = r#"{"id":1477045,"picture_big":"https://cdn-images.dzcdn.net/images/artist//500x500-000000-80-0-0.jpg"}"#;
        assert_eq!(parse_picture(body).unwrap(), None);
    }

    #[test]
    fn error_object_is_no_picture() {
        let body = r#"{"error":{"type":"DataException","message":"no data","code":800}}"#;
        assert_eq!(parse_picture(body).unwrap(), None);
    }
}
