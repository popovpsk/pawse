use cover_search::{Error, get_text};
use serde::{Deserialize, Serialize};
use ureq::Agent;

const ROOT: &str = "https://musicbrainz.org/ws/2";
pub const RELEASE_GROUP_PAGE: usize = 100;
const GENRES_KEPT: usize = 5;
pub const VARIOUS_ARTISTS: &str = "89ad4ac3-39f7-470e-963a-56509c546377";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Kind {
    Person,
    Group,
    #[default]
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtistFacts {
    pub mbid: String,
    pub kind: Kind,
    pub begin_year: Option<i32>,
    pub end_year: Option<i32>,
    pub ended: bool,
    pub area: Option<String>,
    pub begin_area: Option<String>,
    pub genres: Vec<String>,
    pub members: Vec<Membership>,
    pub member_of: Vec<Membership>,
    pub deezer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Membership {
    pub name: String,
    pub current: bool,
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    artists: Vec<SearchArtist>,
}

#[derive(Deserialize)]
struct SearchArtist {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    aliases: Vec<Named>,
}

#[derive(Deserialize)]
struct Named {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseGroupPage {
    pub titles: Vec<String>,
    pub total: usize,
}

#[derive(Deserialize)]
struct BrowseResponse {
    #[serde(default, rename = "release-group-count")]
    count: usize,
    #[serde(default, rename = "release-groups")]
    release_groups: Vec<ReleaseGroup>,
}

#[derive(Deserialize)]
struct ReleaseGroup {
    #[serde(default)]
    title: String,
}

#[derive(Deserialize)]
struct ArtistResponse {
    #[serde(default)]
    id: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default, rename = "life-span")]
    life_span: Option<LifeSpan>,
    #[serde(default)]
    area: Option<Named>,
    #[serde(default, rename = "begin-area")]
    begin_area: Option<Named>,
    #[serde(default)]
    genres: Vec<Genre>,
    #[serde(default)]
    relations: Vec<Relation>,
}

#[derive(Deserialize)]
struct Genre {
    #[serde(default)]
    name: String,
    #[serde(default)]
    count: i64,
}

#[derive(Deserialize)]
struct LifeSpan {
    #[serde(default)]
    begin: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    ended: Option<bool>,
}

#[derive(Deserialize)]
struct Relation {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    direction: String,
    #[serde(default)]
    ended: bool,
    #[serde(default)]
    url: Option<Url>,
    #[serde(default)]
    artist: Option<Named>,
}

#[derive(Deserialize)]
struct Url {
    #[serde(default)]
    resource: String,
}

pub fn search(agent: &Agent, name: &str, limit: usize) -> Result<Vec<Candidate>, Error> {
    let body = get_text(
        agent
            .get(format!("{ROOT}/artist/"))
            .query("query", query(name))
            .query("fmt", "json")
            .query("limit", limit.to_string()),
    )?;
    parse_search(&body)
}

pub fn release_group_titles(
    agent: &Agent,
    artist_id: &str,
    offset: usize,
) -> Result<ReleaseGroupPage, Error> {
    let body = get_text(
        agent
            .get(format!("{ROOT}/release-group"))
            .query("artist", artist_id)
            .query("fmt", "json")
            .query("limit", RELEASE_GROUP_PAGE.to_string())
            .query("offset", offset.to_string()),
    )?;
    parse_release_groups(&body)
}

pub fn details(agent: &Agent, artist_id: &str) -> Result<ArtistFacts, Error> {
    let body = get_text(
        agent
            .get(format!("{ROOT}/artist/{artist_id}"))
            .query("inc", "url-rels+genres+artist-rels")
            .query("fmt", "json"),
    )?;
    parse_details(&body)
}

pub fn query(name: &str) -> String {
    let phrase = phrase(name);
    format!("artist:{phrase} OR alias:{phrase}")
}

fn phrase(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

pub fn parse_search(body: &str) -> Result<Vec<Candidate>, Error> {
    let response: SearchResponse =
        serde_json::from_str(body).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(response
        .artists
        .into_iter()
        .filter(|artist| !artist.id.is_empty())
        .map(|artist| Candidate {
            id: artist.id,
            name: artist.name,
            aliases: artist.aliases.into_iter().map(|alias| alias.name).collect(),
        })
        .collect())
}

pub fn parse_release_groups(body: &str) -> Result<ReleaseGroupPage, Error> {
    let response: BrowseResponse =
        serde_json::from_str(body).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(ReleaseGroupPage {
        titles: response
            .release_groups
            .into_iter()
            .map(|group| group.title)
            .collect(),
        total: response.count,
    })
}

pub fn parse_details(body: &str) -> Result<ArtistFacts, Error> {
    let response: ArtistResponse =
        serde_json::from_str(body).map_err(|e| Error::Parse(e.to_string()))?;
    let kind = match response.kind.as_deref() {
        Some("Person") => Kind::Person,
        Some("Group" | "Orchestra" | "Choir") => Kind::Group,
        _ => Kind::Other,
    };
    let span = response.life_span;
    let year = |date: Option<&String>| date.and_then(|d| d.get(..4)?.parse::<i32>().ok());
    let mut genres = response.genres;
    genres.sort_by_key(|genre| std::cmp::Reverse(genre.count));
    let mut members = Vec::new();
    let mut member_of = Vec::new();
    let mut deezer = None;
    for relation in &response.relations {
        if let Some(artist) = &relation.artist
            && relation.kind == "member of band"
        {
            let list = match relation.direction.as_str() {
                "backward" => &mut members,
                _ => &mut member_of,
            };
            add_membership(list, &artist.name, !relation.ended);
        } else if let Some(url) = &relation.url
            && deezer.is_none()
        {
            deezer = deezer_artist_id(&url.resource);
        }
    }
    let name = |area: Option<Named>| area.map(|a| a.name).filter(|n| !n.is_empty());
    Ok(ArtistFacts {
        mbid: response.id,
        kind,
        begin_year: year(span.as_ref().and_then(|s| s.begin.as_ref())),
        end_year: year(span.as_ref().and_then(|s| s.end.as_ref())),
        ended: span.as_ref().and_then(|s| s.ended).unwrap_or(false),
        area: name(response.area),
        begin_area: name(response.begin_area),
        genres: genres
            .into_iter()
            .map(|genre| genre.name)
            .filter(|name| !name.is_empty())
            .take(GENRES_KEPT)
            .collect(),
        members,
        member_of,
        deezer,
    })
}

fn add_membership(list: &mut Vec<Membership>, name: &str, current: bool) {
    if name.is_empty() {
        return;
    }
    match list.iter_mut().find(|m| m.name == name) {
        Some(existing) => existing.current |= current,
        None => list.push(Membership {
            name: name.to_string(),
            current,
        }),
    }
}

fn deezer_artist_id(url: &str) -> Option<String> {
    let rest = url.split_once("deezer.com/")?.1;
    let after = rest.split_once("artist/")?.1;
    let id: String = after.chars().take_while(char::is_ascii_digit).collect();
    (!id.is_empty()).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn search_keeps_names_and_aliases() {
        let body = r#"{"created":"x","count":2,"offset":0,"artists":[
            {"id":"a1","score":100,"name":"Korn","aliases":[{"name":"KoЯn"},{"name":"Korn"}]},
            {"id":"a2","score":90,"name":"Korn Ferry"}]}"#;
        let found = parse_search(body).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, "a1");
        assert_eq!(found[0].aliases, vec!["KoЯn", "Korn"]);
        assert!(found[1].aliases.is_empty());
    }

    #[test]
    fn invalid_body_is_a_failure() {
        assert!(matches!(parse_search("Rate limit"), Err(Error::Parse(_))));
    }

    #[test]
    fn release_group_titles_are_listed() {
        let body = r#"{"release-group-count":2,"release-groups":[
            {"id":"r1","title":"Mutter","primary-type":"Album"},
            {"id":"r2","title":"Sonne","primary-type":"Single"}]}"#;
        let page = parse_release_groups(body).unwrap();
        assert_eq!(page.titles, vec!["Mutter", "Sonne"]);
        assert_eq!(page.total, 2);
    }

    #[test]
    fn details_read_facts_and_links() {
        let body = r#"{"id":"b2d1","type":"Group",
            "life-span":{"begin":"1994-01","end":null,"ended":false},
            "area":{"name":"Germany"},"begin-area":{"name":"Berlin"},
            "genres":[{"name":"metal","count":3},{"name":"industrial metal","count":12},
                      {"name":"neue deutsche härte","count":8}],
            "relations":[
                {"type":"free streaming","url":{"resource":"https://open.spotify.com/artist/6wWV"}},
                {"type":"free streaming","url":{"resource":"https://www.deezer.com/artist/464"}},
                {"type":"member of band","direction":"backward","ended":false,
                 "artist":{"name":"Till Lindemann"}},
                {"type":"member of band","direction":"backward","ended":true,
                 "artist":{"name":"Former One"}},
                {"type":"member of band","direction":"backward","ended":false,
                 "artist":{"name":"Former One"}},
                {"type":"wikidata","url":{"resource":"https://www.wikidata.org/wiki/Q22134"}}]}"#;
        let facts = parse_details(body).unwrap();
        assert_eq!(facts.mbid, "b2d1");
        assert_eq!(facts.kind, Kind::Group);
        assert_eq!((facts.begin_year, facts.end_year), (Some(1994), None));
        assert!(!facts.ended);
        assert_eq!(facts.area.as_deref(), Some("Germany"));
        assert_eq!(facts.begin_area.as_deref(), Some("Berlin"));
        assert_eq!(
            facts.genres,
            vec!["industrial metal", "neue deutsche härte", "metal"]
        );
        assert_eq!(
            facts.members,
            vec![
                Membership {
                    name: "Till Lindemann".into(),
                    current: true
                },
                Membership {
                    name: "Former One".into(),
                    current: true
                },
            ]
        );
        assert!(facts.member_of.is_empty());
        assert_eq!(facts.deezer.as_deref(), Some("464"));
    }

    #[test]
    fn person_lists_the_bands_they_belong_to() {
        let body = r#"{"id":"e1","type":"Person","relations":[
            {"type":"member of band","direction":"forward","ended":true,"artist":{"name":"D12"}},
            {"type":"member of band","direction":"forward","ended":false,"artist":{"name":"Bad Meets Evil"}}]}"#;
        let facts = parse_details(body).unwrap();
        assert_eq!(facts.kind, Kind::Person);
        assert!(facts.members.is_empty());
        assert_eq!(
            facts
                .member_of
                .iter()
                .map(|m| (m.name.as_str(), m.current))
                .collect::<Vec<_>>(),
            vec![("D12", false), ("Bad Meets Evil", true)]
        );
    }

    #[test]
    fn details_without_span_or_links() {
        let facts = parse_details(r#"{"id":"x","type":"Person"}"#).unwrap();
        assert_eq!(facts.kind, Kind::Person);
        assert_eq!(facts.begin_year, None);
        assert_eq!(facts.area, None);
        assert!(facts.genres.is_empty());
        assert_eq!(facts.deezer, None);
    }

    #[test]
    fn ended_group_has_both_years() {
        let body =
            r#"{"type":"Group","life-span":{"begin":"1964","end":"2014-07-02","ended":true}}"#;
        let facts = parse_details(body).unwrap();
        assert_eq!((facts.begin_year, facts.end_year), (Some(1964), Some(2014)));
        assert!(facts.ended);
    }

    #[test]
    fn facts_survive_a_round_trip_and_missing_fields() {
        let facts = ArtistFacts {
            mbid: "m".into(),
            kind: Kind::Group,
            genres: vec!["rock".into()],
            ..ArtistFacts::default()
        };
        let json = serde_json::to_string(&facts).unwrap();
        assert_eq!(serde_json::from_str::<ArtistFacts>(&json).unwrap(), facts);
        let old: ArtistFacts = serde_json::from_str(r#"{"mbid":"m"}"#).unwrap();
        assert_eq!(old.kind, Kind::Other);
    }

    #[rstest]
    #[case::plain("https://www.deezer.com/artist/464", Some("464"))]
    #[case::localized("https://www.deezer.com/en/artist/1477045", Some("1477045"))]
    #[case::trailing("https://deezer.com/artist/27/", Some("27"))]
    #[case::album("https://www.deezer.com/album/302127", None)]
    #[case::other_site("https://open.spotify.com/artist/464", None)]
    fn deezer_ids(#[case] url: &str, #[case] expected: Option<&str>) {
        assert_eq!(deezer_artist_id(url).as_deref(), expected);
    }

    #[test]
    fn query_searches_names_and_aliases_as_phrases() {
        assert_eq!(
            query(r#"Say "Hi""#),
            r#"artist:"Say \"Hi\"" OR alias:"Say \"Hi\"""#
        );
    }
}
