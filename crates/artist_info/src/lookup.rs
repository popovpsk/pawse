use std::collections::HashSet;
use std::time::Duration;

use cover_search::matching::normalize;
use cover_search::{Error, agent, get_bytes, musicbrainz_turn};
use ureq::Agent;

use crate::deezer;
use crate::musicbrainz::{self, ArtistFacts, Candidate, VARIOUS_ARTISTS};

const BUSY_RETRIES: u32 = 2;
const BUSY_PAUSE: Duration = Duration::from_secs(2);
const SEARCH_LIMIT: usize = 10;
const MAX_CANDIDATES: usize = 4;
const MAX_RELEASE_GROUP_PAGES: usize = 3;
const WEAK_HITS_NEEDED: usize = 2;

pub struct Query<'a> {
    pub name: &'a str,
    pub albums: &'a [String],
    pub tracks: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub facts: ArtistFacts,
    pub photo: Option<Vec<u8>>,
    pub photo_pending: bool,
}

#[derive(Clone)]
pub struct Lookup {
    agent: Agent,
}

impl Default for Lookup {
    fn default() -> Self {
        Self::new()
    }
}

impl Lookup {
    pub fn new() -> Self {
        Self { agent: agent() }
    }

    pub fn find(&self, query: &Query) -> Result<Option<Found>, Error> {
        let Some(artist_id) = self.identify(query)? else {
            return Ok(None);
        };
        let facts = self.musicbrainz(|agent| musicbrainz::details(agent, &artist_id))?;
        let (photo, photo_pending) = match facts.deezer.as_deref().map(|id| self.photo(id)) {
            None => (None, false),
            Some(Ok(photo)) => (photo, false),
            Some(Err(e)) => {
                log::warn!("artist info: photo failed for {}: {e}", query.name);
                (None, true)
            }
        };
        Ok(Some(Found {
            facts,
            photo,
            photo_pending,
        }))
    }

    pub fn photo(&self, deezer_id: &str) -> Result<Option<Vec<u8>>, Error> {
        match deezer::picture_url(&self.agent, deezer_id)? {
            Some(url) => get_bytes(&self.agent, &url).map(Some),
            None => Ok(None),
        }
    }

    fn identify(&self, query: &Query) -> Result<Option<String>, Error> {
        let local = LocalTitles::new(query.name, query.albums, query.tracks);
        if local.is_empty() {
            return Ok(None);
        }
        let found =
            self.musicbrainz(|agent| musicbrainz::search(agent, query.name, SEARCH_LIMIT))?;
        let candidates = found
            .iter()
            .filter(|candidate| {
                candidate.id != VARIOUS_ARTISTS && names_match(query.name, candidate)
            })
            .take(MAX_CANDIDATES);
        for candidate in candidates {
            if self.confirm(&local, &candidate.id)? {
                return Ok(Some(candidate.id.clone()));
            }
        }
        Ok(None)
    }

    fn confirm(&self, local: &LocalTitles, artist_id: &str) -> Result<bool, Error> {
        let mut seen: Vec<String> = Vec::new();
        for _ in 0..MAX_RELEASE_GROUP_PAGES {
            let offset = seen.len();
            let page = self
                .musicbrainz(|agent| musicbrainz::release_group_titles(agent, artist_id, offset))?;
            let last = page.titles.is_empty();
            seen.extend(page.titles);
            if local.confirmed_by(&seen) {
                return Ok(true);
            }
            if last || seen.len() >= page.total {
                break;
            }
        }
        Ok(false)
    }

    fn musicbrainz<T>(&self, call: impl Fn(&Agent) -> Result<T, Error>) -> Result<T, Error> {
        let mut retries = 0;
        loop {
            musicbrainz_turn();
            match call(&self.agent) {
                Err(Error::Status(503)) if retries < BUSY_RETRIES => {
                    retries += 1;
                    std::thread::sleep(BUSY_PAUSE * retries);
                }
                result => return result,
            }
        }
    }
}

pub fn names_match(name: &str, candidate: &Candidate) -> bool {
    let wanted = normalize(name);
    normalize(&candidate.name) == wanted
        || candidate
            .aliases
            .iter()
            .any(|alias| normalize(alias) == wanted)
}

struct LocalTitles {
    name: String,
    albums: HashSet<String>,
    tracks: HashSet<String>,
}

impl LocalTitles {
    fn new(name: &str, albums: &[String], tracks: &[String]) -> Self {
        let keys = |titles: &[String]| {
            titles
                .iter()
                .filter(|title| !title.trim().is_empty())
                .map(|title| normalize(title))
                .collect()
        };
        Self {
            name: normalize(name),
            albums: keys(albums),
            tracks: keys(tracks),
        }
    }

    fn is_empty(&self) -> bool {
        self.albums.is_empty() && self.tracks.is_empty()
    }

    fn confirmed_by(&self, release_groups: &[String]) -> bool {
        let remote: HashSet<String> = release_groups.iter().map(|t| normalize(t)).collect();
        let mut weak_hits = 0;
        for title in &remote {
            let album = self.albums.contains(title);
            if album && *title != self.name {
                return true;
            }
            if album || self.tracks.contains(title) {
                weak_hits += 1;
            }
        }
        weak_hits >= WEAK_HITS_NEEDED
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn titles(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn candidate(name: &str, aliases: &[&str]) -> Candidate {
        Candidate {
            id: "id".into(),
            name: name.into(),
            aliases: titles(aliases),
        }
    }

    #[rstest]
    #[case::exact("Rammstein", candidate("Rammstein", &[]), true)]
    #[case::case_and_article("the strokes", candidate("The Strokes", &[]), true)]
    #[case::alias("KoЯn", candidate("Korn", &["KoЯn"]), true)]
    #[case::other_artist("Muse", candidate("Muse Ensemble", &[]), false)]
    fn name_matching(#[case] name: &str, #[case] candidate: Candidate, #[case] expected: bool) {
        assert_eq!(names_match(name, &candidate), expected);
    }

    #[rstest]
    #[case::album_hit(&["Mutter"], &[], &["Mutter", "Sehnsucht"], true)]
    #[case::album_edition(&["Mutter (Deluxe Edition)"], &[], &["Mutter"], true)]
    #[case::two_track_hits(&[], &["Do I Wanna Know?", "R U Mine?", "Arabella"], &["R U Mine?", "Do I Wanna Know?"], true)]
    #[case::one_track_hit(&[], &["Intro", "Ritual"], &["Intro", "Elsewhere"], false)]
    #[case::same_track_twice(&[], &["Intro"], &["Intro", "Intro"], false)]
    #[case::nothing_in_common(&["Iowa"], &["People = Shit"], &["Something Else"], false)]
    #[case::self_titled_alone(&["Band"], &[], &["Band", "Other"], false)]
    #[case::self_titled_and_a_track(&["Band"], &["Ritual"], &["Band", "Ritual"], true)]
    #[case::self_titled_edition(&["Band (Deluxe Edition)"], &[], &["Band"], false)]
    fn confirmation(
        #[case] albums: &[&str],
        #[case] tracks: &[&str],
        #[case] remote: &[&str],
        #[case] expected: bool,
    ) {
        let local = LocalTitles::new("Band", &titles(albums), &titles(tracks));
        assert_eq!(local.confirmed_by(&titles(remote)), expected);
    }

    #[test]
    fn blank_titles_cannot_confirm() {
        assert!(LocalTitles::new("Band", &titles(&["", " "]), &[]).is_empty());
    }
}
