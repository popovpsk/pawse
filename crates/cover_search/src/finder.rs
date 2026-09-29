use std::time::{Duration, Instant};

use ureq::Agent;

use crate::candidate::Candidate;
use crate::http::{Error, agent, get_bytes};
use crate::matching::is_exact;
use crate::{itunes, musicbrainz};

const ITUNES_GAP: Duration = Duration::from_secs(3);
const MUSICBRAINZ_GAP: Duration = Duration::from_millis(1100);
const SEARCH_LIMIT: usize = 10;
const EXACT_TRIES: usize = 3;
pub const THUMBNAIL_SIZE: u32 = 100;
pub const FULL_SIZE: u32 = 1200;

#[derive(Debug, Clone)]
pub struct Found {
    pub candidate: Candidate,
    pub exact: bool,
    pub thumbnail: Vec<u8>,
}

pub struct Finder {
    agent: Agent,
    itunes_next: Option<Instant>,
    musicbrainz_next: Option<Instant>,
}

impl Default for Finder {
    fn default() -> Self {
        Self::new()
    }
}

impl Finder {
    pub fn new() -> Self {
        Self {
            agent: agent(),
            itunes_next: None,
            musicbrainz_next: None,
        }
    }

    pub fn find(&mut self, artist: &str, album: &str) -> Result<Option<Found>, Error> {
        if artist.trim().is_empty() || album.trim().is_empty() {
            return Ok(None);
        }
        let mut failure = None;

        wait(&mut self.itunes_next, ITUNES_GAP);
        let from_itunes =
            itunes::search(&self.agent, artist, album, SEARCH_LIMIT).unwrap_or_else(|e| {
                log::warn!("cover search: iTunes failed for {artist} — {album}: {e}");
                failure = Some(e);
                Vec::new()
            });
        if let Some(found) = self.first_with_art(&from_itunes, artist, album, &mut failure) {
            return Ok(Some(found));
        }

        wait(&mut self.musicbrainz_next, MUSICBRAINZ_GAP);
        let from_musicbrainz = musicbrainz::search(&self.agent, artist, album, SEARCH_LIMIT)
            .unwrap_or_else(|e| {
                log::warn!("cover search: MusicBrainz failed for {artist} — {album}: {e}");
                failure = Some(e);
                Vec::new()
            });
        if let Some(found) = self.first_with_art(&from_musicbrainz, artist, album, &mut failure) {
            return Ok(Some(found));
        }

        for top in [from_itunes.first(), from_musicbrainz.first()]
            .into_iter()
            .flatten()
        {
            if let Some(thumbnail) = self.thumbnail(top, &mut failure) {
                return Ok(Some(Found {
                    candidate: top.clone(),
                    exact: false,
                    thumbnail,
                }));
            }
        }

        match failure {
            Some(e) => Err(e),
            None => Ok(None),
        }
    }

    pub fn download(&self, candidate: &Candidate) -> Result<Vec<u8>, Error> {
        get_bytes(&self.agent, &candidate.art_url(FULL_SIZE))
    }

    fn first_with_art(
        &self,
        candidates: &[Candidate],
        artist: &str,
        album: &str,
        failure: &mut Option<Error>,
    ) -> Option<Found> {
        candidates
            .iter()
            .filter(|c| is_exact(artist, album, c))
            .take(EXACT_TRIES)
            .find_map(|candidate| {
                self.thumbnail(candidate, failure).map(|thumbnail| Found {
                    candidate: candidate.clone(),
                    exact: true,
                    thumbnail,
                })
            })
    }

    fn thumbnail(&self, candidate: &Candidate, failure: &mut Option<Error>) -> Option<Vec<u8>> {
        match get_bytes(&self.agent, &candidate.art_url(THUMBNAIL_SIZE)) {
            Ok(bytes) if crate::image_extension(&bytes).is_some() => Some(bytes),
            Ok(_) | Err(Error::Status(404)) => None,
            Err(e) => {
                log::warn!(
                    "cover search: art for {} — {} failed: {e}",
                    candidate.artist,
                    candidate.album
                );
                *failure = Some(e);
                None
            }
        }
    }
}

fn wait(next: &mut Option<Instant>, gap: Duration) {
    if let Some(at) = *next {
        let now = Instant::now();
        if at > now {
            std::thread::sleep(at - now);
        }
    }
    *next = Some(Instant::now() + gap);
}
