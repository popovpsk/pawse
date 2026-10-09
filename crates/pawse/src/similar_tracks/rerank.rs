use std::collections::{HashMap, HashSet};

use rand::Rng;
use serde::{Deserialize, Serialize};

pub const MIN_DURATION_MS: i64 = 60_000;
pub const MIN_SCORE: f32 = 0.0;
pub const RECENT_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Familiarity {
    Familiar,
    #[default]
    Any,
    New,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub artist: f32,
    pub repeat: f32,
    pub recent: f32,
    pub familiarity: f32,
    pub noise: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            artist: 0.1,
            repeat: 0.1,
            recent: 0.2,
            familiarity: 0.15,
            noise: 0.04,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: i64,
    pub score: f32,
    pub artist: String,
    pub title: String,
    pub duration_ms: Option<i64>,
    pub available: bool,
    pub liked: bool,
    pub plays: u32,
    pub last_played: Option<u64>,
}

#[derive(Debug, Default)]
pub struct Taken {
    artists: HashMap<String, usize>,
    songs: HashSet<String>,
    last_artist: Option<String>,
}

impl Taken {
    pub fn add(&mut self, artist: &str, title: &str) {
        if artist.is_empty() {
            self.last_artist = None;
            return;
        }
        *self.artists.entry(artist.to_string()).or_default() += 1;
        self.songs.insert(song_key(artist, title));
        self.last_artist = Some(artist.to_string());
    }

    fn penalty(&self, artist: &str, weights: &Weights) -> f32 {
        if artist.is_empty() {
            return 0.0;
        }
        let count = self.artists.get(artist).copied().unwrap_or(0) as f32;
        let repeat = if self.last_artist.as_deref() == Some(artist) {
            weights.repeat
        } else {
            0.0
        };
        weights.artist * count + repeat
    }
}

pub struct Rerank {
    pub count: usize,
    pub familiarity: Familiarity,
    pub now: u64,
    pub weights: Weights,
}

struct Entry {
    base: f32,
    song: Option<String>,
    candidate: Candidate,
}

impl Rerank {
    pub fn run(
        &self,
        candidates: Vec<Candidate>,
        mut taken: Taken,
        rng: &mut impl Rng,
    ) -> Vec<i64> {
        let mut pool: Vec<Entry> = candidates
            .into_iter()
            .filter(|c| {
                c.available
                    && c.score >= MIN_SCORE
                    && c.duration_ms.is_none_or(|ms| ms >= MIN_DURATION_MS)
            })
            .map(|candidate| Entry {
                base: self.base(&candidate) + self.weights.noise * gumbel(rng),
                song: (!candidate.artist.is_empty())
                    .then(|| song_key(&candidate.artist, &candidate.title)),
                candidate,
            })
            .filter(|entry| entry.song.as_ref().is_none_or(|s| !taken.songs.contains(s)))
            .collect();
        let mut picked = Vec::with_capacity(self.count.min(pool.len()));
        while picked.len() < self.count {
            let best = pool
                .iter()
                .enumerate()
                .map(|(ix, entry)| {
                    let value = entry.base - taken.penalty(&entry.candidate.artist, &self.weights);
                    (ix, value)
                })
                .max_by(|a, b| a.1.total_cmp(&b.1));
            let Some((ix, _)) = best else {
                break;
            };
            let entry = pool.swap_remove(ix);
            taken.add(&entry.candidate.artist, &entry.candidate.title);
            if let Some(song) = &entry.song {
                pool.retain(|other| other.song.as_ref() != Some(song));
            }
            picked.push(entry.candidate.id);
        }
        picked
    }

    fn base(&self, c: &Candidate) -> f32 {
        let known = c.liked || c.plays > 0;
        let bonus = match self.familiarity {
            Familiarity::Familiar if known => self.weights.familiarity,
            Familiarity::New if !known => self.weights.familiarity,
            _ => 0.0,
        };
        let recent = c
            .last_played
            .is_some_and(|at| self.now.saturating_sub(at) < RECENT_SECS);
        c.score + bonus - if recent { self.weights.recent } else { 0.0 }
    }
}

fn song_key(artist: &str, title: &str) -> String {
    format!("{artist}\u{0}{title}")
}

fn gumbel(rng: &mut impl Rng) -> f32 {
    let u: f32 = rng.random_range(f32::EPSILON..1.0);
    -(-u.ln()).ln()
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use rstest::rstest;

    use super::*;

    const NOW: u64 = 1_000_000_000;

    fn candidate(id: i64, score: f32, artist: &str) -> Candidate {
        Candidate {
            id,
            score,
            artist: artist.into(),
            title: format!("t{id}"),
            duration_ms: Some(200_000),
            available: true,
            liked: false,
            plays: 0,
            last_played: None,
        }
    }

    fn rerank(count: usize, familiarity: Familiarity) -> Rerank {
        Rerank {
            count,
            familiarity,
            now: NOW,
            weights: Weights {
                noise: 0.0,
                ..Weights::default()
            },
        }
    }

    fn seeded(artist: &str) -> Taken {
        let mut taken = Taken::default();
        taken.add(artist, "seed");
        taken
    }

    fn run(rerank: &Rerank, candidates: Vec<Candidate>, taken: Taken) -> Vec<i64> {
        rerank.run(candidates, taken, &mut StdRng::seed_from_u64(7))
    }

    #[test]
    fn without_noise_or_shared_artists_the_order_is_the_score() {
        let picked = run(
            &rerank(10, Familiarity::Any),
            vec![
                candidate(1, 0.5, "B"),
                candidate(2, 0.9, "C"),
                candidate(3, 0.7, "D"),
            ],
            seeded("A"),
        );
        assert_eq!(picked, vec![2, 3, 1]);
    }

    #[test]
    fn the_seed_artist_is_spread_between_other_artists() {
        let picked = run(
            &rerank(5, Familiarity::Any),
            vec![
                candidate(1, 0.90, "A"),
                candidate(2, 0.89, "A"),
                candidate(3, 0.86, "A"),
                candidate(4, 0.82, "B"),
                candidate(5, 0.75, "C"),
            ],
            seeded("A"),
        );
        assert_eq!(picked, vec![4, 1, 5, 2, 3]);
    }

    #[test]
    fn one_artist_never_plays_twice_in_a_row_while_others_are_close() {
        let picked = run(
            &rerank(4, Familiarity::Any),
            vec![
                candidate(1, 0.90, "B"),
                candidate(2, 0.88, "B"),
                candidate(3, 0.80, "C"),
                candidate(4, 0.76, "D"),
            ],
            Taken::default(),
        );
        assert_eq!(picked, vec![1, 3, 2, 4]);
    }

    #[test]
    fn skits_unavailable_tracks_and_seen_songs_are_left_out() {
        let mut short = candidate(1, 0.9, "B");
        short.duration_ms = Some(MIN_DURATION_MS - 1);
        let mut gone = candidate(2, 0.9, "C");
        gone.available = false;
        let mut unknown_length = candidate(3, 0.8, "D");
        unknown_length.duration_ms = None;
        let mut seed_again = candidate(4, 0.95, "A");
        seed_again.title = "seed".into();
        let mut single = candidate(5, 0.7, "E");
        single.title = "song".into();
        let mut album_version = candidate(6, 0.6, "E");
        album_version.title = "song".into();

        let picked = run(
            &rerank(10, Familiarity::Any),
            vec![
                short,
                gone,
                unknown_length,
                seed_again,
                single,
                album_version,
            ],
            seeded("A"),
        );
        assert_eq!(picked, vec![3, 5]);
    }

    #[test]
    fn tracks_less_alike_than_a_random_pair_are_left_out() {
        let picked = run(
            &rerank(10, Familiarity::New),
            vec![
                candidate(1, 0.3, "B"),
                candidate(2, MIN_SCORE, "C"),
                candidate(3, MIN_SCORE - 0.01, "D"),
            ],
            Taken::default(),
        );
        assert_eq!(picked, vec![1, 2]);
    }

    #[test]
    fn untagged_tracks_are_neither_penalised_nor_deduplicated() {
        let picked = run(
            &rerank(10, Familiarity::Any),
            vec![
                candidate(1, 0.9, ""),
                candidate(2, 0.8, ""),
                candidate(3, 0.7, ""),
            ],
            seeded(""),
        );
        assert_eq!(picked, vec![1, 2, 3]);
    }

    #[rstest]
    #[case::familiar(Familiarity::Familiar, vec![2, 3, 1])]
    #[case::any(Familiarity::Any, vec![1, 2, 3])]
    #[case::new(Familiarity::New, vec![1, 2, 3])]
    fn familiarity_prefers_known_or_unknown_tracks(
        #[case] familiarity: Familiarity,
        #[case] expected: Vec<i64>,
    ) {
        let unplayed = candidate(1, 0.90, "B");
        let mut played = candidate(2, 0.80, "C");
        played.plays = 3;
        played.last_played = Some(NOW - 30 * RECENT_SECS);
        let mut liked = candidate(3, 0.78, "D");
        liked.liked = true;

        let picked = run(
            &rerank(10, familiarity),
            vec![unplayed, played, liked],
            Taken::default(),
        );
        assert_eq!(picked, expected);
    }

    #[test]
    fn a_track_played_in_the_last_day_drops_behind() {
        let mut today = candidate(1, 0.90, "B");
        today.plays = 1;
        today.last_played = Some(NOW - RECENT_SECS + 60);
        let picked = run(
            &rerank(10, Familiarity::Familiar),
            vec![today, candidate(2, 0.86, "C")],
            Taken::default(),
        );
        assert_eq!(picked, vec![2, 1]);
    }

    #[test]
    fn the_list_stops_at_the_count() {
        let candidates = (1..50).map(|id| candidate(id, 0.5, "")).collect();
        let picked = run(&rerank(7, Familiarity::Any), candidates, Taken::default());
        assert_eq!(picked.len(), 7);
    }

    #[test]
    fn the_same_seed_repeats_the_list_and_another_seed_reshuffles_it() {
        let candidates: Vec<Candidate> = (1..80)
            .map(|id| candidate(id, 0.9 - id as f32 * 0.002, &format!("a{id}")))
            .collect();
        let rerank = Rerank {
            count: 20,
            familiarity: Familiarity::Any,
            now: NOW,
            weights: Weights::default(),
        };
        let with = |seed: u64| {
            rerank.run(
                candidates.clone(),
                Taken::default(),
                &mut StdRng::seed_from_u64(seed),
            )
        };
        assert_eq!(with(1), with(1));
        assert_ne!(with(1), with(2));
    }

    #[test]
    fn noise_never_buries_a_clearly_better_track() {
        let mut candidates: Vec<Candidate> = (2..80)
            .map(|id| candidate(id, 0.5, &format!("a{id}")))
            .collect();
        candidates.push(candidate(1, 0.9, "best"));
        let rerank = Rerank {
            count: 1,
            familiarity: Familiarity::Any,
            now: NOW,
            weights: Weights::default(),
        };
        for seed in 0..20 {
            let picked = rerank.run(
                candidates.clone(),
                Taken::default(),
                &mut StdRng::seed_from_u64(seed),
            );
            assert_eq!(picked, vec![1]);
        }
    }
}
