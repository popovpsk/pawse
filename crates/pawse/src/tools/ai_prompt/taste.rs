use std::collections::HashMap;

use music_library::{LibraryRepository, PlayTally, RecentPlay, TrackListing};

const RECENT_PLAYS: usize = 30;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlbumEntry {
    pub id: i64,
    pub artist: String,
    pub title: String,
    pub year: Option<i32>,
    pub genres: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct TasteSnapshot {
    pub period_tallies: Vec<PlayTally>,
    pub history: Vec<PlayTally>,
    pub recent: Vec<RecentPlay>,
    pub albums: Vec<AlbumEntry>,
    pub listings: Vec<TrackListing>,
}

impl TasteSnapshot {
    pub fn has_history(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn album_genres(&self) -> HashMap<i64, &[String]> {
        self.albums
            .iter()
            .map(|album| (album.id, album.genres.as_slice()))
            .collect()
    }
}

pub fn gather(
    repo: &dyn LibraryRepository,
    since: Option<u64>,
) -> music_library::Result<TasteSnapshot> {
    let history = repo.play_tallies(None)?;
    let period_tallies = match since {
        Some(_) => repo.play_tallies(since)?,
        None => history.clone(),
    };
    let mut genres = repo.album_genres_map()?;
    let albums = repo
        .albums()?
        .into_iter()
        .filter(|album| album.id != music_library::NO_METADATA_ALBUM_ID && !album.title.is_empty())
        .map(|album| AlbumEntry {
            genres: genres.remove(&album.id).unwrap_or_default(),
            id: album.id,
            artist: album.artist_name,
            title: album.title,
            year: album.year,
        })
        .collect();
    Ok(TasteSnapshot {
        period_tallies,
        history,
        recent: repo.recent_plays(RECENT_PLAYS)?,
        albums,
        listings: repo.track_listings()?,
    })
}
