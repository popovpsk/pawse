use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use music_library::{LibraryRepository, Track, normalize_tag};
use rand::Rng;

use super::rerank::{Candidate, Rerank, Taken};

pub struct Pool {
    candidates: Vec<Candidate>,
    tracks: HashMap<i64, Track>,
}

impl Pool {
    pub fn load(
        repo: &dyn LibraryRepository,
        found: Vec<Vec<(i64, f32)>>,
    ) -> music_library::Result<Self> {
        let mut seen = HashSet::new();
        let found: Vec<(i64, f32)> = found
            .into_iter()
            .flatten()
            .filter(|(id, _)| seen.insert(*id))
            .collect();
        let ids: Vec<i64> = found.iter().map(|(id, _)| *id).collect();
        let artists = repo.track_artists_map(&ids)?;
        let stats = repo.play_stats(&ids)?;
        let mut candidates = Vec::with_capacity(found.len());
        let mut tracks = HashMap::with_capacity(found.len());
        for (id, score) in found {
            let Some(track) = repo.track(id)? else {
                continue;
            };
            let played = stats.get(&id).copied().unwrap_or_default();
            candidates.push(Candidate {
                id,
                score,
                artist: first_artist(&artists, id),
                title: normalize_tag(&track.title),
                duration_ms: track.duration_ms,
                available: track.available,
                liked: track.liked,
                plays: played.plays,
                last_played: played.last_played,
            });
            tracks.insert(id, track);
        }
        Ok(Self { candidates, tracks })
    }

    pub fn pick(self, rerank: &Rerank, taken: Taken, rng: &mut impl Rng) -> Vec<Track> {
        let Self {
            candidates,
            mut tracks,
        } = self;
        rerank
            .run(candidates, taken, rng)
            .into_iter()
            .filter_map(|id| tracks.remove(&id))
            .collect()
    }
}

pub fn seed_taken(repo: &dyn LibraryRepository, seed: &Track) -> music_library::Result<Taken> {
    let artists = repo.track_artists_map(&[seed.id])?;
    let mut taken = Taken::default();
    taken.add(
        &first_artist(&artists, seed.id),
        &normalize_tag(&seed.title),
    );
    Ok(taken)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn first_artist(artists: &HashMap<i64, Vec<String>>, id: i64) -> String {
    artists
        .get(&id)
        .and_then(|names| names.first())
        .map(|name| normalize_tag(name))
        .unwrap_or_default()
}
