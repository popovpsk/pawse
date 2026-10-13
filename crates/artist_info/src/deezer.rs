use cover_search::{Error, get_text};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use ureq::Agent;

use crate::{Candidate, TitlePage};

const ROOT: &str = "https://api.deezer.com";
const PLACEHOLDER_MARK: &str = "/images/artist//";
pub const ALBUM_PAGE: usize = 100;
const NO_DATA: i64 = 800;

#[derive(Deserialize)]
struct ArtistResponse {
    #[serde(default)]
    nb_fan: u64,
    #[serde(default)]
    picture_big: Option<String>,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ListResponse<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
    #[serde(default)]
    total: usize,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct SearchArtist {
    id: u64,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct Album {
    #[serde(default)]
    title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub fans: u64,
    pub picture: Option<String>,
}

pub fn page(agent: &Agent, artist_id: &str) -> Result<Page, Error> {
    let body = get_text(agent.get(format!("{ROOT}/artist/{artist_id}")))?;
    parse_page(&body)
}

pub fn search(agent: &Agent, name: &str, limit: usize) -> Result<Vec<Candidate>, Error> {
    let body = get_text(
        agent
            .get(format!("{ROOT}/search/artist"))
            .query("q", name)
            .query("limit", limit.to_string()),
    )?;
    parse_search(&body)
}

pub fn album_titles(agent: &Agent, artist_id: &str, offset: usize) -> Result<TitlePage, Error> {
    let body = get_text(
        agent
            .get(format!("{ROOT}/artist/{artist_id}/albums"))
            .query("index", offset.to_string())
            .query("limit", ALBUM_PAGE.to_string()),
    )?;
    parse_album_titles(&body)
}

pub fn parse_page(body: &str) -> Result<Page, Error> {
    let response: ArtistResponse = from_json(body)?;
    check(response.error)?;
    Ok(Page {
        fans: response.nb_fan,
        picture: response
            .picture_big
            .filter(|url| !url.is_empty() && !url.contains(PLACEHOLDER_MARK)),
    })
}

pub fn parse_search(body: &str) -> Result<Vec<Candidate>, Error> {
    let response: ListResponse<SearchArtist> = parse_list(body)?;
    Ok(response
        .data
        .into_iter()
        .map(|artist| Candidate {
            id: artist.id.to_string(),
            name: artist.name,
            aliases: Vec::new(),
        })
        .collect())
}

pub fn parse_album_titles(body: &str) -> Result<TitlePage, Error> {
    let response: ListResponse<Album> = parse_list(body)?;
    Ok(TitlePage {
        titles: response.data.into_iter().map(|album| album.title).collect(),
        total: response.total,
    })
}

fn parse_list<T: DeserializeOwned>(body: &str) -> Result<ListResponse<T>, Error> {
    let mut response: ListResponse<T> = from_json(body)?;
    check(response.error.take())?;
    Ok(response)
}

fn from_json<T: DeserializeOwned>(body: &str) -> Result<T, Error> {
    serde_json::from_str(body).map_err(|e| Error::Parse(e.to_string()))
}

fn check(error: Option<ApiError>) -> Result<(), Error> {
    match error {
        Some(error) if error.code != NO_DATA => Err(Error::Parse(format!(
            "deezer error {}: {}",
            error.code, error.message
        ))),
        _ => Ok(()),
    }
}

pub fn best_picture(pages: Vec<Page>) -> Option<String> {
    pages
        .into_iter()
        .filter(|page| page.picture.is_some())
        .max_by_key(|page| page.fans)
        .and_then(|page| page.picture)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(fans: u64, picture: Option<&str>) -> Page {
        Page {
            fans,
            picture: picture.map(str::to_string),
        }
    }

    #[test]
    fn reads_fans_and_the_big_picture() {
        let body = r#"{"id":464,"name":"Rammstein","nb_fan":3204381,
            "picture_big":"https://cdn-images.dzcdn.net/images/artist/f22cac6a41e838f54d2c7b4ea47b5f94/500x500-000000-80-0-0.jpg"}"#;
        assert_eq!(
            parse_page(body).unwrap(),
            page(
                3204381,
                Some(
                    "https://cdn-images.dzcdn.net/images/artist/f22cac6a41e838f54d2c7b4ea47b5f94/500x500-000000-80-0-0.jpg"
                )
            )
        );
    }

    #[test]
    fn placeholder_picture_is_no_picture() {
        let body = r#"{"id":1477045,"nb_fan":8615,"picture_big":"https://cdn-images.dzcdn.net/images/artist//500x500-000000-80-0-0.jpg"}"#;
        assert_eq!(parse_page(body).unwrap(), page(8615, None));
    }

    #[test]
    fn error_object_is_no_picture() {
        let body = r#"{"error":{"type":"DataException","message":"no data","code":800}}"#;
        assert_eq!(parse_page(body).unwrap(), page(0, None));
    }

    #[test]
    fn a_page_without_picture_is_skipped() {
        let pages = vec![page(8615, None), page(5220311, Some("main"))];
        assert_eq!(best_picture(pages).as_deref(), Some("main"));
    }

    #[test]
    fn the_page_with_more_fans_wins() {
        let pages = vec![page(3987, Some("duplicate")), page(12248386, Some("main"))];
        assert_eq!(best_picture(pages).as_deref(), Some("main"));
    }

    #[test]
    fn search_lists_artists() {
        let body = r#"{"data":[
            {"id":74400,"name":"ASHES dIVIDE","nb_album":1,"nb_fan":4491,"type":"artist"},
            {"id":9694872,"name":"As We Divide","nb_album":12,"nb_fan":52,"type":"artist"}],
            "total":2}"#;
        let found = parse_search(body).unwrap();
        assert_eq!(
            found
                .iter()
                .map(|c| (c.id.as_str(), c.name.as_str()))
                .collect::<Vec<_>>(),
            vec![("74400", "ASHES dIVIDE"), ("9694872", "As We Divide")]
        );
        assert!(found[0].aliases.is_empty());
    }

    #[test]
    fn album_titles_are_listed() {
        let body = r#"{"data":[{"id":109077,"title":"Keep Telling Myself It's Alright",
            "record_type":"album"}],"total":1}"#;
        let page = parse_album_titles(body).unwrap();
        assert_eq!(page.titles, vec!["Keep Telling Myself It's Alright"]);
        assert_eq!(page.total, 1);
    }

    #[test]
    fn an_empty_list_is_not_an_error() {
        let no_data = r#"{"error":{"type":"DataException","message":"no data","code":800}}"#;
        for body in [r#"{"data":[],"total":0}"#, no_data] {
            let page = parse_album_titles(body).unwrap();
            assert!(page.titles.is_empty());
            assert_eq!(page.total, 0);
        }
    }

    #[test]
    fn other_errors_are_failures() {
        let body = r#"{"error":{"type":"Exception","message":"Quota limit exceeded","code":4}}"#;
        assert!(matches!(parse_page(body), Err(Error::Parse(_))));
        assert!(matches!(parse_search(body), Err(Error::Parse(_))));
        assert!(matches!(parse_album_titles(body), Err(Error::Parse(_))));
    }

    #[test]
    fn no_pictures_no_photo() {
        assert_eq!(best_picture(vec![page(10, None)]), None);
        assert_eq!(best_picture(Vec::new()), None);
    }
}
